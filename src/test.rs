use std::time::Duration;

use crate::{ring_buffer, ReadResult};

#[test]
fn test_basic_use_one_thread() {
    let (mut reader, mut writer) = ring_buffer::<usize>(32);

    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());

    writer.write(1);

    assert_eq!(reader.read(), ReadResult::Ok(1));
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());

    writer.write(2);

    assert_eq!(reader.read(), ReadResult::Ok(2));
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());

    writer.write(3);

    assert_eq!(reader.read(), ReadResult::Ok(3));
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());

    writer.write(4);

    assert_eq!(reader.read(), ReadResult::Ok(4));
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());

    writer.write(5);
    writer.write(6);

    assert_eq!(reader.read(), ReadResult::Ok(5));
    assert_eq!(reader.read(), ReadResult::Ok(6));
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());

    writer.write(7);
    writer.write(8);
    writer.write(9);
    writer.write(10);

    assert_eq!(reader.read(), ReadResult::Ok(7));
    assert_eq!(reader.read(), ReadResult::Ok(8));
    assert_eq!(reader.read(), ReadResult::Ok(9));
    assert_eq!(reader.read(), ReadResult::Ok(10));
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());

    writer.write(11);
    writer.write(12);

    assert_eq!(reader.read(), ReadResult::Ok(11));

    writer.write(13);
    writer.write(14);
    writer.write(15);

    assert_eq!(reader.read(), ReadResult::Ok(12));

    writer.write(16);
    writer.write(17);
    writer.write(18);
    writer.write(19);

    assert_eq!(reader.read(), ReadResult::Ok(13));
    assert_eq!(reader.read(), ReadResult::Ok(14));
    assert_eq!(reader.read(), ReadResult::Ok(15));
    assert_eq!(reader.read(), ReadResult::Ok(16));
    assert_eq!(reader.read(), ReadResult::Ok(17));
    assert_eq!(reader.read(), ReadResult::Ok(18));
    assert_eq!(reader.read(), ReadResult::Ok(19));
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
    assert!(reader.read().is_empty());
}

#[test]
fn test_wraparound_keeping_pace_one_thread() {
    let (mut reader, mut writer) = ring_buffer::<usize>(32);

    for i in 0..1024 {
        assert!(reader.read().is_empty());
        assert!(reader.read().is_empty());
        assert!(reader.read().is_empty());
        assert!(reader.read().is_empty());

        writer.write(i);

        assert_eq!(reader.read(), ReadResult::Ok(i));
        assert!(reader.read().is_empty());
        assert!(reader.read().is_empty());
        assert!(reader.read().is_empty());
        assert!(reader.read().is_empty());
    }
}

#[test]
fn test_dropouts_lapped_once_one_thread() {
    let (mut reader, mut writer) = ring_buffer::<usize>(32);

    // one read, capacity+1 writes
    for i in 0..1024 {
        assert_eq!(reader.read(), ReadResult::Empty);

        for _ in 0..33 {
            writer.write(i);
        }

        assert_eq!(reader.read(), ReadResult::Dropout(i));
        assert_eq!(reader.read(), ReadResult::Empty);
    }
}

#[test]
fn test_dropouts_lapped_twice_one_thread() {
    let (mut reader, mut writer) = ring_buffer::<usize>(32);

    // one read, 2*capacity+1 writes
    for i in 0..1024 {
        assert_eq!(reader.read(), ReadResult::Empty);

        for _ in 0..65 {
            writer.write(i);
        }

        assert_eq!(reader.read(), ReadResult::Dropout(i));
        assert_eq!(reader.read(), ReadResult::Empty);
    }
}

#[test]
fn test_skip_ahead_basic_one_thread() {
    let (mut reader, mut writer) = ring_buffer::<usize>(32);

    writer.write(1);
    writer.write(2);
    writer.write(3);
    writer.write(4);

    assert_eq!(reader.read(), ReadResult::Ok(1));
    reader.skip_ahead();
    assert_eq!(reader.read(), ReadResult::Dropout(4));
    assert_eq!(reader.read(), ReadResult::Empty);

    writer.write(5);

    reader.skip_ahead();
    // Might seem a bit silly to return Dropout instead
    // of Ok if there weren't actually any items skipped,
    // but to call skip_ahead is basically to ask for items
    // to be skipped and its effect can't generally be know
    // ahead of time.
    assert_eq!(reader.read(), ReadResult::Dropout(5));
    assert_eq!(reader.read(), ReadResult::Empty);

    writer.write(6);
    writer.write(7);

    reader.skip_ahead();

    writer.write(8);
    writer.write(9);

    reader.skip_ahead();
    assert_eq!(reader.read(), ReadResult::Dropout(9));
    assert_eq!(reader.read(), ReadResult::Empty);
}

