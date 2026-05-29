//! Dev-only test helpers for the photohelper workspace.
//!
//! **This crate must only ever appear as a `[dev-dependencies]` entry.**
//! The D5c E2E check verifies this via `cargo metadata`.
//!
//! ## Contents
//!
//! - [`HeartbeatDeathTrigger`]: Spawns a dedicated test thread that panics
//!   on demand, used to test the `Catalog` mutex-poison recovery path in an
//!   integration context. **Not** a substitute for `heartbeat_loop` — the
//!   production heartbeat thread remains panic-free.
// Test helpers intentionally use panic/expect/unwrap — these are part of
// the testing contract, not production paths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;

/// Spawns a dedicated thread that panics when [`signal`] is called.
///
/// Use this to verify that the Catalog's `PoisonError` recovery path
/// (`into_inner()` + ROLLBACK) behaves correctly when a WORKER thread
/// (not the heartbeat thread) panics while the catalog mutex is NOT held.
///
/// # Design
///
/// The dedicated thread polls `Arc<AtomicBool>` at a tight interval. When the
/// flag is set, the thread panics. The test then waits for the handle to
/// finish (or calls `join()` which returns `Err` for the panicked thread).
///
/// This is an in-process approach: the test binary's dev-dep graph includes
/// this crate; the production binary does NOT (dev-deps are not linked into
/// release artifacts).
///
/// # Example
///
/// ```no_run
/// use photohelper_test_helpers::HeartbeatDeathTrigger;
///
/// let trigger = HeartbeatDeathTrigger::spawn();
/// // ... do work ...
/// trigger.signal(); // tells the dedicated thread to panic
/// trigger.join();   // wait for it to finish (panicked)
/// ```
pub struct HeartbeatDeathTrigger {
    flag: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl HeartbeatDeathTrigger {
    /// Spawn the dedicated panic thread. The thread spins until [`signal`] is
    /// called, then panics.
    #[must_use]
    pub fn spawn() -> Self {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);
        let handle = thread::Builder::new()
            .name("ph-test-death-trigger".into())
            .spawn(move || {
                while !flag_clone.load(Ordering::Relaxed) {
                    thread::yield_now();
                }
                panic!("HeartbeatDeathTrigger: intentional panic for test");
            })
            .expect("spawn HeartbeatDeathTrigger thread");
        Self {
            flag,
            handle: Some(handle),
        }
    }

    /// Signal the thread to panic. Returns immediately; the thread panics
    /// asynchronously. Call [`join`] to wait for it to finish.
    pub fn signal(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    /// Wait for the death-trigger thread to finish (panicked). Returns the
    /// `Result<(), Box<dyn Any>>` from `JoinHandle::join` — callers may
    /// assert it is `Err` to confirm the thread panicked.
    ///
    /// # Panics
    ///
    /// Panics if called more than once (the handle is consumed on first call).
    pub fn join(mut self) -> std::thread::Result<()> {
        self.handle.take().expect("handle already joined").join()
    }

    /// Returns `true` if the thread has finished (panicked or exited).
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
    }
}

impl Drop for HeartbeatDeathTrigger {
    fn drop(&mut self) {
        // Signal and join if not already done, so the test-runner doesn't leak
        // threads even if the test panics before calling `.join()`.
        if self.handle.is_some() {
            self.flag.store(true, Ordering::Relaxed);
            let _ = self.handle.take().unwrap().join();
        }
    }
}
