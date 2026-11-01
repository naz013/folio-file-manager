//! The operations queue: copies, moves and deletes run here in the background, one at a
//! time per drive, with progress, pause and cancel.
//!
//! Each job runs on its drive's worker thread inside [`ops::progress::scope`], so the file
//! copying reports bytes and notices pause and cancel without the operation functions
//! knowing about the queue. Every change to a job is sent to the window that started it
//! as a `queue-update` event.

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use crate::ops::progress::Progress;

pub const UPDATE: &str = "queue-update";

/// Finished jobs kept for the UI to show; older ones are forgotten.
const KEEP_FINISHED: usize = 30;

/// How often progress, speed and time left are sent while jobs run.
const TICK: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Copy,
    Move,
    Trash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl State {
    fn is_finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

/// What the UI is told about a job.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    id: u64,
    kind: Kind,
    label: String,
    state: State,
    bytes_done: u64,
    bytes_total: u64,
    items_done: u64,
    items_total: u64,
    /// Bytes per second, smoothed.
    speed: f64,
    eta_secs: Option<u64>,
    error: Option<String>,
    /// What the operation returned (`Copied` / `Moved` for the UI) once it is done.
    result: Option<Value>,
}

/// What a job does. It reports through the progress it is given and returns what the UI
/// should be told, or why it failed.
pub type Work = Box<dyn FnOnce(&Progress) -> Result<Value, String> + Send>;

struct Job {
    id: u64,
    kind: Kind,
    label: String,
    /// Window that started it, which is the only one told about it.
    window: String,
    progress: Arc<Progress>,
    work: Mutex<Option<Work>>,
    live: Mutex<Live>,
}

struct Live {
    state: State,
    speed: f64,
    last_bytes: u64,
    last_at: Instant,
    error: Option<String>,
    result: Option<Value>,
}

/// Jobs for one drive, run in order by one worker thread.
#[derive(Default)]
struct Lane {
    waiting: Mutex<LaneState>,
}

#[derive(Default)]
struct LaneState {
    queue: VecDeque<Arc<Job>>,
    worker_running: bool,
}

