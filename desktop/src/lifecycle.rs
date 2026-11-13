//! Start/stop handshake between the startup thread, tray Quit, the app exit hooks and the
//! Windows session-end watcher.
//!
//! - A stop requested while the stack is still starting waits for the start to finish, then
//!   stops what was started (the startup thread is told not to open the window).
//! - The stack is stopped once; every other caller blocks until that stop has **finished**
//!   (a caller returning early would let the process exit, closing the job object, in the
//!   middle of the server's ledger flush or before `pg_ctl stop`).
//! - Once Windows has announced the end of the session, any stop uses the short
//!   [`StopBudget::SessionEnd`], whichever thread runs it.

use std::sync::{Condvar, Mutex, MutexGuard};

use crate::supervise::{Stack, StopBudget, StopError};

/// Something [`Lifecycle`] can stop.
pub trait Stoppable: Send {
    type Error;
    fn stop(&mut self, budget: StopBudget) -> Result<(), Self::Error>;
}

impl Stoppable for Stack {
    type Error = StopError;
    fn stop(&mut self, budget: StopBudget) -> Result<(), StopError> {
        self.stop_with(budget)
    }
}

/// Result of [`Lifecycle::stop`].
#[derive(Debug)]
pub enum StopOutcome<E> {
    /// This call ran the stop (with this budget).
    Stopped(StopBudget, Result<(), E>),
    /// Nothing to stop: the start failed, or another caller already stopped it (this call
    /// waited until that stop had finished).
    NothingRunning,
}

struct State<S> {
    starting: bool,
    stopping: bool,
    /// A [`Lifecycle::with_running`] operation holds the stack.
    busy: bool,
    stop_requested: bool,
    session_ending: bool,
    stack: Option<S>,
}

pub struct Lifecycle<S> {
    state: Mutex<State<S>>,
    changed: Condvar,
}

impl<S: Stoppable> Lifecycle<S> {
    /// A lifecycle whose start is in progress.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                starting: true,
                stopping: false,
                busy: false,
                stop_requested: false,
                session_ending: false,
                stack: None,
            }),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State<S>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Ends the start, handing over the started stack (`None` when the start failed).
    /// Returns `false` when a stop was requested meanwhile: the stack is kept for that stop and
    /// the caller must not open the window.
    pub fn finish_start(&self, stack: Option<S>) -> bool {
        let mut st = self.lock();
        st.starting = false;
        st.stack = stack;
        let proceed = !st.stop_requested;
        drop(st);
        self.changed.notify_all();
        proceed
    }

    /// Marks a stop as requested without waiting (Quit). `true` for the first request only.
    pub fn request_stop(&self) -> bool {
        !std::mem::replace(&mut self.lock().stop_requested, true)
    }

    /// Windows announced (`true`) or cancelled (`false`) the end of the session.
    pub fn set_session_ending(&self, ending: bool) {
        self.lock().session_ending = ending;
    }

    /// Stops the stack once. Waits for an in-progress start, and for a stop already run by
    /// another caller, so it never returns before the stack is fully stopped.
    pub fn stop(&self, budget: StopBudget) -> StopOutcome<S::Error> {
        let mut st = self.lock();
        st.stop_requested = true;
        while st.starting || st.stopping || st.busy {
            st = self
                .changed
                .wait(st)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        let Some(mut stack) = st.stack.take() else {
            return StopOutcome::NothingRunning;
        };
        st.stopping = true;
        let budget = if st.session_ending {
            StopBudget::SessionEnd
        } else {
            budget
        };
        drop(st);
        let result = stack.stop(budget);
        drop(stack);
        self.lock().stopping = false;
        self.changed.notify_all();
        StopOutcome::Stopped(budget, result)
    }

    /// Runs `f` on the running stack, one operation at a time (export, import). `None` when
    /// there is no running stack (start failed, stop requested or done). A stop requested
    /// meanwhile waits for `f` to return, then stops the stack.
    pub fn with_running<R>(&self, f: impl FnOnce(&mut S) -> R) -> Option<R> {
        let mut st = self.lock();
        while st.starting || st.busy {
            st = self
                .changed
                .wait(st)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if st.stop_requested || st.stopping {
            return None;
        }
        let stack = st.stack.take()?;
        st.busy = true;
        drop(st);
        // Hands the stack back even if `f` panics.
        let mut guard = Lease {
            life: self,
            stack: Some(stack),
        };
        let stack = guard.stack.as_mut()?;
        Some(f(stack))
    }
}

/// The stack lent to a [`Lifecycle::with_running`] operation; returned on drop.
struct Lease<'a, S: Stoppable> {
    life: &'a Lifecycle<S>,
    stack: Option<S>,
}

