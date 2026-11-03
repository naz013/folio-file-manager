//! Work that waits on a disk which may not answer: a network share that went away, a
//! sleeping NAS. Such a call can block for minutes and cannot be cancelled, so it runs on
//! its own thread and the caller stops waiting after a limit; the window stays usable.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

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
/// few dozen at a time. The newest request goes first: the page cannot take back a request, so
/// the ones it has since scrolled away from are left to sink to the back of the line, where
/// they run out their time limit without ever starting.
pub static THUMBNAILS: LazyLock<Lane> = LazyLock::new(|| Lane::newest_first(32));

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
    places: Arc<Places>,
}

/// Who gets a place when one comes free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// The call that has waited longest.
    Oldest,
    /// The call that asked last.
    Newest,
}

/// The places of a lane and the calls waiting for one.
struct Places {
    order: Order,
    state: Mutex<PlaceState>,
}

struct PlaceState {
    free: usize,
    next_id: u64,
    waiting: VecDeque<(u64, oneshot::Sender<Place>)>,
}

/// A place in a lane, held by a running call; dropping it hands the place to the next waiter.
struct Place {
    places: Arc<Places>,
}

impl Drop for Place {
    fn drop(&mut self) {
        self.places.release();
    }
}

/// A call waiting for a place. If it stops waiting (its time ran out), it leaves the line.
struct Waiting {
    places: Arc<Places>,
    id: u64,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let mut state = self.places.lock();
        state.waiting.retain(|(id, _)| *id != self.id);
    }
}

impl Places {
    fn lock(&self) -> std::sync::MutexGuard<'_, PlaceState> {
        // Nothing here can panic while holding the lock, but a poisoned one is still usable.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Waits for a place. Never gives up by itself: the caller's time limit drops it.
    async fn acquire(self: &Arc<Self>) -> Place {
        let (id, line) = {
            let mut state = self.lock();
            if state.free > 0 {
                state.free -= 1;
                return Place {
                    places: Arc::clone(self),
                };
            }
            let (give, line) = oneshot::channel();
            state.next_id += 1;
            let id = state.next_id;
            state.waiting.push_back((id, give));
            (id, line)
        };
        let _waiting = Waiting {
            places: Arc::clone(self),
            id,
        };
        // A waiter's sender is only ever dropped by the waiter leaving, so this is a place.
        line.await
            .expect("a waiting call's place is sent, not dropped")
    }

    /// Gives a place that came free to the next waiter, or keeps it free.
    fn release(self: &Arc<Self>) {
        let mut state = self.lock();
        loop {
            let next = match self.order {
                Order::Oldest => state.waiting.pop_front(),
                Order::Newest => state.waiting.pop_back(),
            };
            let Some((_, give)) = next else {
                state.free += 1;
                return;
            };
            let place = Place {
                places: Arc::clone(self),
            };
            match give.send(place) {
                Ok(()) => return,
                // A waiter leaves the line before its receiver closes (see `Waiting`), so this
                // should not happen. If it does, dropping the place here would take the lock
                // again: it is set aside, costing one reference count, and the next is tried.
                Err(unused) => std::mem::forget(unused),
            }
        }
    }
}

impl Lane {
    fn with_order(most: usize, order: Order) -> Self {
        Self {
            places: Arc::new(Places {
                order,
                state: Mutex::new(PlaceState {
                    free: most,
                    next_id: 0,
                    waiting: VecDeque::new(),
                }),
            }),
        }
    }

    /// A lane where the call that has waited longest goes first.
    fn new(most: usize) -> Self {
        Self::with_order(most, Order::Oldest)
    }

    /// A lane where the call that asked last goes first.
    fn newest_first(most: usize) -> Self {
        Self::with_order(most, Order::Newest)
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
        let places = Arc::clone(&self.places);
        // Set when the work begins, not when the place is taken: a call that holds a place but
        // never ran (the blocking pool was full) is busy, not timed out.
        let started = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&started);
        let turn = async move {
            let place = places.acquire().await;
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

    /// Holds the lane's only place for a while, queues three calls in a fixed order behind it,
    /// and returns the order they ran in.
    fn order_of_three(lane: &Arc<Lane>) -> Vec<&'static str> {
        let ran = Arc::new(Mutex::new(Vec::new()));
        let (started, running) = std::sync::mpsc::channel();
        let holder = {
            let lane = Arc::clone(lane);
            std::thread::spawn(move || {
                run(lane.within(Duration::from_secs(5), move || {
                    started.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(400));
                }))
            })
        };
        running.recv().unwrap();
        let mut waiters = Vec::new();
        for name in ["first", "second", "third"] {
            let lane = Arc::clone(lane);
            let ran = Arc::clone(&ran);
            waiters.push(std::thread::spawn(move || {
                run(lane.within(Duration::from_secs(5), move || {
                    ran.lock().unwrap().push(name);
                }))
            }));
            // Each one is in line before the next asks.
            std::thread::sleep(Duration::from_millis(60));
        }
        holder.join().unwrap();
        for w in waiters {
            w.join().unwrap();
        }
        // Every thread has finished, so this is the last handle.
        Arc::try_unwrap(ran).unwrap().into_inner().unwrap()
    }

    #[test]
    fn a_newest_first_lane_serves_the_last_to_ask_first() {
        assert_eq!(
            order_of_three(&Arc::new(Lane::newest_first(1))),
            ["third", "second", "first"]
        );
    }

    #[test]
    fn an_ordinary_lane_serves_the_longest_waiting_first() {
        assert_eq!(
            order_of_three(&Arc::new(Lane::new(1))),
            ["first", "second", "third"]
        );
    }

    #[test]
    fn a_call_that_stopped_waiting_never_starts_and_loses_no_place() {
        let lane = Arc::new(Lane::newest_first(1));
        let busy = Arc::clone(&lane);
        let (started, running) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            run(busy.within(Duration::from_secs(5), move || {
                started.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(300));
            }))
        });
        running.recv().unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let dropped = run(lane.within(Duration::from_millis(40), move || {
            flag.store(true, Ordering::SeqCst);
        }));
        assert_eq!(dropped, None, "gave up waiting");
        holder.join().unwrap();
        assert!(!ran.load(Ordering::SeqCst), "and never ran");
        // Its place in line did not swallow the place the holder gave back.
        assert_eq!(run(lane.within(Duration::from_secs(5), || 9)), Some(9));
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
