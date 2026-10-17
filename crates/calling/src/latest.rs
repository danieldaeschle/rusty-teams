use std::sync::{Condvar, Mutex};

struct Slot<T> {
    newest: Option<T>,
    closed: bool,
    dropped: u64,
}

/// A one-place mailbox: a new value replaces an unread one, so a slow reader always gets the newest and nothing queues up.
pub struct Latest<T> {
    slot: Mutex<Slot<T>>,
    wake: Condvar,
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Latest { slot: Mutex::new(Slot { newest: None, closed: false, dropped: 0 }), wake: Condvar::new() }
    }
}

impl<T> Latest<T> {
    pub fn put(&self, value: T) {
        let mut slot = self.slot.lock().expect("latest slot lock");
        if slot.newest.replace(value).is_some() {
            slot.dropped += 1;
        }
        drop(slot);
        self.wake.notify_one();
    }

    /// Waits for a value; `None` once the slot is closed.
    pub fn take(&self) -> Option<T> {
        let mut slot = self.slot.lock().expect("latest slot lock");
        loop {
            if slot.closed {
                return None;
            }
            if let Some(value) = slot.newest.take() {
                return Some(value);
            }
            slot = self.wake.wait(slot).expect("latest slot lock");
        }
    }

    pub fn close(&self) {
        self.slot.lock().expect("latest slot lock").closed = true;
        self.wake.notify_all();
    }

    pub fn dropped(&self) -> u64 {
        self.slot.lock().expect("latest slot lock").dropped
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_reader_gets_the_newest_value_and_older_ones_are_counted_as_dropped() {
        let latest = Latest::default();
        for value in 1..=3 {
            latest.put(value);
        }
        assert_eq!(latest.take(), Some(3));
        assert_eq!(latest.dropped(), 2);
    }

    #[test]
    fn closing_wakes_a_waiting_reader() {
        let latest = Arc::new(Latest::<u8>::default());
        let reader = {
            let latest = latest.clone();
            std::thread::spawn(move || latest.take())
        };
        std::thread::sleep(Duration::from_millis(20));
        latest.close();
        assert_eq!(reader.join().unwrap(), None);
    }
}
