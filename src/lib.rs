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

pub struct Reader<T> {
    data: Arc<[Item<T>]>,
    write_index: Arc<AtomicUsize>,
    read_index: usize,
    lap_count: u16,
}

unsafe impl<T> Send for Reader<T> {}

pub struct Writer<T> {
    data: Arc<[Item<T>]>,
    write_index: Arc<AtomicUsize>,
    lap_count: u16,
}

unsafe impl<T> Send for Writer<T> {}

pub fn ring_buffer<T>(capacity: usize) -> (Reader<T>, Writer<T>)
where
    T: Default,
{
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadResult<T> {
    Ok(T),
    Dropout(T),
    Empty,
}

impl<T> ReadResult<T> {
    pub fn is_ok(&self) -> bool {
        match self {
            ReadResult::Ok(_) => true,
            _ => false,
        }
    }

    pub fn is_dropout(&self) -> bool {
        match self {
            ReadResult::Dropout(_) => true,
            _ => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            ReadResult::Empty => true,
            _ => false,
        }
    }

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