#[test]
fn test_skip_ahead_lapped_one_thread() {
    let (mut reader, mut writer) = ring_buffer::<usize>(32);

    // one read, 2*capacity+1 writes
    for i in 0..1024 {
        for _ in 0..65 {
            writer.write(i);
        }

        reader.skip_ahead();
        assert_eq!(reader.read(), ReadResult::Dropout(i));
        assert_eq!(reader.read(), ReadResult::Empty);
    }
}

#[test]
fn test_two_readers_one_thread() {
    let (mut reader1, mut writer) = ring_buffer::<usize>(32);
    let mut reader2 = reader1.clone();

    assert_eq!(reader1.read(), ReadResult::Empty);
    assert_eq!(reader2.read(), ReadResult::Empty);

    writer.write(1);

    assert_eq!(reader1.read(), ReadResult::Ok(1));
    assert_eq!(reader1.read(), ReadResult::Empty);

    assert_eq!(reader2.read(), ReadResult::Ok(1));
    assert_eq!(reader2.read(), ReadResult::Empty);

    writer.write(2);

    assert_eq!(reader1.read(), ReadResult::Ok(2));
    assert_eq!(reader1.read(), ReadResult::Empty);

    writer.write(3);

    assert_eq!(reader1.read(), ReadResult::Ok(3));
    assert_eq!(reader1.read(), ReadResult::Empty);

    writer.write(4);

    assert_eq!(reader1.read(), ReadResult::Ok(4));
    assert_eq!(reader1.read(), ReadResult::Empty);

    assert_eq!(reader2.read(), ReadResult::Ok(2));
    assert_eq!(reader2.read(), ReadResult::Ok(3));
    assert_eq!(reader2.read(), ReadResult::Ok(4));
    assert_eq!(reader2.read(), ReadResult::Empty);

    writer.write(5);
    writer.write(6);
    writer.write(7);
    writer.write(8);

    reader2.skip_ahead();
    assert_eq!(reader2.read(), ReadResult::Dropout(8));
    assert_eq!(reader2.read(), ReadResult::Empty);

    assert_eq!(reader1.read(), ReadResult::Ok(5));
    assert_eq!(reader1.read(), ReadResult::Ok(6));
    assert_eq!(reader1.read(), ReadResult::Ok(7));
    assert_eq!(reader1.read(), ReadResult::Ok(8));
    assert_eq!(reader1.read(), ReadResult::Empty);
}

