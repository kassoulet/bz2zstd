//! Backpressure for out-of-order block reassembly.
//!
//! Workers decompress blocks in parallel, but the consumer needs them in order. Without a
//! limit, one slow block lets every later block pile up in the consumer's reorder buffer.
//! A `Throttle` makes a worker wait before delivering block `idx` until the consumer is
//! within `window` blocks of it. Block `next` itself never waits, so this cannot deadlock.

use std::sync::{Condvar, Mutex};

/// Limits how far workers may run ahead of the in-order consumer.
#[derive(Debug)]
pub struct Throttle {
    next: Mutex<usize>,
    cv: Condvar,
    window: usize,
}

impl Throttle {
    /// Creates a throttle allowing at most `window` blocks ahead of the consumer.
    pub fn new(window: usize) -> Self {
        Self {
            next: Mutex::new(0),
            cv: Condvar::new(),
            window,
        }
    }

    /// Blocks until block `idx` is within the window of the consumer's next block.
    pub fn wait(&self, idx: usize) {
        let mut next = self.next.lock().unwrap();
        while idx >= next.saturating_add(self.window) {
            next = self.cv.wait(next).unwrap();
        }
    }

    /// Consumer: `next` is the index of the next block it needs.
    pub fn set_next(&self, next: usize) {
        *self.next.lock().unwrap() = next;
        self.cv.notify_all();
    }

    /// Releases all current and future waiters (consumer finished, failed or dropped).
    pub fn release(&self) {
        self.set_next(usize::MAX);
    }
}
