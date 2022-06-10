use std::{thread, time};

use mpmcbq::RingBuffer;

const THREADS : u32 = 2;
const CAPACITY : usize = 128;
const ELEMENTS : u32 = 100000000;

type Token = u64;

fn main() {
    let mut handles = vec![];
    let (q, s, r)  = RingBuffer::<Token>::new(CAPACITY);

    for i in 0..THREADS/2 {

        let mut sender = {
            let mut s = s.clone();

            move |i : u32| {
                println!("sender {} started", i);

                let mut succ: u32 = 0;
                let mut fail: u32 = 0;

                loop {
                    if s.send(i as Token) {
                        succ += 1;
                    } else {
                        fail += 1;
                    }

                    if succ == ELEMENTS {
                        break;
                    }
                }

                println!("sender exit i: {} succ: {} fail: {}", i, succ, fail);
            }
        };

        let mut receiver = {
            let mut r = r.clone();

            move |i : u32| {
                println!("receiver {} started", i);

                let mut succ: u32 = 0;
                let mut fail: u32 = 0;

                loop {
                    if let Ok(_) = r.recv() {
                        succ += 1;
                    } else {
                        fail += 1;

                        thread::sleep(time::Duration::from_micros(1));
                    }

                    if succ == ELEMENTS {
                        break;
                    }
                }

                println!("receiver exit i: {} succ: {} fail: {}", i, succ, fail);
            }
        };

        handles.push(thread::spawn(move || sender(i)));
        handles.push(thread::spawn(move || receiver(i)));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert!(q.empty());
}
