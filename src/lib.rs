//! An implementation of a bounded array-based ring buffer / circular queue that
//! supports a single producer and multiple consumers (SPMC). Individual readers
//! / consumers may get overtaken by the writer / producer without either of them
//! blocking. Readers can detect when new data is available, when the queue is
//! empty, and when they have been overtaken. Readers may also skip to the front
//! of the queue.
//!
//! Currently, hang-ups are not detected. Additionally, the stored value needs
//! to implement `Copy` and `Default`.
//!
//! To use a ring buffer, call [ring_buffer] to receive a [Reader] and a [Writer].
//! Call [Writer::write] to push new data onto the queue and [Reader::read] to
//! receive the new data. Pass both readers and writer to different threads and
//! clone new readers as desired.

use std::{
    cell::UnsafeCell,
    sync::{
        atomic::{AtomicI16, AtomicUsize, Ordering},
        Arc,
    },
};

#[cfg(test)]
mod test;

struct Item<T> {
    // Use count by either readers or the writer, used for busy waiting and synchronization
    // and guarding access to data and lap_count
    //    0     -> not in use
    // positive -> in use by that many readers
    //   -1     -> in use by writer
    use_count: AtomicI16,

    // A simple counter for the number of times the writer had gone through the entire array
    // when it last wrote data to this item. Wraps upon overflow. Used to detect dropouts.
    lap_count: UnsafeCell<u16>,

    // the actual data being stored
    data: UnsafeCell<T>,
}

/// The receiving end of a ring buffer, which reads data from the [Writer] that it was
/// created with by calling [ring_buffer]. Call [Reader::read] to receive new data if
/// it's, available, and clone the reader to create additional readers.
pub struct Reader<T> {
    data: Arc<[Item<T>]>,
    write_index: Arc<AtomicUsize>,
    read_index: usize,
    lap_count: u16,
}

unsafe impl<T> Send for Reader<T> where T: Send {}

/// The sending end of a ring buffer, which passes data to any [Reader] instances
/// created from calling [ring_buffer]. Call [Writer::write] to make new data
/// available, at risk of overwriting old data and overtaking readers.
pub struct Writer<T> {
    data: Arc<[Item<T>]>,
    write_index: Arc<AtomicUsize>,
    lap_count: u16,
}

unsafe impl<T> Send for Writer<T> where T: Send {}

/// Construct a new ring buffer consisting of a [Reader] and a [Writer].
/// The internal buffer will have the specified capacity, and no
/// additional heap allocation will be performed by either the readers
/// or the writer. A larger capacity means that more past data will be
/// retained before being overwritten, and slower readers will have a
/// better chance of observing all data, though it also increases memory
/// usage.
///
/// # Panics
/// Panics if the capacity is less than 2.
pub fn ring_buffer<T>(capacity: usize) -> (Reader<T>, Writer<T>)
where
    T: Default,
{
    assert!(capacity >= 2);

    let mut data = Vec::<Item<T>>::new();
    data.resize_with(capacity, || Item {
        use_count: AtomicI16::new(0),
        data: UnsafeCell::new(T::default()),
        lap_count: UnsafeCell::new(0),
    });

    let data: Arc<[Item<T>]> = data.into_boxed_slice().into();

    let write_index = Arc::new(AtomicUsize::new(0));

    let reader = Reader {
        data: Arc::clone(&data),
        write_index: Arc::clone(&write_index),
        read_index: 0,
        // NOTE: the writer and writer lap counts must be 1 if the data lap counts are all zero,
        // see note in Reader::read
        lap_count: 1,
    };

    let writer = Writer {
        data,
        write_index,
        // NOTE: the writer and writer lap counts must be 1 if the data lap counts are all zero,
        // see note in Reader::read
        lap_count: 1,
    };

    (reader, writer)
}

/// The result of reading from a ring buffer by [Reader::read]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadResult<T> {
    /// New data was received, and the reader is somewhere in the middle of the queue.
    Ok(T),

    /// New data was received, but the reader was overtaken by the writer since its
    /// last read, and so some data was lost. Consider calling [Reader::skip_ahead]
    /// immediately before the next read to drop additional data but recover from
    /// any latency that might have accumulated.
    Dropout(T),

    /// The reader is at the very front of the queue and no new data is available.
    Empty,
}

impl<T> ReadResult<T> {
    /// Returns whether self is [ReadResult::Ok]
    pub fn is_ok(&self) -> bool {
        match self {
            ReadResult::Ok(_) => true,
            _ => false,
        }
    }

    /// Returns whether self is [ReadResult::Dropout]
    pub fn is_dropout(&self) -> bool {
        match self {
            ReadResult::Dropout(_) => true,
            _ => false,
        }
    }

    /// Returns whether self is [ReadResult::Empty]
    pub fn is_empty(&self) -> bool {
        match self {
            ReadResult::Empty => true,
            _ => false,
        }
    }

    /// If self is [ReadResult::Ok] or [ReadResult::Dropout], returns the
    /// received value. Otherwise, returns None.
    pub fn value(self) -> Option<T> {
        match self {
            ReadResult::Ok(v) => Some(v),
            ReadResult::Dropout(v) => Some(v),
            ReadResult::Empty => None,
        }
    }
}

