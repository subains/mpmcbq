use std::cell::UnsafeCell;
use std::sync::{Arc, Mutex};
use crossbeam_utils::CachePadded;
use std::sync::atomic::{AtomicU32, Ordering};

struct Cell<T: Default + Copy> {
    pos: AtomicU32,
    data: UnsafeCell<T>,
}

struct Users {
    senders: Arc<Mutex<u32>>,
    receivers: Arc<Mutex<u32>>,
}

pub struct RingBuffer<T: Default + Copy> {
    n: CachePadded<usize>,
    v: CachePadded<Vec<Cell<T>>>,
    enq_pos: CachePadded<AtomicU32>,
    deq_pos: CachePadded<AtomicU32>,
    users: CachePadded<Users>,
}

pub struct Sender<T: Default + Copy> {
    rb: UnsafeCell<*mut RingBuffer<T>>,
}

pub struct Receiver<T: Default + Copy> {
    rb: UnsafeCell<*mut RingBuffer<T>>,
}

impl<T: Default + Copy> Drop for Cell<T> {
    fn drop(&mut self) {
        // println!("Cell drop({:?})", self.pos.load(Ordering::SeqCst));
    }
}

impl<T: Default + Copy> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        let n_s;

        {
            let n = self.users.senders.lock().unwrap();

            assert!(*n == 0, "Dropping ring buffer with active senders");

            n_s = *n;
        }

        let n_r;
        {
            let n = self.users.receivers.lock().unwrap();

            assert!(*n == 0, "Dropping ring buffer with active receivers");

            n_r = *n;
        }

        println!(
            "RingBuffer drop : senders: {}, receivers: {} {:?}",
            n_s, n_r, self.n
        );
    }
}

impl<T: Default + Copy> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut n = unsafe { (*(*self.rb.get())).users.senders.lock().unwrap() };

        assert!(*n > 0, "Number of senders can't be zero.");

        *n -= 1;

        println!("Sender::drop active: {}", *n);
    }
}

impl<T: Default + Copy> Drop for Receiver<T> {
    fn drop(&mut self) {
        let mut n = unsafe { (*(*self.rb.get())).users.receivers.lock().unwrap() };

        assert!(*n > 0, "Number of receivers can't be zero");

        *n -= 1;

        println!("Receiver::drop active: {}", *n);
    }
}

unsafe impl<T: Default + Copy> Send for Sender<T> where T: Send {}
unsafe impl<T: Default + Copy> Sync for Sender<T> where T: Sync {}

unsafe impl<T: Default + Copy> Send for Receiver<T> where T: Send {}
unsafe impl<T: Default + Copy> Sync for Receiver<T> where T: Sync {}

impl Users {
    pub fn new(s: u32, r: u32) -> Self {
        Self {
            senders: Arc::new(Mutex::new(s)),
            receivers: Arc::new(Mutex::new(r)),
        }
    }
}

impl<T: Default + Copy> Sender<T> {
    pub fn send(&mut self, d: T) -> bool {
        unsafe { (*(*self.rb.get())).send(d) }
    }

    pub fn empty(&mut self) -> bool {
        unsafe { (*(*self.rb.get())).empty() }
    }

    pub fn capacity(&mut self) -> usize {
        unsafe { (*(*self.rb.get())).capacity() }
    }
}

impl<T: Default + Copy> Clone for Sender<T> {
    fn clone(&self) -> Self {
        let mut n = unsafe { (*(*self.rb.get())).users.senders.lock().unwrap() };

        assert!(*n > 0, "Number of senders can't be zero");

        *n += 1;

        println!("Sender::clone active: {}", *n);

        unsafe {
            Sender {
                rb: UnsafeCell::new(*self.rb.get()),
            }
        }
    }
}

impl<T: Default + Copy> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        let mut n = unsafe { (*(*self.rb.get())).users.receivers.lock().unwrap() };

        assert!(*n > 0, "Number of receivers can't be zero");

        *n += 1;

        println!("Receiver::clone active: {}", *n);

        unsafe {
            Receiver {
                rb: UnsafeCell::new(*self.rb.get()),
            }
        }
    }
}