pub struct Queue {
    jobs: Mutex<Vec<Arc<Job>>>,
    lanes: Mutex<HashMap<String, Arc<Lane>>>,
    next_id: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Queue {
    /// Creates the queue and starts the thread that reports running jobs' progress.
    pub fn start(app: &AppHandle) -> Arc<Self> {
        let queue = Arc::new(Self {
            jobs: Mutex::default(),
            lanes: Mutex::default(),
            next_id: AtomicU64::new(1),
        });
        let (weak, app) = (Arc::downgrade(&queue), app.clone());
        std::thread::spawn(move || {
            while let Some(queue) = weak.upgrade() {
                std::thread::sleep(TICK);
                queue.tick(&app);
            }
        });
        queue
    }

    /// Adds a job to the lane of `lane` (a drive), to run after the ones before it.
    /// Returns its id.
    pub fn submit(
        self: &Arc<Self>,
        app: &AppHandle,
        window: &str,
        kind: Kind,
        label: String,
        lane: &str,
        work: Work,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let job = Arc::new(Job {
            id,
            kind,
            label,
            window: window.to_owned(),
            progress: Arc::new(Progress::default()),
            work: Mutex::new(Some(work)),
            live: Mutex::new(Live {
                state: State::Queued,
                speed: 0.0,
                last_bytes: 0,
                last_at: Instant::now(),
                error: None,
                result: None,
            }),
        });
        {
            let mut jobs = lock(&self.jobs);
            jobs.push(Arc::clone(&job));
            // Old finished jobs go, never ones still waiting or running.
            let finished = jobs
                .iter()
                .filter(|j| lock(&j.live).state.is_finished())
                .count();
            let mut drop_first = finished.saturating_sub(KEEP_FINISHED);
            jobs.retain(|j| {
                if drop_first > 0 && lock(&j.live).state.is_finished() {
                    drop_first -= 1;
                    false
                } else {
                    true
                }
            });
        }
        emit(app, &job);
        let lane = Arc::clone(lock(&self.lanes).entry(lane.to_owned()).or_default());
        let spawn = {
            let mut state = lock(&lane.waiting);
            state.queue.push_back(job);
            !std::mem::replace(&mut state.worker_running, true)
        };
        if spawn {
            let app = app.clone();
            std::thread::spawn(move || work_through(&lane, &app));
        }
        id
    }

    fn find(&self, id: u64) -> Option<Arc<Job>> {
        lock(&self.jobs).iter().find(|j| j.id == id).cloned()
    }

    /// Pauses a running job; it waits at its next file or chunk.
    pub fn pause(&self, app: &AppHandle, id: u64) {
        if let Some(job) = self.find(id) {
            let mut live = lock(&job.live);
            if live.state == State::Running {
                job.progress.pause(true);
                live.state = State::Paused;
                drop(live);
                emit(app, &job);
            }
        }
    }

    pub fn resume(&self, app: &AppHandle, id: u64) {
        if let Some(job) = self.find(id) {
            let mut live = lock(&job.live);
            if live.state == State::Paused {
                job.progress.pause(false);
                live.state = State::Running;
                live.last_at = Instant::now();
                live.last_bytes = job.progress.bytes();
                drop(live);
                emit(app, &job);
            }
        }
    }

    /// Whether `window` has jobs that are waiting, running or paused.
    pub fn has_active(&self, window: &str) -> bool {
        lock(&self.jobs)
            .iter()
            .any(|j| j.window == window && !lock(&j.live).state.is_finished())
    }

    /// Whether a copy or move is waiting, running or paused, in any window. The folders
    /// such a job is filling are named like unfinished copies, so nothing may be called a
    /// leftover while one is.
    pub fn copy_running(&self) -> bool {
        lock(&self.jobs).iter().any(|j| {
            matches!(j.kind, Kind::Copy | Kind::Move) && !lock(&j.live).state.is_finished()
        })
    }

    /// Whether any job at all is waiting, running or paused.
    pub fn is_busy(&self) -> bool {
        lock(&self.jobs)
            .iter()
            .any(|j| !lock(&j.live).state.is_finished())
    }

    /// Cancels a job. One still waiting never starts; a running one stops at its next
    /// file or chunk. The item it was copying is removed; items it had finished stay.
    pub fn cancel(&self, app: &AppHandle, id: u64) {
        let Some(job) = self.find(id) else { return };
        let mut live = lock(&job.live);
        match live.state {
            State::Queued => {
                job.progress.cancel();
                live.state = State::Cancelled;
                live.error = Some("Cancelled".into());
                drop(live);
                emit(app, &job);
            }
            State::Running | State::Paused => job.progress.cancel(),
            _ => {}
        }
    }

    /// Forgets a finished job.
    pub fn dismiss(&self, id: u64) {
        lock(&self.jobs).retain(|j| j.id != id || !lock(&j.live).state.is_finished());
    }

    /// Every job, oldest first.
    pub fn list(&self) -> Vec<JobView> {
        lock(&self.jobs).iter().map(|j| view(j)).collect()
    }

    /// Updates speed and time left of running jobs and tells their windows.
    fn tick(&self, app: &AppHandle) {
        let running: Vec<Arc<Job>> = lock(&self.jobs)
            .iter()
            .filter(|j| lock(&j.live).state == State::Running)
            .cloned()
            .collect();
        for job in running {
            {
                let mut live = lock(&job.live);
                let now = Instant::now();
                let secs = now.duration_since(live.last_at).as_secs_f64();
                let bytes = job.progress.bytes();
                if secs > 0.0 {
                    #[expect(clippy::cast_precision_loss, reason = "a speed estimate")]
                    let instant = bytes.saturating_sub(live.last_bytes) as f64 / secs;
                    // Smoothed, so the numbers shown don't jump about.
                    live.speed = if live.speed > 0.0 {
                        live.speed * 0.7 + instant * 0.3
                    } else {
                        instant
                    };
                }
                live.last_at = now;
                live.last_bytes = bytes;
            }
            emit(app, &job);
        }
    }
}

/// Runs the jobs of a lane one after another until none are left.
fn work_through(lane: &Lane, app: &AppHandle) {
    loop {
        let job = {
            let mut state = lock(&lane.waiting);
            let Some(job) = state.queue.pop_front() else {
                state.worker_running = false;
                return;
            };
            job
        };
        run(&job, app);
    }
}

fn run(job: &Arc<Job>, app: &AppHandle) {
    {
        let mut live = lock(&job.live);
        // Cancelled while it waited.
        if live.state != State::Queued {
            return;
        }
        live.state = State::Running;
        live.last_at = Instant::now();
    }
    emit(app, job);
    let Some(work) = lock(&job.work).take() else {
        return;
    };
    // A panic in the work fails this job, not the drive's whole lane.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::ops::progress::scope(&job.progress, || work(&job.progress))
    }))
    .unwrap_or_else(|_| Err("Folio ran into an unexpected error".to_owned()));
    {
        let mut live = lock(&job.live);
        match outcome {
            Ok(value) => {
                live.state = State::Done;
                live.result = Some(value);
            }
            // Only the cancel itself is a cancel: a real error at the same moment is not.
            Err(message) if job.progress.is_cancelled() && message.starts_with("Cancelled") => {
                live.state = State::Cancelled;
                live.error = Some(message);
            }
            Err(message) => {
                live.state = State::Failed;
                live.error = Some(message);
            }
        }
        live.speed = 0.0;
    }
    emit(app, job);
}

fn view(job: &Job) -> JobView {
    let live = lock(&job.live);
    let (done, total) = (job.progress.bytes(), job.progress.total_bytes());
    #[expect(
        clippy::cast_possible_truncation,
        reason = "whole seconds of an estimate"
    )]
    #[expect(clippy::cast_sign_loss, reason = "the speed is positive here")]
    #[expect(clippy::cast_precision_loss, reason = "an estimate")]
    let eta_secs = (live.state == State::Running && live.speed > 1.0 && total > done)
        .then(|| ((total - done) as f64 / live.speed) as u64);
    JobView {
        id: job.id,
        kind: job.kind,
        label: job.label.clone(),
        state: live.state,
        bytes_done: done,
        bytes_total: total,
        items_done: job.progress.items(),
        items_total: job.progress.total_items(),
        speed: live.speed,
        eta_secs,
        error: live.error.clone(),
        result: live.result.clone(),
    }
}

fn emit(app: &AppHandle, job: &Job) {
    let _ = app.emit_to(job.window.as_str(), UPDATE, view(job));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_done_failed_and_cancelled_are_finished() {
        assert!(State::Done.is_finished());
        assert!(State::Failed.is_finished());
        assert!(State::Cancelled.is_finished());
        assert!(!State::Queued.is_finished());
        assert!(!State::Running.is_finished());
        assert!(!State::Paused.is_finished());
    }
}
