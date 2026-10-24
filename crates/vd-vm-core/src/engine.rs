//! The owner thread.
//!
//! Exactly one thread ever touches the Voicemeeter DLL. Everything else talks
//! to it over channels, which sidesteps the DLL's thread-safety question
//! entirely and means no locks on the hot path.
//!
//! The loop is event-driven rather than a busy poll: it sleeps until the next
//! piece of work is actually due, so an idle mixer costs ~20 wakeups a second
//! (just the dirty check) and nothing else.

use crate::api::{LevelKind, LoginStatus, VmError, VoicemeeterApi, VoicemeeterKind};
use crate::state::{diff, read_state, VmDelta, VmState};
use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// How often to ask "did anything change?" — 20 Hz.
const DIRTY_INTERVAL: Duration = Duration::from_millis(50);
/// Meter refresh, capped at 30 Hz and only while something is displaying them.
const LEVEL_INTERVAL: Duration = Duration::from_millis(33);
/// Deltas are batched this long before being sent, so a fader drag produces a
/// steady ~30 messages a second instead of one per parameter.
const COALESCE_INTERVAL: Duration = Duration::from_millis(33);
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(5);

/// Peak level per channel, left and right. A side that was not requested comes
/// back empty rather than zeroed, so the UI can tell "silent" from "not asked".
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Levels {
    pub strips: Vec<[f32; 2]>,
    pub buses: Vec<[f32; 2]>,
}

/// Which meters are actually on screen.
///
/// Every channel costs two `VBVMR_GetLevel` calls per frame, and those calls
/// are the most expensive thing the engine does — reading all 16 channels of a
/// Potato caps the loop around 21 Hz. Asking only for the visible half (one
/// mixer tab) keeps the full 30 Hz, and asking for nothing costs nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LevelScope {
    pub strips: bool,
    pub buses: bool,
}

impl LevelScope {
    /// No meters at all — the window is hidden or minimised to tray.
    pub fn none() -> Self {
        Self { strips: false, buses: false }
    }
    pub fn strips_only() -> Self {
        Self { strips: true, buses: false }
    }
    pub fn buses_only() -> Self {
        Self { strips: false, buses: true }
    }
    pub fn all() -> Self {
        Self { strips: true, buses: true }
    }
    pub fn is_off(&self) -> bool {
        !self.strips && !self.buses
    }
}

/// Raises the Windows timer resolution to 1 ms for as long as it is alive.
///
/// Windows wakes sleeping threads on a ~15.6 ms tick by default, so a 33 ms
/// meter interval actually lands on 46.8 ms — about 21 Hz instead of 30. Media
/// apps ask for finer granularity while they need it; since Windows 10 2004
/// the request is per-process, so it does not slow the rest of the system.
///
/// Deliberately scoped to *while meters are streaming*: idle VoiceDeck leaves
/// the timer alone, which is the whole point of the low-power idle path.
#[cfg(windows)]
struct TimerResolution;

#[cfg(windows)]
#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

#[cfg(windows)]
impl TimerResolution {
    fn acquire() -> Self {
        // SAFETY: a plain winmm call; every begin is paired with an end in Drop.
        unsafe { timeBeginPeriod(1) };
        tracing::debug!("raised timer resolution to 1ms for meters");
        Self
    }
}

#[cfg(windows)]
impl Drop for TimerResolution {
    fn drop(&mut self) {
        unsafe { timeEndPeriod(1) };
        tracing::debug!("released 1ms timer resolution");
    }
}

#[cfg(not(windows))]
struct TimerResolution;

#[cfg(not(windows))]
impl TimerResolution {
    fn acquire() -> Self {
        Self
    }
}

/// Work for the owner thread.
#[derive(Debug, Clone)]
pub enum Command {
    SetFloat { name: String, value: f32 },
    SetString { name: String, value: String },
    /// Apply several parameters in a single DLL call.
    SetMany(Vec<(String, f32)>),
    /// Ask for a full state emit (window opened or reloaded).
    RequestSnapshot,
    /// Choose which meters to stream. `LevelScope::none()` stops all level
    /// calls, which is the idle-CPU win when no window is showing meters.
    SetLevelScope(LevelScope),
    /// Restart the Voicemeeter audio engine (the classic crackle fix).
    RestartEngine,
    Shutdown,
}