#[test]
fn test_one_reader_two_threads() {
    let (mut reader, mut writer) = ring_buffer::<usize>(32);

    let reader_thread = std::thread::spawn(move || {
        for i in 0..1024 {
            loop {
                match reader.read() {
                    ReadResult::Ok(j) => {
                        assert_eq!(i, j);
                        break;
                    }
                    ReadResult::Dropout(_) => panic!(),
                    ReadResult::Empty => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        }
    });

    let writer_thread = std::thread::spawn(move || {
        for i in 0..1024 {
            writer.write(i);
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    reader_thread.join().unwrap();
    writer_thread.join().unwrap();
}

#[test]
fn test_two_readers_three_threads() {
    let (mut reader1, mut writer) = ring_buffer::<usize>(32);
    let mut reader2 = reader1.clone();

    let reader1_thread = std::thread::spawn(move || {
        for i in 0..1024 {
            loop {
                match reader1.read() {
                    ReadResult::Ok(j) => {
                        assert_eq!(i, j);
                        break;
                    }
                    ReadResult::Dropout(_) => panic!(),
                    ReadResult::Empty => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        }
    });

    let reader2_thread = std::thread::spawn(move || {
        for i in 0..1024 {
            loop {
                match reader2.read() {
                    ReadResult::Ok(j) => {
                        assert_eq!(i, j);
                        break;
                    }
                    ReadResult::Dropout(_) => panic!(),
                    ReadResult::Empty => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        }
    });

    let writer_thread = std::thread::spawn(move || {
        for i in 0..1024 {
            writer.write(i);
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    reader1_thread.join().unwrap();
    reader2_thread.join().unwrap();
    writer_thread.join().unwrap();
}

#[test]
fn test_one_reader_two_threads_high_throughput() {
    let (mut reader, mut writer) = ring_buffer::<usize>(32);

    const ITERATIONS: usize = 1024 * 1024 * 64;

    let reader_thread = std::thread::spawn(move || {
        for _ in 0..ITERATIONS {
            let Some(value) = reader.read().value() else {
                continue;
            };
            let bytes = value.to_be_bytes();

            // Expect the same bit pattern in all bytes
            assert!(bytes.iter().all(|b| *b == bytes[0]));
        }
    });

    let writer_thread = std::thread::spawn(move || {
        for i in 0..ITERATIONS {
            // Copy the same bit pattern accross all bytes
            let b = (i & 0xff) as u8;
            let value = usize::from_be_bytes([b; 8]);

            writer.write(value);
        }
    });

    reader_thread.join().unwrap();
    writer_thread.join().unwrap();
}

#[test]
fn test_two_readers_three_threads_high_throughput() {
    let (mut reader1, mut writer) = ring_buffer::<usize>(32);
    let mut reader2 = reader1.clone();

    const ITERATIONS: usize = 1024 * 1024 * 64;

    let reader1_thread = std::thread::spawn(move || {
        for _ in 0..ITERATIONS {
            let Some(value) = reader1.read().value() else {
                continue;
            };
            let bytes = value.to_be_bytes();

            // Expect the same bit pattern in all bytes
            assert!(bytes.iter().all(|b| *b == bytes[0]));
        }
    });

    let reader2_thread = std::thread::spawn(move || {
        for _ in 0..ITERATIONS {
            let Some(value) = reader2.read().value() else {
                continue;
            };
            let bytes = value.to_be_bytes();

            // Expect the same bit pattern in all bytes
            assert!(bytes.iter().all(|b| *b == bytes[0]));
        }
    });

    let writer_thread = std::thread::spawn(move || {
        for i in 0..ITERATIONS {
            // Copy the same bit pattern accross all bytes
            let b = (i & 0xff) as u8;
            let value = usize::from_be_bytes([b; 8]);

            writer.write(value);
        }
    });

    reader1_thread.join().unwrap();
    reader2_thread.join().unwrap();
    writer_thread.join().unwrap();
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Blob {
    data: [u8; 1024],
}

impl Blob {
    fn new(value: u8) -> Blob {
        Blob {
            data: [value; 1024],
        }
    }

    fn all_equal_to(&self, value: u8) -> bool {
        self.data.iter().all(|b| *b == value)
    }

    fn all_equal(&self) -> bool {
        self.all_equal_to(self.data[0])
    }
}

impl Default for Blob {
    fn default() -> Self {
        Self { data: [0; 1024] }
    }
}

#[test]
fn test_custom_data_type_one_thread() {
    let (mut reader, mut writer) = ring_buffer::<Blob>(32);

    assert_eq!(reader.read(), ReadResult::Empty);

    writer.write(Blob::new(0));

    assert_eq!(reader.read(), ReadResult::Ok(Blob::new(0)));
    assert_eq!(reader.read(), ReadResult::Empty);

    writer.write(Blob::new(1));

    assert_eq!(reader.read(), ReadResult::Ok(Blob::new(1)));
    assert_eq!(reader.read(), ReadResult::Empty);

    for _ in 0..64 {
        writer.write(Blob::new(3));
    }

    assert_eq!(reader.read(), ReadResult::Dropout(Blob::new(3)));

    reader.skip_ahead();
    assert_eq!(reader.read(), ReadResult::Dropout(Blob::new(3)));
    assert_eq!(reader.read(), ReadResult::Empty);
}

#[test]
fn test_custom_data_type_one_reader_two_threads_high_throughput() {
    let (mut reader, mut writer) = ring_buffer::<Blob>(32);

    const ITERATIONS: usize = 1024 * 1024 * 64;

    let reader_thread = std::thread::spawn(move || {
        for _ in 0..ITERATIONS {
            let Some(value) = reader.read().value() else {
                continue;
            };
            assert!(value.all_equal());
        }
    });

    let writer_thread = std::thread::spawn(move || {
        for i in 0..ITERATIONS {
            let b = (i & 0xff) as u8;
            writer.write(Blob::new(b));
        }
    });

    reader_thread.join().unwrap();
    writer_thread.join().unwrap();
}

#[test]
fn test_custom_data_type_two_readers_three_threads_high_throughput() {
    let (mut reader1, mut writer) = ring_buffer::<Blob>(32);
    let mut reader2 = reader1.clone();

    const ITERATIONS: usize = 1024 * 1024 * 64;

    let reader1_thread = std::thread::spawn(move || {
        for _ in 0..ITERATIONS {
            let Some(value) = reader1.read().value() else {
                continue;
            };
            assert!(value.all_equal());
        }
    });

    let reader2_thread = std::thread::spawn(move || {
        for _ in 0..ITERATIONS {
            let Some(value) = reader2.read().value() else {
                continue;
            };
            assert!(value.all_equal());
        }
    });

    let writer_thread = std::thread::spawn(move || {
        for i in 0..ITERATIONS {
            let b = (i & 0xff) as u8;
            writer.write(Blob::new(b));
        }
    });

    reader1_thread.join().unwrap();
    reader2_thread.join().unwrap();
    writer_thread.join().unwrap();
}