impl<T> Reader<T>
where
    T: Copy,
{
    /// Receive the next item in the queue if anything is available.
    /// If the reader is somewhere in the middle of the queue, returns
    /// [ReadResult::Ok] with the next item. If the reader has beenovertaken
    /// by the writer since its last read, returns [ReadResult::Dropout]
    /// with a more recent item to indicate that some items were lost.
    /// Otherwise, if the reader is fully caught up to writer and no new
    /// data is available, returns [ReadResult::Empty].
    ///
    /// This method uses a spin lock and may busy-wait for a short duration
    /// if the writer happens to be writing to the same position as the
    /// reader. The guarded section performs only a trivial copy of the data.
    pub fn read(&mut self) -> ReadResult<T> {
        // Get the item to be read from
        let item = &self.data[self.read_index];

        // try to increment the use count, spin until the old use count was definitely positive
        let mut expected_use_count = 0;
        while let Err(actual_use_count) = item.use_count.compare_exchange(
            expected_use_count,
            expected_use_count + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            debug_assert!(actual_use_count >= -1, "Invalid use count");
            debug_assert!(actual_use_count < i16::MAX, "Reader overflow");
            expected_use_count = actual_use_count.max(0);
            std::hint::spin_loop();
        }

        // SAFETY: the spin loop above ensures that the use count wasn't -1 before and is positive
        // now. Thus, the writer will block until the use count is decremented again, thus this
        // read is guarded. Mutation is not safe because there could be multiple readers.

        // Copy the value then immediately leave the locked section to release the lock again to
        // prevent holding up the writer. T must be Copy for this reason.
        let value = unsafe { *item.data.get() };

        let value_lap_count = unsafe { *item.lap_count.get() };

        // Read lock is released here
        let final_use_count = item.use_count.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(final_use_count >= 0);

        let expected_lap_count = self.lap_count;

        if value_lap_count.wrapping_add(1) == expected_lap_count {
            // If the lap count is exactly one behind the expected lap count,
            // we just overtook the writer. Discard the value because it's
            // old and don't move.
            // NOTE that if all value lap counts are set to 0 initially, the
            // reader and writer must start with a lap count of 1 for the
            // buffer to appear empty to the reader when it is first constructed.
            return ReadResult::Empty;
        } else if value_lap_count != expected_lap_count {
            // If the lap count is off, we lost some values. Overwrite
            // the lap count to attempt to catch up with the reader.
            self.lap_count = value_lap_count;
        }

        // Move one index forward
        self.read_index += 1;
        if self.read_index == self.data.len() {
            self.read_index = 0;
            self.lap_count = self.lap_count.wrapping_add(1);
        }

        if value_lap_count == expected_lap_count {
            // If the lap count matches what we expected, all is normal.
            ReadResult::Ok(value)
        } else {
            // If the lap count is off, we lost some values in between
            ReadResult::Dropout(value)
        }
    }

    /// Immediately advance the reader to the front of the queue and catch
    /// up with the reader. This method should ideally only be used right
    /// before a call to [Reader::read], since otherwise the reader could
    /// overtake the writer again. The next result of reading will always be
    /// [ReadResult::Dropout] regardless of whether data was actually lost.
    ///
    /// Calling this method multiple times in between reads may result
    /// in the same item being observed multiple times.
    pub fn skip_ahead(&mut self) {
        // Because the write_index typically points to the index that the
        // writer is _going_ to write to, subtract one so that we point
        // the most-recently written item if not the second-most recent.
        self.read_index = self.write_index.load(Ordering::SeqCst);
        self.read_index = if self.read_index == 0 {
            self.data.len()
        } else {
            self.read_index
        } - 1;

        // Also adjust the lap count to (effectively) guarantee that the
        // next read returns Dropout
        self.lap_count = self.lap_count.wrapping_sub(1);
    }
}

impl<T> Clone for Reader<T> {
    fn clone(&self) -> Self {
        Self {
            data: Arc::clone(&self.data),
            write_index: Arc::clone(&self.write_index),
            read_index: self.read_index,
            lap_count: self.lap_count,
        }
    }
}

impl<T> Writer<T> {
    /// Write new data onto the queue, possibly overwriting old data. Any readers
    /// that were fully caught up will see the new data with [ReadResult::Ok],
    /// while any readers that get overtaken will see the new data but with
    /// [ReadResult::Dropout] instead.
    ///
    /// This method uses a spin lock and may busy-wait for a short duration if
    /// any readers happen to be actively reading from the very back of the
    /// queue. The guarded section is performs only a trivial copy of the data.
    pub fn write(&mut self, value: T) {
        // Get the current write index
        let index = self.write_index.load(Ordering::SeqCst);

        // fetch the item about to be written to
        let item = &self.data[index];

        // spin until use count is zero, write -1
        while let Err(actual_use_count) =
            item.use_count
                .compare_exchange(0, -1, Ordering::SeqCst, Ordering::SeqCst)
        {
            debug_assert!(actual_use_count > 0, "Invalid use count");

            std::hint::spin_loop();
        }

        // SAFETY: the spin loop above ensures that the use count was zero before and is now -1
        // This value indicates to all readers that the writer is busy here, and they will block
        // until it's non-negative again. Thus, there is no data race.
        unsafe {
            *item.data.get() = value;

            *item.lap_count.get() = self.lap_count;
        }

        // If the index wraps around, increment the lap count
        let mut next_index = index + 1;
        if next_index == self.data.len() {
            next_index = 0;
            self.lap_count = self.lap_count.wrapping_add(1);
        }

        // update the write index to be visible by readers
        self.write_index.store(next_index, Ordering::SeqCst);

        // release the write lock on the current item by assigning zero back to the use count.
        // The use count must still be -1, nothing should have modified it during writing.
        item.use_count
            .compare_exchange(-1, 0, Ordering::SeqCst, Ordering::SeqCst)
            .unwrap();
    }
}