/// What the owner thread reports.
#[derive(Debug, Clone)]
pub enum Event {
    Connected(Box<VmState>),
    Disconnected { reason: String },
    /// Only the fields that changed.
    Delta(VmDelta),
    /// Full state, in reply to `RequestSnapshot`.
    Snapshot(Box<VmState>),
    Levels(Levels),
}

/// Handle to the running engine.
pub struct Engine {
    commands: Sender<Command>,
    events: Receiver<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    /// Start the owner thread.
    ///
    /// `connect` is called to (re)acquire the API — a closure rather than a
    /// value so reconnecting after Voicemeeter restarts can build a fresh one.
    pub fn spawn<F>(connect: F) -> Self
    where
        F: Fn() -> crate::api::Result<Box<dyn VoicemeeterApi>> + Send + 'static,
    {
        let (cmd_tx, cmd_rx) = unbounded::<Command>();
        // Bounded so a stalled consumer cannot grow the queue without limit;
        // meter frames are droppable by design.
        let (evt_tx, evt_rx) = bounded::<Event>(256);

        let thread = std::thread::Builder::new()
            .name("vm-core".into())
            .spawn(move || run(connect, cmd_rx, evt_tx))
            .expect("spawning the vm-core thread");

        Self {
            commands: cmd_tx,
            events: evt_rx,
            thread: Some(thread),
        }
    }

    pub fn commands(&self) -> Sender<Command> {
        self.commands.clone()
    }

    pub fn events(&self) -> Receiver<Event> {
        self.events.clone()
    }