impl<T: Default + Copy> Receiver<T> {
    pub fn recv(&mut self) -> Result<T, bool> {
        unsafe { (*(*self.rb.get())).recv() }
    }

    pub fn empty(&mut self) -> bool {
        unsafe { (*(*self.rb.get())).empty() }
    }

    pub fn capacity(&mut self) -> usize {
        unsafe { (*(*self.rb.get())).capacity() }
    }
}

impl<T: Default + Copy> Cell<T> {
    pub fn new(i: u32) -> Cell<T> {
        // println!("Cell::new({})", i);

        Self {
            pos: AtomicU32::new(i),
            data: Default::default(),
        }
    }
}

impl<T: Default + Copy> RingBuffer<T> {
    fn send(&mut self, d: T) -> bool {
        let mut pos = self.enq_pos.load(Ordering::Relaxed);

        loop {
            let cell = &mut self.v[pos as usize & *self.n];
            let seq = cell.pos.load(Ordering::Acquire);
            let diff = seq as i32 - pos as i32;

            if diff == 0 {
                let new = pos + 1;

                match self.enq_pos.compare_exchange_weak(
                    pos,
                    new,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        cell.data = UnsafeCell::new(d);
                        cell.pos.store(new, Ordering::Release);
                        return true;
                    }
                    Err(_) => (),
                }
            } else if diff < 0 {
                return false;
            } else {
                pos = self.enq_pos.load(Ordering::Relaxed);
            }
        }
    }

    fn recv(&mut self) -> Result<T, bool> {
        let mut pos = self.deq_pos.load(Ordering::Relaxed);

        loop {
            let cell = &mut self.v[pos as usize & *self.n];
            let seq = cell.pos.load(Ordering::Acquire);
            let diff = seq as i32 - (pos + 1) as i32;

            if diff == 0 {
                let new = pos + 1;

                match self.deq_pos.compare_exchange_weak(
                    pos,
                    new,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        let d = *cell.data.get_mut();
                        cell.pos.store(pos + *self.n as u32 + 1, Ordering::Release);
                        return Ok(d);
                    }
                    Err(_) => (),
                }
            } else if diff < 0 {
                // Ring buffer is empty.
                return Err(false);
            } else {
                pos = self.deq_pos.load(Ordering::Relaxed);
            }
        }
    }

    pub fn empty(&self) -> bool {
        let mut pos = self.deq_pos.load(Ordering::Relaxed);

        loop {
            let cell = &self.v[pos as usize & *self.n];
            let seq = cell.pos.load(Ordering::Acquire);
            let diff = seq as i32 - (pos + 1) as i32;

            if diff == 0 {
                return false;
            } else if diff < 0 {
                // Ring buffer is empty.
                return true;
            } else {
                pos = self.deq_pos.load(Ordering::Relaxed);
            }
        }
    }

    pub fn capacity(&self) -> usize {
        *self.n
    }

    pub fn new(n: usize) -> (Box<RingBuffer<T>>, Sender<T>, Receiver<T>) {
        assert!(n > 0, "size must be > 0");

        let n = (n + 1).next_power_of_two();
        let mut v: Vec<Cell<T>> = Vec::new();

        for i in 0..n {
            v.push(Cell::<T>::new(i as u32));
        }

        let mut rb = Box::new(Self {
            n: CachePadded::new(n - 1),
            v: CachePadded::new(v),
            enq_pos: CachePadded::new(AtomicU32::new(0)),
            deq_pos: CachePadded::new(AtomicU32::new(0)),
            users: CachePadded::new(Users::new(1, 1)),
        });

        let rb_ptr = &mut *rb as *mut RingBuffer<T>;

        (
            rb,
            Sender {
                rb: UnsafeCell::new(rb_ptr),
            },
            Receiver {
                rb: UnsafeCell::new(rb_ptr),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
