//! Work that waits on a disk which may not answer: a network share that went away, a
//! sleeping NAS. Such a call can block for minutes and cannot be cancelled, so it runs on
//! its own thread and the caller stops waiting after a limit; the window stays usable.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// How long a folder listing may take before it is reported as not answering.
pub const LISTING: Duration = Duration::from_secs(10);

/// How long a thumbnail may take before the plain icon is shown instead.
pub const THUMBNAIL: Duration = Duration::from_secs(5);

/// Most calls of this kind that may be running at once, finished or stuck. Stuck ones keep
/// their thread, and the pool is shared with file operations, so a grid of thumbnails from a
/// dead share must not be able to fill it. Past the cap a call is refused at once.
const MOST_AT_ONCE: usize = 64;

/// A count of running calls with a cap.
struct Slots(AtomicUsize);

impl Slots {
    const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    fn take(&self, most: usize) -> bool {
        if self.0.fetch_add(1, Ordering::AcqRel) < most {
            return true;
        }
        self.0.fetch_sub(1, Ordering::AcqRel);
        false
    }

    fn give_back(&self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

static RUNNING: Slots = Slots::new();

/// Hands its slot back when the call ends, however it ends.
struct Held(&'static Slots);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.give_back();
    }
}

/// Runs `work` off the calling thread and waits at most `limit` for it. `None` means it did
/// not finish in time, panicked, or too many calls were already running. A late call is not
/// stopped: its thread ends when the operating system finally answers, and its result is
/// dropped.
pub async fn within<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    if !RUNNING.take(MOST_AT_ONCE) {
        return None;
    }
    let held = Held(&RUNNING);
    let job = tauri::async_runtime::spawn_blocking(move || {
        let _held = held;
        work()
    });
    tokio::time::timeout(limit, job).await.ok()?.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_calls_past_the_cap_and_frees_slots() {
        let slots = Slots::new();
        assert!(slots.take(2));
        assert!(slots.take(2));
        assert!(!slots.take(2), "the third is refused");
        slots.give_back();
        assert!(slots.take(2), "a freed slot can be taken again");
    }

    #[test]
    fn returns_what_finished_in_time() {
        let got = tauri::async_runtime::block_on(within(Duration::from_secs(5), || 7));
        assert_eq!(got, Some(7));
    }

    #[test]
    fn gives_up_on_work_that_does_not_finish() {
        let started = std::time::Instant::now();
        let got = tauri::async_runtime::block_on(within(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_millis(1500));
            7
        }));
        assert_eq!(got, None);
        assert!(
            started.elapsed() < Duration::from_millis(1000),
            "did not wait for it"
        );
    }
}
