//! Progress, pause and cancel for the operation running on this thread.
//!
//! The queue runs each operation on one worker thread inside [`scope`]; the file copying
//! below it reports bytes and checks for pause and cancel through [`current`], so none of
//! the operation functions need to carry the handle in their signatures.

use std::cell::RefCell;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// How often a paused operation looks again.
const PAUSE_POLL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct Progress {
    bytes: AtomicU64,
    total_bytes: AtomicU64,
    items: AtomicU64,
    total_items: AtomicU64,
    paused: AtomicBool,
    cancelled: AtomicBool,
}

impl Progress {
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Relaxed)
    }

    pub fn items(&self) -> u64 {
        self.items.load(Ordering::Relaxed)
    }

    pub fn total_items(&self) -> u64 {
        self.total_items.load(Ordering::Relaxed)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn set_totals(&self, bytes: u64, items: u64) {
        self.total_bytes.store(bytes, Ordering::Relaxed);
        self.total_items.store(items, Ordering::Relaxed);
    }

    pub fn add_bytes(&self, n: u64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_item(&self) {
        self.items.fetch_add(1, Ordering::Relaxed);
    }

    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Waits while paused; fails once cancelled. Called between files and chunks.
    pub fn checkpoint(&self) -> io::Result<()> {
        while self.is_paused() && !self.is_cancelled() {
            std::thread::sleep(PAUSE_POLL);
        }
        if self.is_cancelled() {
            Err(cancelled())
        } else {
            Ok(())
        }
    }
}

/// What an operation stops with when it is cancelled. Not `ErrorKind::Interrupted`, which
/// the standard library's read and write helpers retry.
#[derive(Debug)]
struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// The error an operation stops with when it is cancelled.
pub fn cancelled() -> io::Error {
    io::Error::other(Cancelled)
}

/// Whether `e` is the error of a cancelled operation.
#[cfg(test)]
pub fn is_cancelled_error(e: &io::Error) -> bool {
    matches!(e.get_ref(), Some(inner) if inner.is::<Cancelled>())
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Progress>>> = const { RefCell::new(None) };
}

/// Runs `f` with `progress` as this thread's current operation.
pub fn scope<R>(progress: &Arc<Progress>, f: impl FnOnce() -> R) -> R {
    let before = CURRENT.with(|c| c.replace(Some(Arc::clone(progress))));
    let result = f();
    CURRENT.with(|c| *c.borrow_mut() = before);
    result
}

/// The operation running on this thread, if it runs in the queue.
pub fn current() -> Option<Arc<Progress>> {
    CURRENT.with(|c| c.borrow().clone())
}

/// [`Progress::checkpoint`] for the current operation; always fine outside the queue.
pub fn checkpoint() -> io::Result<()> {
    current().map_or(Ok(()), |p| p.checkpoint())
}

/// Counts `n` bytes done for the current operation.
pub fn add_bytes(n: u64) {
    if let Some(p) = current() {
        p.add_bytes(n);
    }
}

/// Counts one more item done for the current operation.
pub fn add_item() {
    if let Some(p) = current() {
        p.add_item();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_the_queue_nothing_is_tracked_or_stopped() {
        assert!(checkpoint().is_ok());
        add_bytes(10);
        add_item();
        assert!(current().is_none());
    }

    #[test]
    fn scope_makes_the_progress_current_and_restores_after() {
        let p = Arc::new(Progress::default());
        scope(&p, || {
            add_bytes(5);
            add_item();
            assert!(current().is_some());
        });
        assert_eq!((p.bytes(), p.items()), (5, 1));
        assert!(current().is_none());
    }

    #[test]
    fn cancel_stops_at_the_next_checkpoint_even_while_paused() {
        let p = Arc::new(Progress::default());
        p.pause(true);
        let waiting = Arc::clone(&p);
        let handle = std::thread::spawn(move || waiting.checkpoint());
        std::thread::sleep(Duration::from_millis(150));
        p.cancel();
        let result = handle.join().unwrap();
        assert!(is_cancelled_error(&result.unwrap_err()));
    }

    #[test]
    fn resume_lets_a_paused_checkpoint_go_on() {
        let p = Arc::new(Progress::default());
        p.pause(true);
        let waiting = Arc::clone(&p);
        let handle = std::thread::spawn(move || waiting.checkpoint());
        std::thread::sleep(Duration::from_millis(150));
        p.pause(false);
        assert!(handle.join().unwrap().is_ok());
    }
}
