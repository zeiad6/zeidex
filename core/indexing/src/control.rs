//! Pause, resume and cancel for a long index walk.
//!
//! The walk and its database writer run on two different threads, and the user
//! can ask to stop between two entries. This is the one shared switch they both
//! read, so a cancel cannot be seen by the writer while the walker keeps going
//! (or the other way round).
//!
//! The hot path is lock-free: a producer checks [`IndexControl::checkpoint`] once
//! per directory entry, and that is an atomic load in the normal case. The mutex
//! and condition variable are only touched while paused, which is what makes a
//! paused walk cost nothing instead of spinning.
//!
//! Cancelling is deliberately *not* the same as "stop and forget". The caller
//! that owns the database must skip its stale-row sweep: rows that were never
//! visited are not rows that are gone, and pruning them would delete a large
//! part of a working index. That decision lives in the engine; this type only
//! reports what was asked.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// What the user asked the current walk to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexRun {
    Running,
    Paused,
    Cancelled,
}

impl IndexRun {
    fn code(self) -> u8 {
        match self {
            IndexRun::Running => 0,
            IndexRun::Paused => 1,
            IndexRun::Cancelled => 2,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => IndexRun::Paused,
            2 => IndexRun::Cancelled,
            _ => IndexRun::Running,
        }
    }
}

/// The shared switch. Cheap to clone (an `Arc`), and every clone sees the same
/// state.
#[derive(Debug)]
pub struct IndexControl {
    state: AtomicU8,
    gate: Mutex<()>,
    parked: Condvar,
}

impl Default for IndexControl {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexControl {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(IndexRun::Running.code()),
            gate: Mutex::new(()),
            parked: Condvar::new(),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn state(&self) -> IndexRun {
        IndexRun::from_code(self.state.load(Ordering::Acquire))
    }

    /// Ask a running walk to park after its current entry. Returns whether the
    /// state changed, so the caller can report "already paused" honestly instead
    /// of claiming a fresh pause.
    pub fn pause(&self) -> bool {
        self.state
            .compare_exchange(
                IndexRun::Running.code(),
                IndexRun::Paused.code(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// Let a parked walk continue. A cancelled walk stays cancelled: resuming it
    /// would surprise a user who already asked to stop.
    pub fn resume(&self) -> bool {
        if self
            .state
            .compare_exchange(
                IndexRun::Paused.code(),
                IndexRun::Running.code(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.parked.notify_all();
            true
        } else {
            false
        }
    }

    /// Stop the walk. Wakes a parked walker so the cancel is seen immediately
    /// rather than after the next resume.
    pub fn cancel(&self) -> bool {
        let was = self
            .state
            .swap(IndexRun::Cancelled.code(), Ordering::AcqRel);
        self.parked.notify_all();
        was != IndexRun::Cancelled.code()
    }

    /// Arm a fresh walk. A control reused for the next run must not start
    /// cancelled because the previous run was stopped.
    pub fn reset(&self) {
        self.state
            .store(IndexRun::Running.code(), Ordering::Release);
        self.parked.notify_all();
    }

    /// The producer's checkpoint: `true` to continue with the next entry,
    /// `false` when the walk must stop. Blocks while paused, so a paused walk
    /// uses no CPU.
    pub fn checkpoint(&self) -> bool {
        match self.state.load(Ordering::Acquire) {
            code if code == IndexRun::Running.code() => true,
            code if code == IndexRun::Cancelled.code() => false,
            _ => {
                // Parked. The lock is only taken in this branch, so the normal
                // path never contends with a resume.
                let mut guard = self
                    .gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while self.state.load(Ordering::Acquire) == IndexRun::Paused.code() {
                    guard = self
                        .parked
                        .wait(guard)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                self.state.load(Ordering::Acquire) != IndexRun::Cancelled.code()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn a_fresh_control_lets_work_through() {
        let control = IndexControl::new();
        assert_eq!(control.state(), IndexRun::Running);
        assert!(control.checkpoint());
    }

    #[test]
    fn pause_parks_a_worker_until_resume() {
        let control = IndexControl::shared();
        assert!(control.pause());
        assert_eq!(control.state(), IndexRun::Paused);
        assert!(!control.pause(), "a second pause changes nothing");

        let (tx, rx) = mpsc::channel();
        let worker = control.clone();
        let handle = thread::spawn(move || {
            // Blocks here until the test resumes.
            let go = worker.checkpoint();
            let _ = tx.send(go);
        });

        // Nothing arrives while parked: the worker is not spinning, it is
        // waiting on the condition variable.
        assert!(rx.recv_timeout(Duration::from_millis(120)).is_err());
        assert!(control.resume());
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(true));
        handle.join().expect("worker thread");
    }

    #[test]
    fn cancel_wakes_a_parked_worker_and_stays_cancelled() {
        let control = IndexControl::shared();
        control.pause();

        let worker = control.clone();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = tx.send(worker.checkpoint());
        });

        thread::sleep(Duration::from_millis(50));
        assert!(control.cancel());
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(false));
        assert_eq!(control.state(), IndexRun::Cancelled);
        assert!(!control.resume(), "a cancelled walk does not resume");
        assert!(!control.checkpoint(), "and every later checkpoint stops");
        handle.join().expect("worker thread");
    }

    #[test]
    fn cancel_is_idempotent_and_reset_arms_the_next_run() {
        let control = IndexControl::new();
        assert!(control.cancel());
        assert!(!control.cancel(), "the second cancel is not a change");
        control.reset();
        assert_eq!(control.state(), IndexRun::Running);
        assert!(control.checkpoint());
    }

    #[test]
    fn resume_without_a_pause_reports_no_change() {
        let control = IndexControl::new();
        assert!(!control.resume());
    }
}
