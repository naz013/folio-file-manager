//! Work that waits on a disk which may not answer: a network share that went away, a
//! sleeping NAS. Such a call can block for minutes and cannot be cancelled, so it runs on
//! its own thread and the caller stops waiting after a limit; the window stays usable.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::Semaphore;

/// How long a folder listing may take before it is reported as not answering.
pub const LISTING: Duration = Duration::from_secs(10);

/// How long a smart folder may walk a folder tree before it gives up and stops.
pub const SMART: Duration = Duration::from_secs(60);

/// How long counting the size of a folder may take before it is stopped.
pub const MEASURE: Duration = Duration::from_mins(5);

/// How long counting the folder being looked at may take before it is given up as too big:
/// nobody asked for it, so it must not go on and on.
pub const MEASURE_AUTO: Duration = Duration::from_secs(30);

/// How long a thumbnail may take before the plain icon is shown instead.
pub const THUMBNAIL: Duration = Duration::from_secs(5);

/// Where folder listings run.
pub static LISTINGS: LazyLock<Lane> = LazyLock::new(|| Lane::new(16));

/// Where smart folders walk a folder tree. Walks last long, so they have a lane of their own: a
/// few of them must not hold up the listing of a folder.
pub static WALKS: LazyLock<Lane> = LazyLock::new(|| Lane::new(2));

/// Where the size of a folder is counted for the inspector. Counting is a long walk, so it has
/// a lane of its own, like the smart folders' walks.
pub static MEASURES: LazyLock<Lane> = LazyLock::new(|| Lane::new(2));

/// Where the folder being looked at is counted without being asked. Apart from the counts the
/// person asks for by selecting a folder, so those never wait behind it.
pub static AUTO_MEASURES: LazyLock<Lane> = LazyLock::new(|| Lane::new(1));

/// Where thumbnails are drawn: a big grid asks for a few hundred at once, and they are made a
/// few dozen at a time.
pub static THUMBNAILS: LazyLock<Lane> = LazyLock::new(|| Lane::new(32));

/// How a call in a [`Lane`] ended. The cause matters to the message: a call that never got a
/// place says the app is busy, one that started and went quiet says its disk is not answering.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    Done(T),
    /// No place came free within the limit: the lane is full of other, slower calls.
    Busy,
    /// It started and did not finish within the limit.
    TimedOut,
    /// It panicked.
    Failed,
}

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
    /// place included. `None` means it did not finish in time or panicked; [`Lane::run`] says
    /// which. A late call is not stopped: its thread ends when the operating system finally
    /// answers, and its result is dropped.
    pub async fn within<T: Send + 'static>(
        &self,
        limit: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        match self.run(limit, work).await {
            Outcome::Done(value) => Some(value),
            _ => None,
        }
    }

    /// Like [`Lane::within`], but says why a call that did not finish did not.
    pub async fn run<T: Send + 'static>(
        &self,
        limit: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Outcome<T> {
        let permits = Arc::clone(&self.permits);
        // Set when the work begins, not when the place is taken: a call that holds a place but
        // never ran (the blocking pool was full) is busy, not timed out.
        let started = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&started);
        let turn = async move {
            let Ok(place) = permits.acquire_owned().await else {
                return Outcome::Failed;
            };
            // The place goes with the work, so a stuck call keeps its place to the end.
            match tauri::async_runtime::spawn_blocking(move || {
                let _place = place;
                flag.store(true, Ordering::SeqCst);
                work()
            })
            .await
            {
                Ok(value) => Outcome::Done(value),
                Err(_) => Outcome::Failed,
            }
        };
        match tokio::time::timeout(limit, turn).await {
            Ok(outcome) => outcome,
            Err(_) if started.load(Ordering::SeqCst) => Outcome::TimedOut,
            Err(_) => Outcome::Busy,
        }
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
    fn says_why_a_call_did_not_finish() {
        let lane = Lane::new(1);
        // Started, then went quiet: timed out, and it keeps the only place.
        let slow = run(lane.run(Duration::from_millis(250), || {
            std::thread::sleep(Duration::from_millis(1200));
        }));
        assert_eq!(slow, Outcome::TimedOut);
        // Never got a place: busy, not timed out.
        let waiting = run(lane.run(Duration::from_millis(60), || ()));
        assert_eq!(waiting, Outcome::Busy);
    }

    #[test]
    fn a_finished_call_is_done_and_a_panic_is_failed() {
        let lane = Lane::new(2);
        assert_eq!(
            run(lane.run(Duration::from_secs(5), || 3)),
            Outcome::Done(3)
        );
        let failed: Outcome<()> = run(lane.run(Duration::from_secs(5), || panic!("boom")));
        assert_eq!(failed, Outcome::Failed);
    }

    #[test]
    fn a_stuck_call_keeps_its_place_and_others_give_up_waiting() {
        let lane = Lane::new(1);
        let stuck = run(lane.within(Duration::from_millis(30), || {
            std::thread::sleep(Duration::from_millis(600));
        }));
        assert_eq!(stuck, None);
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let got = run(lane.within(Duration::from_millis(60), move || {
            flag.store(true, Ordering::SeqCst);
        }));
        assert_eq!(got, None, "gave up waiting for a place");
        assert!(!ran.load(Ordering::SeqCst), "and never ran");
    }
}
