//! Applies a dashboard's timer changes on a background thread.

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use super::{StateError, Store, TimerOwnership};
use crate::time::pomodoro::Saved;

/// A timer change to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerWrite {
    /// Replace the project's saved timer.
    Save { project_id: String, timer: Saved },
    /// Remove the project's saved timer.
    Clear { project_id: String },
}

impl TimerWrite {
    fn project_id(&self) -> &str {
        match self {
            Self::Save { project_id, .. } | Self::Clear { project_id } => project_id,
        }
    }
}

/// Stores timer changes on one thread, in the order they were made, so a
/// slow or locked database never blocks keys and a pause followed by a quick
/// resume is never stored the other way round.
///
/// The thread keeps a clone of the dashboard's [`TimerOwnership`], so the
/// timer lock stays held until the last queued write has finished, and it
/// refuses any write for a project whose timer that ownership does not hold.
pub struct TimerWriter {
    jobs: Option<Sender<TimerWrite>>,
    thread: Option<JoinHandle<()>>,
}

impl TimerWriter {
    /// Starts the writer. `open` opens the state database on first use, and
    /// again after a failure to open it. `report` receives each result.
    pub fn spawn<O, R>(ownership: TimerOwnership, mut open: O, mut report: R) -> Self
    where
        O: FnMut() -> Result<Store, StateError> + Send + 'static,
        R: FnMut(Result<(), StateError>) + Send + 'static,
    {
        let (jobs, queue) = mpsc::channel::<TimerWrite>();
        let thread = thread::spawn(move || {
            let mut store = None;
            for job in queue {
                report(write(&ownership, &mut store, &mut open, job));
            }
            // `ownership` is dropped here, after the last write.
        });
        Self {
            jobs: Some(jobs),
            thread: Some(thread),
        }
    }

    /// Queues `write`.
    pub fn send(&self, write: TimerWrite) {
        if let Some(jobs) = &self.jobs {
            // The thread only stops after a panic, already reported on stderr.
            let _ = jobs.send(write);
        }
    }
}

impl Drop for TimerWriter {
    /// Waits for queued writes, so quitting right after pausing keeps the
    /// pause, and the timer lock is not released before they finish.
    fn drop(&mut self) {
        drop(self.jobs.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn write<O>(
    ownership: &TimerOwnership,
    store: &mut Option<Store>,
    open: &mut O,
    job: TimerWrite,
) -> Result<(), StateError>
where
    O: FnMut() -> Result<Store, StateError>,
{
    if !ownership.owns(job.project_id()) {
        return Err(StateError::TimerNotOwned);
    }
    let store = match store {
        Some(store) => store,
        None => store.insert(open()?),
    };
    match job {
        TimerWrite::Save { project_id, timer } => store.save_timer(&project_id, &timer),
        TimerWrite::Clear { project_id } => store.clear_timer(&project_id),
    }
}