impl<S: Stoppable> Drop for Lease<'_, S> {
    fn drop(&mut self) {
        let mut st = self.life.lock();
        st.stack = self.stack.take();
        st.busy = false;
        drop(st);
        self.life.changed.notify_all();
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::supervise::fakes::*;
    use crate::supervise::{Timings, SERVER_PORT};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// A server that ignores the shutdown request: a full stop waits `stop_timeout` (300 ms in
    /// `fast()`) before the kill, which leaves time for a concurrent caller to arrive.
    fn slow_stack(log: &Log, data: &std::path::Path) -> Stack {
        let mut host = FakeHost::new(log);
        host.honours_shutdown = false;
        let (stack, _host) = start(host, data);
        stack.expect("start")
    }

    #[test]
    fn concurrent_stop_waits_until_the_first_stop_has_finished() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let life = Arc::new(Lifecycle::new());
        assert!(life.finish_start(Some(slow_stack(&log, tmp.path()))));
        let first = {
            let life = life.clone();
            std::thread::spawn(move || life.stop(StopBudget::Full))
        };
        // Let the first caller take the stack and enter its graceful wait.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !entries(&log)
            .iter()
            .any(|e| e.starts_with("server.shutdown"))
        {
            assert!(Instant::now() < deadline, "first stop never started");
            std::thread::sleep(Duration::from_millis(5));
        }
        let second = life.stop(StopBudget::Full);
        assert!(matches!(second, StopOutcome::NothingRunning), "{second:?}");
        // The second caller returned only after the whole stop, Postgres included.
        assert!(
            entries(&log).contains(&"db.stop".to_string()),
            "{:?}",
            entries(&log)
        );
        let first = first.join().expect("join");
        assert!(matches!(
            first,
            StopOutcome::Stopped(StopBudget::Full, Ok(()))
        ));
    }

    #[test]
    fn second_sequential_stop_is_a_no_op() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let life = Lifecycle::new();
        let (stack, _host) = start(FakeHost::new(&log), tmp.path());
        assert!(life.finish_start(Some(stack.expect("start"))));
        assert!(matches!(
            life.stop(StopBudget::Full),
            StopOutcome::Stopped(_, Ok(()))
        ));
        let after_first = entries(&log);
        assert!(matches!(
            life.stop(StopBudget::Full),
            StopOutcome::NothingRunning
        ));
        assert_eq!(entries(&log), after_first);
    }

    #[test]
    fn stop_requested_while_starting_stops_what_was_started() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let life = Arc::new(Lifecycle::new());
        let stopper = {
            let life = life.clone();
            std::thread::spawn(move || life.stop(StopBudget::Full))
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!stopper.is_finished(), "stop must wait for the start");
        let (stack, _host) = start(FakeHost::new(&log), tmp.path());
        // The start completes, but the window must not be opened.
        assert!(!life.finish_start(Some(stack.expect("start"))));
        let outcome = stopper.join().expect("join");
        assert!(matches!(outcome, StopOutcome::Stopped(_, Ok(()))));
        let log = entries(&log);
        assert!(log.contains(&format!("server.shutdown({SERVER_PORT})")));
        assert_eq!(log.last().map(String::as_str), Some("db.stop"));
    }

    #[test]
    fn quit_request_while_starting_blocks_the_window() {
        let life: Lifecycle<Stack> = Lifecycle::new();
        assert!(life.request_stop());
        assert!(!life.request_stop(), "only the first request counts");
        assert!(!life.finish_start(None));
        assert!(matches!(
            life.stop(StopBudget::Full),
            StopOutcome::NothingRunning
        ));
    }

    #[test]
    fn with_running_lends_the_stack_and_a_stop_waits_for_the_operation() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let life = Arc::new(Lifecycle::new());
        let (stack, _host) = start(FakeHost::new(&log), tmp.path());
        assert!(life.finish_start(Some(stack.expect("start"))));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let op = {
            let life = life.clone();
            std::thread::spawn(move || {
                life.with_running(|stack| {
                    entered_tx.send(stack.port()).expect("send");
                    release_rx.recv().expect("release");
                    "done"
                })
            })
        };
        assert_eq!(entered_rx.recv().expect("entered"), SERVER_PORT);
        let stopper = {
            let life = life.clone();
            std::thread::spawn(move || life.stop(StopBudget::Full))
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!stopper.is_finished(), "stop must wait for the operation");
        assert!(!entries(&log).contains(&"db.stop".to_string()));
        release_tx.send(()).expect("release");
        assert_eq!(op.join().expect("join"), Some("done"));
        assert!(matches!(
            stopper.join().expect("join"),
            StopOutcome::Stopped(_, Ok(()))
        ));
        // Once stopped, nothing is lent any more.
        assert!(life.with_running(|_| ()).is_none());
    }

    #[test]
    fn once_the_session_is_ending_every_stop_uses_the_short_budget() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = Log::default();
        let mut host = FakeHost::new(&log);
        host.honours_shutdown = false;
        let timings = Timings {
            stop_timeout: Duration::from_secs(30),
            session_end_stop_timeout: Duration::from_millis(50),
            ..fast()
        };
        let (stack, _host) = start_with_timings(host, tmp.path(), timings);
        let life = Lifecycle::new();
        assert!(life.finish_start(Some(stack.expect("start"))));
        life.set_session_ending(true);
        let began = Instant::now();
        // The app exit hook asks for a full stop; the session end wins.
        let outcome = life.stop(StopBudget::Full);
        assert!(began.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            outcome,
            StopOutcome::Stopped(StopBudget::SessionEnd, Ok(()))
        ));
    }
}
