//! Shared heartbeat infrastructure for long-running subcommands.
//!
//! Extracted at the third-consumer trigger (ingest + cull + dedup) per TD-016
//! (CLAUDE.md "Three similar lines is better than a premature abstraction").
//!
//! # Usage
//!
//! ```ignore
//! let stop = Arc::new(HeartbeatStop::new());
//! let handle = {
//!     let stop = Arc::clone(&stop);
//!     thread::Builder::new()
//!         .name("ph-heartbeat".into())
//!         .spawn(move || run_heartbeat_loop(&stop, heartbeat_interval(), || {
//!             eprintln!("[heartbeat] walked {}", stats.walked.load(Ordering::Relaxed));
//!         }))
//!         .expect("spawning heartbeat thread")
//! };
//! // ... do work ...
//! if handle.is_finished() {
//!     tracing::warn!("heartbeat thread died before end-of-run; liveness signal was unavailable");
//! }
//! stop.signal();
//! let _ = handle.join(); // result discarded — WARN already surfaced above
//! ```

use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Read the heartbeat interval from `PHOTOHELPER_HEARTBEAT_INTERVAL_MS`, defaulting to 10s.
///
/// Tests set the env-var to a small value (e.g. 1ms) to exercise the heartbeat path
/// without waiting 10 seconds.
pub(crate) fn heartbeat_interval() -> Duration {
    if let Ok(s) = std::env::var("PHOTOHELPER_HEARTBEAT_INTERVAL_MS")
        && let Ok(ms) = s.parse::<u64>()
    {
        return Duration::from_millis(ms.max(10));
    }
    Duration::from_secs(10)
}

/// Cooperative stop signal for the heartbeat thread.
///
/// Pairs a `Mutex<bool>` with a `Condvar` so `run_heartbeat_loop`'s timed wait
/// can be cut short the instant `signal()` is called — no granularity-cycle
/// latency added to summary printing. Closes TD-003 (the previous
/// `AtomicBool` + `thread::sleep` design left the heartbeat orphaned past the
/// summary line).
pub(crate) struct HeartbeatStop {
    lock: Mutex<bool>,
    cvar: Condvar,
}

impl HeartbeatStop {
    pub(crate) fn new() -> Self {
        Self {
            lock: Mutex::new(false),
            cvar: Condvar::new(),
        }
    }

    /// Mark the stop flag and wake every waiter immediately.
    pub(crate) fn signal(&self) {
        let mut stopped = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        *stopped = true;
        drop(stopped);
        self.cvar.notify_all();
    }

    /// Wait up to `dur` for `signal()`; returns `true` if stop was observed.
    pub(crate) fn wait_for_stop(&self, dur: Duration) -> bool {
        let stopped = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        if *stopped {
            return true;
        }
        let (stopped, _) = self
            .cvar
            .wait_timeout(stopped, dur)
            .unwrap_or_else(|p| p.into_inner());
        *stopped
    }
}

/// Spawn a heartbeat thread that panics after its first tick (test-only seam).
///
/// Used by `ingest.rs` and `dedup.rs` tests to exercise the `is_finished()`
/// WARN path without the full `run_ingest`/`run_dedup` pipeline.
/// `pub(crate)` so integration-test modules in `commands/` can import it.
#[cfg(test)]
pub(crate) fn spawn_dying_heartbeat(
    stop: std::sync::Arc<HeartbeatStop>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("ph-heartbeat-dying".into())
        .spawn(move || {
            // Tick once (emit a heartbeat line so test can observe it if needed).
            eprintln!("[heartbeat] dying-test tick");
            // Wait for stop signal (don't hog CPU). If stop fires, return cleanly;
            // otherwise pause briefly then panic to simulate heartbeat death.
            if stop.wait_for_stop(std::time::Duration::from_millis(50)) {
                return;
            }
            panic!("test-induced heartbeat death (TD-010 row 4 seam)");
        })
        .expect("spawning dying heartbeat thread")
}

/// Tick-first heartbeat loop. Call `on_tick` every `interval`, then wait for
/// `stop.signal()`. Tick-first (DN-019 lesson): a wait-first loop races
/// thread-startup against `stop.signal()` and can return without printing;
/// tick-first guarantees at least one liveness signal per `interval` even
/// when the run is shorter than OS thread-startup latency.
///
/// `granularity = min(interval, 100ms)` ensures sub-100ms env overrides take
/// effect in tests while production still gets responsive-to-stop behavior.
pub(crate) fn run_heartbeat_loop(stop: &HeartbeatStop, interval: Duration, on_tick: impl Fn()) {
    let granularity = interval.min(Duration::from_millis(100));
    let ticks = interval
        .as_millis()
        .checked_div(granularity.as_millis())
        .unwrap_or(1)
        .max(1) as u64;
    let mut counter: u64 = 0;
    loop {
        counter += 1;
        if counter >= ticks {
            counter = 0;
            on_tick();
        }
        if stop.wait_for_stop(granularity) {
            return;
        }
    }
}
