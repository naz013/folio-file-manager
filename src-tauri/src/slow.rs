//! Work that waits on a disk which may not answer: a network share that went away, a
//! sleeping NAS. Such a call can block for minutes and cannot be cancelled, so it runs on
//! its own thread and the caller stops waiting after a limit; the window stays usable.

use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::Semaphore;

/// How long a folder listing may take before it is reported as not answering.
pub const LISTING: Duration = Duration::from_secs(10);

/// How long a thumbnail may take before the plain icon is shown instead.
pub const THUMBNAIL: Duration = Duration::from_secs(5);

/// Where folder listings run.
pub static LISTINGS: LazyLock<Lane> = LazyLock::new(|| Lane::new(16));

/// Where thumbnails are drawn: a big grid asks for a few hundred at once, and they are made a
/// few dozen at a time.
pub static THUMBNAILS: LazyLock<Lane> = LazyLock::new(|| Lane::new(32));

/// A kind of call with a cap on how many run at once, stuck ones included. A stuck call
/// keeps its thread, and the pool is shared with file operations, so thumbnails from a dead
/// share must not be able to fill it. Calls past the cap wait their turn, inside their time
/// limit. Each kind has its own lane: a dead share's stuck thumbnails don't hold up the
/// listing of a folder on a healthy disk.
pub struct Lane {
    permits: Arc<Semaphore>,
}

impl Lane {
    fn new(most: usize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(most)),
        }
    }

    /// Runs `work` off the calling thread and waits at most `limit` for it, a wait for a free
    /// place included. `None` means it did not finish in time or panicked. A late call is not
    /// stopped: its thread ends when the operating system finally answers, and its result is
    /// dropped.
    pub async fn within<T: Send + 'static>(
        &self,
        limit: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let permits = Arc::clone(&self.permits);
        let turn = async move {
            let place = permits.acquire_owned().await.ok()?;
            // The place goes with the work, so a stuck call keeps its place to the end.
            tauri::async_runtime::spawn_blocking(move || {
                let _place = place;
                work()
            })
            .await
            .ok()
        };
        tokio::time::timeout(limit, turn).await.ok()?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<T>(f: impl Future<Output = T>) -> T {
        tauri::async_runtime::block_on(f)
    }

    #[test]
    fn returns_what_finished_in_time() {
        let lane = Lane::new(2);
        assert_eq!(run(lane.within(Duration::from_secs(5), || 7)), Some(7));
    }

    #[test]
    fn gives_up_on_work_that_does_not_finish() {
        let lane = Lane::new(2);
        let started = std::time::Instant::now();
        let got = run(lane.within(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_millis(1500));
            7
        }));
        assert_eq!(got, None);
        assert!(
            started.elapsed() < Duration::from_millis(1000),
            "did not wait for it"
        );
    }

    #[test]
    fn calls_past_the_cap_wait_for_a_free_place() {
        let lane = Arc::new(Lane::new(1));
        let busy = Arc::clone(&lane);
        let (started, running) = std::sync::mpsc::channel();
        let first = std::thread::spawn(move || {
            run(busy.within(Duration::from_secs(5), move || {
                // The place is held from here on, so the second call must wait for it.
                started.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(300));
                1
            }))
        });
        running.recv().unwrap();
        let waited = std::time::Instant::now();
        // Waits its turn and still gets its answer.
        assert_eq!(run(lane.within(Duration::from_secs(5), || 2)), Some(2));
        assert!(
            waited.elapsed() >= Duration::from_millis(100),
            "it waited for the first call's place"
        );
        assert_eq!(first.join().unwrap(), Some(1));
    }

    #[test]
    fn a_stuck_call_keeps_its_place_and_others_give_up_waiting() {
        let lane = Lane::new(1);
        let stuck = run(lane.within(Duration::from_millis(30), || {
            std::thread::sleep(Duration::from_millis(600));
        }));
        assert_eq!(stuck, None);
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let got = run(lane.within(Duration::from_millis(60), move || {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        assert_eq!(got, None, "gave up waiting for a place");
        assert!(
            !ran.load(std::sync::atomic::Ordering::SeqCst),
            "and never ran"
        );
    }
}
