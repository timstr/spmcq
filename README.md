# ringbuffer

A Rust library for a thread-safe single-producer, multiple-consumer bounded ring buffer (FIFO queue).

This library was written with real-time audio applications in mind, where a typical scenario would be something like the following:

-   An audio callback periodically receives buffers of new audio data from a microphone on one thread
-   A separate high-priorty audio thread needs to receive those buffers with minimum latency. This thread is assumed to read at the same frequency and so dropouts are generally unlikely
-   A low-priority GUI thread also receives the same audio data and displays it. The GUI thread runs less often, and can tolerate a much larger latency but still ideally receives the entire audio signal in batches of old data if it needs to. Dropouts due to the GUI not being repainted are tolerable and perhaps even expected.

Features:

-   Fixed-size capacity and no additional heap allocation after construction
-   Multiple readers
-   The writer may overtake readers without erroring or extra blocking, and readers can detect this scenario and may skip ahead
-   Low latency and low synchronization overhead. Both reads and writes consist of a simple spin lock and a single memcopy of the item.

## Basic Usage

The high-level API is somewhat similar to that of `std::sync::mpsc::sync_channel`. Choose a fixed size capacity and call `ring_buffer` with that size to receive a `Reader<T>` and a `Writer<T>`. Both can be passed to different threads, and `Reader<T>` can be cloned. `Writer::write` pushes a new value onto the buffer. `Reader::read` attempts to read the next value in its own sequence.

```rust
let (mut reader, mut writer) = ring_buffer::<usize>(256);

std::thread::spawn(move ||{
    for i in 1000 {
        writer.write(i);
        std::thread::sleep(Duration::from_millis(1))
    }
});

std::thread::spawn(move ||{
    loop {
        match reader.read() {
            ReadResult::Ok(i) => println!("Received {}", i),
            ReadResult::Dropout(i) => println!("Received {} but lost some values", i),
            ReadResult::Empty(i) => println("No new data"),
        }
        std::thread::sleep(Duration::from_millis(1));
    }
});
```

If a reader has fully caught up to the writer, `read()` will return `ReadResult::Empty` until more is written. If the reader is somewhere between the front and the back of the queue, `read()` will return `ReadResult::Ok(_)` containing its next value. Otherwise, if the writer has completely overtaken a reader, its `read()` method returns `ReadResult::Dropout(_)`, which informs that the reader has fallen at least one lap behind since its last read, but still returns a value from the current lap.

In order to skip a reader to the front of the queue, call `Reader::skip_ahead()`. The next read will always return `ReadResult::Dropout(_)`, but any accumulated latency can be cut down this way if dropped values are tolerable.

The stored data type `T` must be `Copy`. This constraint allows minimizing the time that readers spend holding a read lock on each item, since the lock must be held only long enough to do a memcpy of the item.

## TODO's

-   Allow readers to detect hang-ups if the writer is dropped?