    pub fn send(&self, cmd: Command) {
        // A closed channel means the thread already exited; nothing to do.
        let _ = self.commands.send(cmd);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Owns the DLL handle and logs out when dropped, including on panic.
struct Session {
    api: Box<dyn VoicemeeterApi>,
    kind: VoicemeeterKind,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Err(e) = self.api.logout() {
            tracing::warn!(error = %e, "VBVMR_Logout failed");
        } else {
            tracing::debug!("logged out of Voicemeeter");
        }
    }
}

/// Try to log in and read the initial state.
fn connect_session<F>(connect: &F) -> crate::api::Result<(Session, VmState)>
where
    F: Fn() -> crate::api::Result<Box<dyn VoicemeeterApi>>,
{
    let api = connect()?;
    match api.login()? {
        LoginStatus::Ok | LoginStatus::AlreadyLoggedIn => {}
        LoginStatus::EngineNotRunning => {
            // Logged in, but the GUI is not up. Bail and let the caller retry;
            // launching it is a deliberate user action, not something to do
            // behind their back on every reconnect.
            return Err(VmError::EngineDown);
        }
    }

    let kind = api.kind()?;
    let state = read_state(api.as_ref(), kind)?;
    // Prime the dirty flag so the first real change is not swallowed.
    let _ = api.is_dirty();

    Ok((Session { api, kind }, state))
}

fn run<F>(connect: F, commands: Receiver<Command>, events: Sender<Event>)
where
    F: Fn() -> crate::api::Result<Box<dyn VoicemeeterApi>>,
{
    let mut session: Option<Session> = None;
    let mut cached: Option<VmState> = None;
    let mut scope = LevelScope::none();
    // Held only while meters stream; dropping it restores the default tick.
    let mut timer: Option<TimerResolution> = None;
    let mut backoff = RECONNECT_MIN;

    let now = Instant::now();
    let mut next_dirty = now;
    let mut next_level = now;
    let mut next_flush = now + COALESCE_INTERVAL;
    let mut pending = VmDelta::default();

    loop {
        // Sleep until the soonest piece of work, waking early for commands.
        let now = Instant::now();
        let mut deadline = next_dirty;
        if !scope.is_off() && session.is_some() && next_level < deadline {
            deadline = next_level;
        }
        if !pending.is_empty() && next_flush < deadline {
            deadline = next_flush;
        }
        let timeout = deadline.saturating_duration_since(now);

        match commands.recv_timeout(timeout) {
            Ok(Command::Shutdown) => break,
            Ok(cmd) => {
                if let Some(s) = session.as_ref() {
                    apply_command(s, cmd, &events, &mut cached, &mut scope);
                    sync_timer(&scope, &mut timer);
                } else if let Command::SetLevelScope(next) = cmd {
                    // Remember it even while disconnected, so meters resume
                    // automatically once Voicemeeter comes back.
                    scope = next;
                    sync_timer(&scope, &mut timer);
                }
                continue; // drain the rest of the queue before timed work
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        let now = Instant::now();

        // ---- reconnect ----
        if session.is_none() {
            if now < next_dirty {
                continue;
            }
            match connect_session(&connect) {
                Ok((s, state)) => {
                    tracing::info!(kind = ?s.kind, "connected to Voicemeeter");
                    backoff = RECONNECT_MIN;
                    cached = Some(state.clone());
                    session = Some(s);
                    let _ = events.send(Event::Connected(Box::new(state)));
                    next_dirty = now + DIRTY_INTERVAL;
                }
                Err(e) => {
                    tracing::debug!(error = %e, backoff = ?backoff, "Voicemeeter unavailable");
                    next_dirty = now + backoff;
                    // 1s, 2s, 4s, then hold at 5s — no spinning.
                    backoff = (backoff * 2).min(RECONNECT_MAX);
                }
            }
            continue;
        }

        let s = session.as_ref().expect("checked above");

        // ---- change detection ----
        if now >= next_dirty {
            // Advance on a fixed cadence rather than from `now`, so wake
            // latency does not accumulate into an ever-slower loop.
            next_dirty = schedule_next(next_dirty, now, DIRTY_INTERVAL);
            match s.api.is_dirty() {
                Ok(true) => match read_state(s.api.as_ref(), s.kind) {
                    Ok(fresh) => {
                        if let Some(old) = cached.as_ref() {
                            let d = diff(old, &fresh);
                            if !d.is_empty() {
                                pending.merge(d);
                            }
                        }
                        cached = Some(fresh);
                    }
                    Err(e) => tracing::warn!(error = %e, "reading state failed"),
                },
                Ok(false) => {}
                Err(e) => {
                    // Voicemeeter went away; drop the session and back off.
                    tracing::info!(error = %e, "lost the Voicemeeter connection");
                    session = None;
                    cached = None;
                    pending = VmDelta::default();
                    timer = None; // nothing to meter while disconnected
                    let _ = events.send(Event::Disconnected { reason: e.to_string() });
                    next_dirty = Instant::now() + backoff;
                    continue;
                }
            }
        }

        // ---- meters ----
        if !scope.is_off() && now >= next_level {
            next_level = schedule_next(next_level, now, LEVEL_INTERVAL);
            if let Some(state) = cached.as_ref() {
                // try_send: meter frames are droppable, never block the engine
                // waiting on a slow consumer.
                let _ = events.try_send(Event::Levels(read_levels(s, state, scope)));
            }
        }

        // ---- flush coalesced deltas ----
        if !pending.is_empty() && now >= next_flush {
            next_flush = schedule_next(next_flush, now, COALESCE_INTERVAL);
            let _ = events.send(Event::Delta(std::mem::take(&mut pending)));
        }
    }

    // `session` drops here, which logs out.
    tracing::debug!("vm-core thread exiting");
}

/// Next deadline on a fixed cadence.
///
/// Adding the interval to the previous deadline keeps the average rate exact.
/// If we fell more than a whole period behind (a stall, or the machine slept),
/// resynchronise from now instead of trying to catch up in a burst.
fn schedule_next(previous: Instant, now: Instant, interval: Duration) -> Instant {
    let next = previous + interval;
    if next <= now {
        now + interval
    } else {
        next
    }
}

/// Acquire or release the 1 ms timer to match whether meters are streaming.
fn sync_timer(scope: &LevelScope, timer: &mut Option<TimerResolution>) {
    if scope.is_off() {
        *timer = None;
    } else if timer.is_none() {
        *timer = Some(TimerResolution::acquire());
    }
}

fn apply_command(
    s: &Session,
    cmd: Command,
    events: &Sender<Event>,
    cached: &mut Option<VmState>,
    scope: &mut LevelScope,
) {
    match cmd {
        Command::SetFloat { name, value } => {
            if let Err(e) = s.api.set_float(&name, value) {
                tracing::warn!(%name, error = %e, "set_float failed");
            }
        }
        Command::SetString { name, value } => {
            if let Err(e) = s.api.set_string(&name, &value) {
                tracing::warn!(%name, error = %e, "set_string failed");
            }
        }
        Command::SetMany(pairs) => {
            if pairs.is_empty() {
                return;
            }
            // One DLL call for the whole batch: `Strip[0].Gain=-6;Strip[1].Mute=1`
            let script = pairs
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join(";");
            if let Err(e) = s.api.set_parameters(&script) {
                tracing::warn!(error = %e, "set_parameters failed");
            }
        }
        Command::RequestSnapshot => {
            // Read fresh rather than trusting the cache: the window may have
            // been closed for a while with changes made in Voicemeeter itself.
            match read_state(s.api.as_ref(), s.kind) {
                Ok(state) => {
                    *cached = Some(state.clone());
                    let _ = events.send(Event::Snapshot(Box::new(state)));
                }
                Err(e) => tracing::warn!(error = %e, "snapshot failed"),
            }
        }
        Command::SetLevelScope(next) => {
            tracing::debug!(?next, "level scope");
            *scope = next;
        }
        Command::RestartEngine => {
            if let Err(e) = s.api.set_float("Command.Restart", 1.0) {
                tracing::warn!(error = %e, "engine restart failed");
            }
        }
        Command::Shutdown => {}
    }
}

fn read_levels(s: &Session, state: &VmState, scope: LevelScope) -> Levels {
    let layout = s.kind.layout();
    let strips = if !scope.strips { Vec::new() } else { (0..state.strips.len())
        .map(|i| {
            let base = layout.strip_level_base(i);
            [
                s.api.get_level(LevelKind::PostMuteInput, base).unwrap_or(0.0),
                s.api.get_level(LevelKind::PostMuteInput, base + 1).unwrap_or(0.0),
            ]
        })
        .collect() };
    let buses = if !scope.buses { Vec::new() } else { (0..state.buses.len())
        .map(|j| {
            let base = layout.bus_level_base(j);
            [
                s.api.get_level(LevelKind::BusOutput, base).unwrap_or(0.0),
                s.api.get_level(LevelKind::BusOutput, base + 1).unwrap_or(0.0),
            ]
        })
        .collect() };
    Levels { strips, buses }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockApi;
    use std::sync::Arc;

    /// Wait for an event matching `pred`, or fail after a second.
    fn wait_for<T>(rx: &Receiver<Event>, mut pred: impl FnMut(&Event) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(evt) = rx.recv_timeout(Duration::from_millis(100)) {
                if let Some(v) = pred(&evt) {
                    return v;
                }
            }
        }
        panic!("timed out waiting for the expected event");
    }

    fn engine_with(mock: Arc<MockApi>) -> Engine {
        Engine::spawn(move || {
            let m = mock.clone();
            Ok(Box::new(ArcApi(m)) as Box<dyn VoicemeeterApi>)
        })
    }

    /// Lets the test keep a handle on the same mock the engine is driving.
    struct ArcApi(Arc<MockApi>);

    impl VoicemeeterApi for ArcApi {
        fn login(&self) -> crate::api::Result<LoginStatus> { self.0.login() }
        fn logout(&self) -> crate::api::Result<()> { self.0.logout() }
        fn run_voicemeeter(&self, k: VoicemeeterKind) -> crate::api::Result<()> { self.0.run_voicemeeter(k) }
        fn kind(&self) -> crate::api::Result<VoicemeeterKind> { self.0.kind() }
        fn is_dirty(&self) -> crate::api::Result<bool> { self.0.is_dirty() }
        fn get_float(&self, n: &str) -> crate::api::Result<f32> { self.0.get_float(n) }
        fn set_float(&self, n: &str, v: f32) -> crate::api::Result<()> { self.0.set_float(n, v) }
        fn get_string(&self, n: &str) -> crate::api::Result<String> { self.0.get_string(n) }
        fn set_string(&self, n: &str, v: &str) -> crate::api::Result<()> { self.0.set_string(n, v) }
        fn set_parameters(&self, s: &str) -> crate::api::Result<()> { self.0.set_parameters(s) }
        fn get_level(&self, k: LevelKind, c: usize) -> crate::api::Result<f32> { self.0.get_level(k, c) }
    }

    #[test]
    fn connects_and_reports_initial_state() {
        let mock = Arc::new(MockApi::new(VoicemeeterKind::Potato));
        let engine = engine_with(mock);
        let events = engine.events();

        let strips = wait_for(&events, |e| match e {
            Event::Connected(s) => Some(s.strips.len()),
            _ => None,
        });
        assert_eq!(strips, 8);
    }

    #[test]
    fn a_change_in_voicemeeter_produces_a_delta() {
        let mock = Arc::new(MockApi::new(VoicemeeterKind::Potato));
        let engine = engine_with(mock.clone());
        let events = engine.events();
        wait_for(&events, |e| matches!(e, Event::Connected(_)).then_some(()));

        // As if the user moved a fader in Voicemeeter itself.
        mock.poke("Strip[2].Gain", -8.0);

        let delta = wait_for(&events, |e| match e {
            Event::Delta(d) => Some(d.clone()),
            _ => None,
        });
        assert_eq!(delta.strips.len(), 1);
        assert_eq!(delta.strips[0].index, 2);
        assert_eq!(delta.strips[0].gain, Some(-8.0));
    }

    #[test]
    fn an_idle_mixer_emits_nothing() {
        let mock = Arc::new(MockApi::new(VoicemeeterKind::Potato));
        let engine = engine_with(mock);
        let events = engine.events();
        wait_for(&events, |e| matches!(e, Event::Connected(_)).then_some(()));

        // Nothing changed, so several dirty-check cycles must stay silent.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            events.try_recv().is_err(),
            "an untouched mixer must not generate traffic"
        );
    }

    #[test]
    fn batched_writes_become_one_call() {
        let mock = Arc::new(MockApi::new(VoicemeeterKind::Potato));
        let engine = engine_with(mock.clone());
        let events = engine.events();
        wait_for(&events, |e| matches!(e, Event::Connected(_)).then_some(()));

        engine.send(Command::SetMany(vec![
            ("Strip[0].Gain".into(), -6.0),
            ("Strip[1].Gain".into(), -3.0),
        ]));
        std::thread::sleep(Duration::from_millis(150));

        let scripts = mock.scripts();
        assert_eq!(scripts.len(), 1, "the batch must be a single DLL call");
        assert!(scripts[0].contains("Strip[0].Gain=-6"));
        assert!(scripts[0].contains(';'), "statements are semicolon separated");
    }

    #[test]
    fn levels_only_flow_when_asked_for() {
        let mock = Arc::new(MockApi::new(VoicemeeterKind::Potato));
        mock.set_level(LevelKind::PostMuteInput, 0, 0.5);
        let engine = engine_with(mock.clone());
        let events = engine.events();
        wait_for(&events, |e| matches!(e, Event::Connected(_)).then_some(()));

        std::thread::sleep(Duration::from_millis(200));
        assert!(events.try_recv().is_err(), "meters are off by default");

        engine.send(Command::SetLevelScope(LevelScope::all()));
        let left = wait_for(&events, |e| match e {
            Event::Levels(l) => Some(l.strips[0][0]),
            _ => None,
        });
        assert_eq!(left, 0.5);
    }

    #[test]
    fn survives_voicemeeter_going_away_and_coming_back() {
        let mock = Arc::new(MockApi::new(VoicemeeterKind::Potato));
        let engine = engine_with(mock.clone());
        let events = engine.events();
        wait_for(&events, |e| matches!(e, Event::Connected(_)).then_some(()));

        mock.set_engine_running(false);
        wait_for(&events, |e| matches!(e, Event::Disconnected { .. }).then_some(()));

        mock.set_engine_running(true);
        wait_for(&events, |e| matches!(e, Event::Connected(_)).then_some(()));
    }

    #[test]
    fn logs_out_when_the_engine_is_dropped() {
        let mock = Arc::new(MockApi::new(VoicemeeterKind::Potato));
        let engine = engine_with(mock.clone());
        wait_for(&engine.events(), |e| matches!(e, Event::Connected(_)).then_some(()));
        assert!(mock.is_logged_in());

        drop(engine);
        assert!(!mock.is_logged_in(), "Drop must call VBVMR_Logout");
    }
}
