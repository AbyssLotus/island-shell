//! Island Shell's module host (docs/design/modules-handoff.md §4.5–§4.8): loads module folders,
//! runs their backends by run mode, restarts crashed ones with backoff, forwards UI calls, and routes
//! what backends send (UI events, notifications, activities, calendar events, topics, logs).
//!
//! Everything the island shows goes through a [`Sink`], so this crate has no Tauri in it and its
//! tests run anywhere with a real backend process (src/bin/echo-module.rs).

pub mod installed;
mod job;
pub mod ui;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use island_module_proto::manifest::{Manifest, RunMode};
use island_module_proto::msg::{Activity, CalendarPut, Emit, Init, Log, Notify, Theme, TopicData};
use island_module_proto::{code, method, write_message, FrameReader, Message, RpcError};
use serde::Serialize;
use serde_json::{json, Value};

pub use island_module_proto as proto;

/// Where the island shows what modules send.
pub trait Sink: Send + Sync + 'static {
    /// The Tauri event `mod:<module>:<name>`.
    fn emit(&self, module: &str, name: &str, data: Value);
    fn notify(&self, module: &str, n: Notify);
    /// Every live activity, highest priority first (ties: the most recent first).
    fn activities(&self, list: Vec<ActivityOf>);
    fn calendar(&self, module: &str, put: CalendarPut);
    /// A line for island.log.
    fn log(&self, module: &str, level: &str, msg: &str);
    /// A module's state changed (for the Modules sheet).
    fn changed(&self, info: &ModuleInfo);
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ActivityOf {
    pub module: String,
    #[serde(flatten)]
    pub activity: Activity,
    /// Order of arrival, for ties.
    pub seq: u64,
}

#[derive(Clone, Debug)]
pub struct Paths {
    /// `%APPDATA%\league.seaweed.islandshell\modules`: one data folder per module.
    pub data_root: PathBuf,
    /// `%LOCALAPPDATA%\league.seaweed.islandshell\modules-cache`.
    pub cache_root: PathBuf,
    /// `%APPDATA%\league.seaweed.islandshell`: where 0.1.x kept its files.
    pub legacy_dir: PathBuf,
}

/// The timeouts and delays of §4.5–§4.6 (tests shorten them).
#[derive(Clone, Debug)]
pub struct Timing {
    pub init: Duration,
    pub call: Duration,
    pub tick: Duration,
    pub shutdown: Duration,
    /// Restart delays after the first, second and third crash.
    pub backoff: [Duration; 3],
    /// Three crashes inside this window and the module stays stopped.
    pub crash_window: Duration,
    /// A `while-process` module stops this long after its process exits.
    pub process_grace: Duration,
    /// A `scheduled` module exits this long after its tick or last call.
    pub scheduled_idle: Duration,
    /// The first tick of a `scheduled` module comes this long after it's loaded.
    pub first_tick: Duration,
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            init: Duration::from_secs(5),
            call: Duration::from_secs(15),
            tick: Duration::from_secs(60),
            shutdown: Duration::from_secs(2),
            backoff: [Duration::from_secs(1), Duration::from_secs(5), Duration::from_secs(30)],
            crash_window: Duration::from_secs(300),
            process_grace: Duration::from_secs(5),
            scheduled_idle: Duration::from_secs(10),
            first_tick: Duration::from_secs(20),
            poll: Duration::from_millis(250),
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// Turned off by the user.
    Disabled,
    /// Not running; starts when its run mode says so.
    Stopped,
    Starting,
    Running,
    /// Crashed; restarts after a delay.
    Backoff,
    /// Crashed too often, or can't run at all: stays stopped until Restart.
    Failed,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModuleInfo {
    pub id: String,
    pub manifest: Manifest,
    pub dir: String,
    pub dev: bool,
    pub enabled: bool,
    pub state: State,
    pub pid: Option<u32>,
    pub error: String,
    /// Crashes in the last window.
    pub crashes: usize,
}

const LOG_LINES: usize = 200;

struct Proc {
    pid: u32,
    gen: u64,
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Result<Value, RpcError>>>>,
    next_id: AtomicU64,
    /// Set once `island.init` has answered: Ok, or why it failed.
    ready: (Mutex<Option<Result<(), RpcError>>>, Condvar),
    /// Calls in flight, so an idle stop never cuts one off.
    busy: AtomicUsize,
}

impl Proc {
    fn send(&self, m: &Message) -> bool {
        let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        write_message(&mut *stdin, m).is_ok()
    }

    fn request(&self, method: &str, params: impl Serialize, timeout: Duration) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if !self.send(&Message::request(id, method, params)) {
            self.pending.lock().unwrap().remove(&id);
            return Err(RpcError::new(code::STOPPED, "The module stopped"));
        }
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(RpcError::new(code::FAILED, format!("The module didn't answer {method} in {} s", timeout.as_secs())))
            }
        }
    }

    fn wait_ready(&self, timeout: Duration) -> Result<(), RpcError> {
        let (lock, cv) = &self.ready;
        let guard = lock.lock().unwrap();
        let (guard, _) = cv.wait_timeout_while(guard, timeout, |r| r.is_none()).unwrap();
        guard.clone().unwrap_or_else(|| Err(RpcError::new(code::STOPPED, "The module is still starting")))
    }

    /// The first answer wins (init's, or the exit's if init never answered).
    fn set_ready(&self, r: Result<(), RpcError>) {
        let (lock, cv) = &self.ready;
        let mut g = lock.lock().unwrap();
        if g.is_none() {
            *g = Some(r);
        }
        cv.notify_all();
    }

    fn kill(&self) {
        let _ = self.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
    }
}

struct Slot {
    manifest: Manifest,
    dir: PathBuf,
    dev: bool,
    enabled: bool,
    state: State,
    error: String,
    proc: Option<Arc<Proc>>,
    gen: u64,
    /// We asked it to stop, so its exit isn't a crash.
    stopping: bool,
    crashes: VecDeque<Instant>,
    retry_at: Option<Instant>,
    last_used: Instant,
    /// Last time one of its `while-process` processes was seen.
    process_seen: Option<Instant>,
    next_tick: Option<Instant>,
    ticking: bool,
    logs: VecDeque<String>,
}

impl Slot {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: self.manifest.id.clone(),
            manifest: self.manifest.clone(),
            dir: self.dir.to_string_lossy().into_owned(),
            dev: self.dev,
            enabled: self.enabled,
            state: self.state,
            pid: self.proc.as_ref().map(|p| p.pid),
            error: self.error.clone(),
            crashes: self.crashes.len(),
        }
    }

    fn mode(&self) -> Option<RunMode> {
        self.manifest.backend.as_ref().map(|b| b.run)
    }

    fn push_log(&mut self, line: String) {
        if self.logs.len() >= LOG_LINES {
            self.logs.pop_front();
        }
        self.logs.push_back(line);
    }
}

struct Inner {
    sink: Arc<dyn Sink>,
    paths: Paths,
    timing: Timing,
    base_version: String,
    env: Mutex<(String, Theme)>,
    slots: Mutex<HashMap<String, Slot>>,
    topics: Mutex<HashMap<String, Value>>,
    activities: Mutex<Vec<ActivityOf>>,
    seq: AtomicU64,
    processes: Mutex<HashSet<String>>,
    job: Option<job::Job>,
    quit: AtomicBool,
}

/// The module host. Cheap to clone; the background loop stops when [`Supervisor::shutdown`] runs.
#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

impl Supervisor {
    pub fn new(paths: Paths, sink: Arc<dyn Sink>, base_version: &str, timing: Timing) -> Supervisor {
        let inner = Arc::new(Inner {
            sink,
            paths,
            timing,
            base_version: base_version.into(),
            env: Mutex::new(("en-US".into(), Theme { dark: true, accent: "#C9E9FF".into() })),
            slots: Mutex::new(HashMap::new()),
            topics: Mutex::new(HashMap::new()),
            activities: Mutex::new(Vec::new()),
            seq: AtomicU64::new(1),
            processes: Mutex::new(HashSet::new()),
            job: job::Job::new(),
            quit: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new().name("modules".into()).spawn(move || run_loop(weak)).expect("module supervisor thread");
        Supervisor { inner }
    }

    /// Reads and checks `module.json` in `dir`, and registers the module (it starts when its run mode
    /// says so). Returns its id.
    pub fn load(&self, dir: &Path, dev: bool, enabled: bool) -> Result<String, String> {
        let bytes = std::fs::read(dir.join("module.json")).map_err(|e| format!("Couldn't read module.json: {e}"))?;
        let manifest = Manifest::parse(&bytes)?;
        if let Some(b) = &manifest.backend {
            if !dir.join(&b.exe).is_file() {
                return Err(format!("{}: its backend {} is missing", manifest.id, b.exe));
            }
        }
        let id = manifest.id.clone();
        let mut slots = self.inner.slots.lock().unwrap();
        if slots.contains_key(&id) {
            return Err(format!("{} is already loaded", manifest.name));
        }
        let now = Instant::now();
        let next_tick = manifest.every().map(|_| now + self.inner.timing.first_tick);
        let slot = Slot {
            manifest,
            dir: dir.to_path_buf(),
            dev,
            enabled,
            state: if enabled { State::Stopped } else { State::Disabled },
            error: String::new(),
            proc: None,
            gen: 0,
            stopping: false,
            crashes: VecDeque::new(),
            retry_at: None,
            last_used: now,
            process_seen: None,
            next_tick,
            ticking: false,
            logs: VecDeque::new(),
        };
        let info = slot.info();
        slots.insert(id.clone(), slot);
        drop(slots);
        self.inner.sink.changed(&info);
        Ok(id)
    }

    /// Stops the module (waiting for it to exit) and forgets it.
    pub fn unload(&self, id: &str) {
        self.stop_and_wait(id);
        self.inner.slots.lock().unwrap().remove(id);
        clear_activities(&self.inner, id);
    }

    pub fn set_enabled(&self, id: &str, on: bool) -> Result<(), String> {
        {
            let mut slots = self.inner.slots.lock().unwrap();
            let slot = slots.get_mut(id).ok_or("No such module")?;
            slot.enabled = on;
            if on {
                if slot.state == State::Disabled {
                    slot.state = State::Stopped;
                }
                slot.crashes.clear();
                slot.error.clear();
            }
        }
        if !on {
            self.stop_and_wait(id);
            let mut slots = self.inner.slots.lock().unwrap();
            if let Some(slot) = slots.get_mut(id) {
                slot.state = State::Disabled;
            }
        }
        changed(&self.inner, id);
        Ok(())
    }

    /// Clears the crash history and starts the module again (if its run mode wants it running).
    pub fn restart(&self, id: &str) -> Result<(), String> {
        self.stop_and_wait(id);
        let mut slots = self.inner.slots.lock().unwrap();
        let slot = slots.get_mut(id).ok_or("No such module")?;
        slot.crashes.clear();
        slot.error.clear();
        slot.retry_at = None;
        if slot.enabled {
            slot.state = State::Stopped;
        }
        drop(slots);
        changed(&self.inner, id);
        Ok(())
    }

    /// A UI call (`mod_call`). Starts `on-demand` and `scheduled` modules as needed.
    pub fn call(&self, id: &str, method_name: &str, params: Value) -> Result<Value, RpcError> {
        if !method::is_ui_call(method_name) {
            return Err(RpcError::unknown_method(method_name));
        }
        let proc = ensure_running(&self.inner, id, true)?;
        proc.busy.fetch_add(1, Ordering::Relaxed);
        touch(&self.inner, id);
        let r = proc.request(method_name, params, self.inner.timing.call);
        proc.busy.fetch_sub(1, Ordering::Relaxed);
        touch(&self.inner, id);
        r
    }

    /// A topic the base publishes (`island.geometry`, `fullscreen`, `session`, `power`, `theme`).
    pub fn publish(&self, topic: &str, data: Value) {
        deliver(&self.inner, None, topic, data);
    }

    /// The processes running now (image names), for `while-process` modules.
    pub fn set_processes(&self, names: impl IntoIterator<Item = String>) {
        *self.inner.processes.lock().unwrap() = names.into_iter().map(|n| n.to_ascii_lowercase()).collect();
    }

    /// Process names `while-process` modules wait for (so the base only watches when it must).
    pub fn watched_processes(&self) -> Vec<String> {
        let slots = self.inner.slots.lock().unwrap();
        slots.values().filter(|s| s.enabled && s.mode() == Some(RunMode::WhileProcess)).flat_map(|s| s.manifest.backend.as_ref().map(|b| b.process.clone()).unwrap_or_default()).collect()
    }

    pub fn set_env(&self, locale: &str, theme: Theme) {
        *self.inner.env.lock().unwrap() = (locale.into(), theme);
    }

    pub fn list(&self) -> Vec<ModuleInfo> {
        let slots = self.inner.slots.lock().unwrap();
        let mut v: Vec<_> = slots.values().map(Slot::info).collect();
        v.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        v
    }

    pub fn info(&self, id: &str) -> Option<ModuleInfo> {
        self.inner.slots.lock().unwrap().get(id).map(Slot::info)
    }

    pub fn logs(&self, id: &str) -> Vec<String> {
        self.inner.slots.lock().unwrap().get(id).map(|s| s.logs.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn activities(&self) -> Vec<ActivityOf> {
        sorted_activities(&self.inner)
    }

    /// Asks every backend to stop, waits up to the shutdown timeout, then kills what's left.
    pub fn shutdown(&self) {
        self.inner.quit.store(true, Ordering::Relaxed);
        let ids: Vec<String> = self.inner.slots.lock().unwrap().keys().cloned().collect();
        let procs: Vec<Arc<Proc>> = ids.iter().filter_map(|id| request_stop(&self.inner, id)).collect();
        wait_exit(&procs, self.inner.timing.shutdown);
    }

    fn stop_and_wait(&self, id: &str) {
        if let Some(p) = request_stop(&self.inner, id) {
            wait_exit(&[p], self.inner.timing.shutdown);
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Without a job object (tests off Windows), nothing else would end the children.
        for slot in self.slots.get_mut().unwrap().values() {
            if let Some(p) = &slot.proc {
                p.kill();
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn changed(inner: &Inner, id: &str) {
    let info = inner.slots.lock().unwrap().get(id).map(Slot::info);
    if let Some(info) = info {
        inner.sink.changed(&info);
    }
}

fn touch(inner: &Inner, id: &str) {
    if let Some(s) = inner.slots.lock().unwrap().get_mut(id) {
        s.last_used = Instant::now();
    }
}

fn log(inner: &Inner, id: &str, level: &str, msg: &str) {
    if let Some(s) = inner.slots.lock().unwrap().get_mut(id) {
        s.push_log(format!("{} {level} {msg}", now_ms()));
    }
    inner.sink.log(id, level, msg);
}

/// The running process, starting it if allowed. `for_call` is a UI call (which may start
/// `on-demand` and `scheduled` modules; `always` and `while-process` ones run on their own terms).
fn ensure_running(inner: &Arc<Inner>, id: &str, for_call: bool) -> Result<Arc<Proc>, RpcError> {
    let stopped = |why: String| RpcError::new(code::STOPPED, why);
    let proc = {
        let mut slots = inner.slots.lock().unwrap();
        let slot = slots.get_mut(id).ok_or_else(|| stopped("That module isn't installed".into()))?;
        let name = slot.manifest.name.clone();
        let mode = slot.mode().ok_or_else(|| stopped(format!("{name} has no backend")))?;
        match slot.state {
            State::Disabled => return Err(stopped(format!("{name} is turned off"))),
            State::Failed => return Err(stopped(format!("{name} stopped after crashing; restart it in Modules"))),
            State::Backoff if slot.retry_at.is_some_and(|t| t > Instant::now()) => return Err(stopped(format!("{name} is restarting"))),
            State::Running | State::Starting => slot.proc.clone().expect("a running slot has a process"),
            _ => {
                if for_call && mode == RunMode::WhileProcess {
                    let procs = slot.manifest.backend.as_ref().map(|b| b.process.join(" or ")).unwrap_or_default();
                    return Err(stopped(format!("{name} works while {procs} is running")));
                }
                spawn(inner, slot).map_err(|e| {
                    slot.state = State::Failed;
                    slot.error = e.clone();
                    stopped(e)
                })?
            }
        }
    };
    proc.wait_ready(inner.timing.init + Duration::from_secs(1)).map(|_| proc)
}

/// Starts the backend (called with the slots lock held; the init exchange runs on its own thread).
fn spawn(inner: &Arc<Inner>, slot: &mut Slot) -> Result<Arc<Proc>, String> {
    let backend = slot.manifest.backend.clone().ok_or("No backend")?;
    let id = slot.manifest.id.clone();
    let data_dir = inner.paths.data_root.join(&id);
    let cache_dir = inner.paths.cache_root.join(&id);
    let _ = std::fs::create_dir_all(&data_dir);
    let _ = std::fs::create_dir_all(&cache_dir);
    let mut cmd = Command::new(slot.dir.join(&backend.exe));
    cmd.current_dir(&slot.dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window, ever (the exe is also built with windows_subsystem = "windows").
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start {}: {e}", slot.manifest.name))?;
    if let Some(job) = &inner.job {
        job.assign(&child);
    }
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    slot.gen += 1;
    let gen = slot.gen;
    let proc = Arc::new(Proc {
        pid: child.id(),
        gen,
        stdin: Mutex::new(stdin),
        child: Mutex::new(child),
        pending: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
        ready: (Mutex::new(None), Condvar::new()),
        busy: AtomicUsize::new(0),
    });
    slot.proc = Some(proc.clone());
    slot.state = State::Starting;
    slot.stopping = false;
    slot.last_used = Instant::now();
    slot.push_log(format!("{} info started (pid {})", now_ms(), proc.pid));

    // stdout: the protocol. At its end the process is gone.
    let (i, p, mid) = (inner.clone(), proc.clone(), id.clone());
    std::thread::Builder::new()
        .name(format!("mod-{id}"))
        .spawn(move || {
            for frame in FrameReader::new(BufReader::new(stdout)) {
                match frame {
                    Ok(m) => handle(&i, &mid, &p, m),
                    Err(e) => log(&i, &mid, "warn", &format!("bad message: {e}")),
                }
            }
            // stdout closed: normally the process is gone. One that closed it but lives on is useless,
            // so it gets the shutdown timeout and is then killed. The child lock is never held while
            // waiting, so a stop or a kill from elsewhere always gets through.
            let until = Instant::now() + i.timing.shutdown;
            let code = loop {
                let polled = p.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait();
                match polled {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
                    Ok(None) => p.kill(),
                    Err(_) => break None,
                }
            };
            exited(&i, &mid, &p, code);
        })
        .map_err(|e| e.to_string())?;
    // stderr: the module's own log.
    let (i, mid) = (inner.clone(), id.clone());
    std::thread::Builder::new()
        .name(format!("mod-{id}-log"))
        .spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    log(&i, &mid, "info", &line);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    // island.init, then the last value of every topic it subscribes to.
    let (env_locale, env_theme) = inner.env.lock().unwrap().clone();
    let settings = std::fs::read(data_dir.join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    let init = Init {
        api: island_module_proto::API,
        id: id.clone(),
        base_version: inner.base_version.clone(),
        data_dir: data_dir.to_string_lossy().into_owned(),
        cache_dir: cache_dir.to_string_lossy().into_owned(),
        legacy_dir: inner.paths.legacy_dir.to_string_lossy().into_owned(),
        legacy: slot.manifest.legacy.clone(),
        locale: env_locale,
        theme: env_theme,
        settings,
        hosts: slot.manifest.hosts.clone(),
    };
    let subscribes = slot.manifest.subscribes.clone();
    let (i, p) = (inner.clone(), proc.clone());
    std::thread::spawn(move || {
        // A module that can't finish init is stopped, as far as callers are concerned.
        let r = p.request(method::INIT, &init, i.timing.init).map(|_| ()).map_err(|e| RpcError::new(code::STOPPED, format!("The module didn't start: {}", e.message)));
        match &r {
            Ok(()) => {
                let last: Vec<(String, Value)> = {
                    let topics = i.topics.lock().unwrap();
                    subscribes.iter().filter_map(|t| topics.get(t).map(|v| (t.clone(), v.clone()))).collect()
                };
                for (topic, data) in last {
                    p.send(&Message::notification(method::EVENT, TopicData { topic, data }));
                }
                let mut slots = i.slots.lock().unwrap();
                if let Some(s) = slots.get_mut(&id) {
                    if s.gen == p.gen && s.state == State::Starting {
                        s.state = State::Running;
                    }
                }
            }
            Err(e) => {
                log(&i, &id, "error", &format!("init failed: {}", e.message));
                // A missed init counts as a crash.
                p.kill();
            }
        }
        p.set_ready(r);
        changed(&i, &id);
    });
    Ok(proc)
}

/// A backend's process ended.
fn exited(inner: &Inner, id: &str, p: &Proc, code_: Option<i32>) {
    // Whoever was waiting on it gets an answer now instead of a timeout.
    let waiting: Vec<_> = p.pending.lock().unwrap().drain().collect();
    for (_, tx) in waiting {
        let _ = tx.send(Err(RpcError::new(code::STOPPED, "The module stopped")));
    }
    p.set_ready(Err(RpcError::new(code::STOPPED, "The module stopped")));
    {
        let mut slots = inner.slots.lock().unwrap();
        let Some(slot) = slots.get_mut(id) else { return };
        if slot.gen != p.gen {
            return;
        }
        slot.proc = None;
        let now = Instant::now();
        if slot.stopping {
            slot.stopping = false;
            slot.state = if slot.enabled { State::Stopped } else { State::Disabled };
            slot.push_log(format!("{} info stopped", now_ms()));
        } else {
            slot.crashes.push_back(now);
            while slot.crashes.front().is_some_and(|t| now.duration_since(*t) > inner.timing.crash_window) {
                slot.crashes.pop_front();
            }
            let n = slot.crashes.len();
            slot.error = format!("Crashed{}", code_.map(|c| format!(" (exit code {c})")).unwrap_or_default());
            if n >= 3 {
                slot.state = State::Failed;
                slot.error = format!("Crashed {n} times in {} minutes", inner.timing.crash_window.as_secs() / 60);
                slot.retry_at = None;
            } else {
                slot.state = State::Backoff;
                slot.retry_at = Some(now + inner.timing.backoff[n - 1]);
            }
            let line = format!("{} error {}", now_ms(), slot.error);
            slot.push_log(line);
        }
    }
    let err = inner.slots.lock().unwrap().get(id).filter(|s| s.state == State::Backoff || s.state == State::Failed).map(|s| s.error.clone());
    if let Some(e) = err {
        inner.sink.log(id, "error", &e);
    }
    clear_activities(inner, id);
    changed(inner, id);
}

/// Sends `island.shutdown` (the process exits on its own, or is killed after the timeout).
fn request_stop(inner: &Inner, id: &str) -> Option<Arc<Proc>> {
    let proc = {
        let mut slots = inner.slots.lock().unwrap();
        let slot = slots.get_mut(id)?;
        let p = slot.proc.clone()?;
        slot.stopping = true;
        p
    };
    proc.send(&Message::notification(method::SHUTDOWN, json!({})));
    Some(proc)
}

fn wait_exit(procs: &[Arc<Proc>], timeout: Duration) {
    let until = Instant::now() + timeout;
    for p in procs {
        loop {
            let done = p.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait().map(|s| s.is_some()).unwrap_or(true);
            if done {
                break;
            }
            if Instant::now() >= until {
                p.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// What a backend sends.
fn handle(inner: &Inner, id: &str, proc: &Proc, msg: Message) {
    match msg {
        Message::Response { id: rid, result } => {
            if let Some(tx) = proc.pending.lock().unwrap().remove(&rid) {
                let _ = tx.send(result);
            }
        }
        Message::Request { id: rid, method: m, .. } => {
            proc.send(&Message::err(rid, RpcError::unknown_method(&m)));
        }
        Message::Notification { method: m, params } => match m.as_str() {
            method::EMIT => match serde_json::from_value::<Emit>(params) {
                Ok(e) if valid_event_name(&e.name) => inner.sink.emit(id, &e.name, e.data),
                _ => log(inner, id, "warn", "host.emit needs a name of letters, digits, - _ / and :"),
            },
            method::NOTIFY => match serde_json::from_value::<Notify>(params) {
                Ok(n) => inner.sink.notify(id, n),
                Err(e) => log(inner, id, "warn", &format!("bad host.notify: {e}")),
            },
            method::ACTIVITY => match serde_json::from_value::<Activity>(params) {
                Ok(a) => set_activity(inner, id, a),
                Err(e) => log(inner, id, "warn", &format!("bad host.activity: {e}")),
            },
            method::CALENDAR => match serde_json::from_value::<CalendarPut>(params) {
                Ok(c) => {
                    let declared = inner.slots.lock().unwrap().get(id).is_some_and(|s| s.manifest.contributes.calendar.iter().any(|cal| cal.source == c.source));
                    if declared {
                        inner.sink.calendar(id, c);
                    } else {
                        log(inner, id, "warn", &format!("calendar source {} isn't in its manifest", c.source));
                    }
                }
                Err(e) => log(inner, id, "warn", &format!("bad host.calendar: {e}")),
            },
            method::PUBLISH => match serde_json::from_value::<TopicData>(params) {
                Ok(t) => {
                    let declared = inner.slots.lock().unwrap().get(id).is_some_and(|s| s.manifest.publishes.contains(&t.topic));
                    if declared {
                        deliver(inner, Some(id), &t.topic, t.data);
                    } else {
                        log(inner, id, "warn", &format!("topic {} rejected: it isn't in the manifest's publishes", t.topic));
                    }
                }
                Err(e) => log(inner, id, "warn", &format!("bad host.publish: {e}")),
            },
            method::LOG => {
                if let Ok(l) = serde_json::from_value::<Log>(params) {
                    let level = if matches!(l.level.as_str(), "error" | "warn" | "info") { l.level } else { "info".into() };
                    log(inner, id, &level, &l.msg);
                }
            }
            other => log(inner, id, "warn", &format!("unknown message {other}")),
        },
    }
}

fn valid_event_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_/:".contains(&b))
}

/// Stores the topic's last value and sends it to every running subscriber but the publisher.
fn deliver(inner: &Inner, from: Option<&str>, topic: &str, data: Value) {
    inner.topics.lock().unwrap().insert(topic.into(), data.clone());
    let targets: Vec<Arc<Proc>> = {
        let slots = inner.slots.lock().unwrap();
        slots
            .values()
            .filter(|s| Some(s.manifest.id.as_str()) != from && s.state == State::Running && s.manifest.subscribes.iter().any(|t| t == topic))
            .filter_map(|s| s.proc.clone())
            .collect()
    };
    let msg = Message::notification(method::EVENT, TopicData { topic: topic.into(), data });
    for p in targets {
        p.send(&msg);
    }
}

fn set_activity(inner: &Inner, id: &str, a: Activity) {
    {
        let mut list = inner.activities.lock().unwrap();
        list.retain(|x| !(x.module == id && x.activity.id == a.id));
        if !a.clear {
            list.push(ActivityOf { module: id.into(), activity: a, seq: inner.seq.fetch_add(1, Ordering::Relaxed) });
        }
    }
    inner.sink.activities(sorted_activities(inner));
}

fn clear_activities(inner: &Inner, id: &str) {
    let removed = {
        let mut list = inner.activities.lock().unwrap();
        let before = list.len();
        list.retain(|x| x.module != id);
        before != list.len()
    };
    if removed {
        inner.sink.activities(sorted_activities(inner));
    }
}

fn sorted_activities(inner: &Inner) -> Vec<ActivityOf> {
    let mut v = inner.activities.lock().unwrap().clone();
    v.sort_by(|a, b| b.activity.effective_priority().cmp(&a.activity.effective_priority()).then(b.seq.cmp(&a.seq)));
    v
}

/// The supervisor's clock: run modes, restarts, ticks and idle stops.
fn run_loop(weak: Weak<Inner>) {
    loop {
        let Some(inner) = weak.upgrade() else { return };
        if inner.quit.load(Ordering::Relaxed) {
            return;
        }
        let poll = inner.timing.poll;
        step(&inner);
        drop(inner);
        std::thread::sleep(poll);
    }
}

enum Action {
    Start(String),
    Stop(String),
    Tick(String),
}

fn step(inner: &Arc<Inner>) {
    let now = Instant::now();
    let running_procs = inner.processes.lock().unwrap().clone();
    let mut actions = Vec::new();
    {
        let mut slots = inner.slots.lock().unwrap();
        for slot in slots.values_mut() {
            let Some(mode) = slot.mode() else { continue };
            let id = slot.manifest.id.clone();
            if !slot.enabled || slot.state == State::Failed {
                continue;
            }
            let running = matches!(slot.state, State::Running | State::Starting);
            let busy = slot.proc.as_ref().map_or(0, |p| p.busy.load(Ordering::Relaxed));
            if slot.state == State::Backoff && slot.retry_at.is_some_and(|t| t <= now) {
                slot.retry_at = None;
                slot.state = State::Stopped;
            }
            let stopped = slot.state == State::Stopped;
            match mode {
                RunMode::Always => {
                    if stopped {
                        actions.push(Action::Start(id));
                    }
                }
                RunMode::WhileProcess => {
                    let wanted = slot.manifest.backend.as_ref().map(|b| b.process.iter().any(|p| running_procs.contains(&p.to_ascii_lowercase()))).unwrap_or(false);
                    if wanted {
                        slot.process_seen = Some(now);
                        if stopped {
                            actions.push(Action::Start(id));
                        }
                    } else if running && !slot.stopping && slot.process_seen.is_none_or(|t| now.duration_since(t) > inner.timing.process_grace) {
                        actions.push(Action::Stop(id));
                    }
                }
                RunMode::Scheduled => {
                    if !slot.ticking && slot.next_tick.is_some_and(|t| t <= now) && slot.state != State::Backoff {
                        slot.ticking = true;
                        slot.next_tick = slot.manifest.every().map(|e| now + e);
                        actions.push(Action::Tick(id));
                    } else if running && !slot.ticking && busy == 0 && !slot.stopping && now.duration_since(slot.last_used) > inner.timing.scheduled_idle {
                        actions.push(Action::Stop(id));
                    }
                }
                RunMode::OnDemand => {
                    if running && busy == 0 && !slot.stopping && now.duration_since(slot.last_used) > slot.manifest.idle() {
                        actions.push(Action::Stop(id));
                    }
                }
            }
        }
    }
    for a in actions {
        match a {
            Action::Start(id) => {
                let i = inner.clone();
                std::thread::spawn(move || {
                    let _ = ensure_running(&i, &id, false);
                });
            }
            Action::Stop(id) => {
                if let Some(p) = request_stop(inner, &id) {
                    let t = inner.timing.shutdown;
                    std::thread::spawn(move || wait_exit(&[p], t));
                }
            }
            Action::Tick(id) => {
                let i = inner.clone();
                std::thread::spawn(move || {
                    let r = ensure_running(&i, &id, false).and_then(|p| {
                        p.busy.fetch_add(1, Ordering::Relaxed);
                        let r = p.request(method::TICK, json!({}), i.timing.tick);
                        p.busy.fetch_sub(1, Ordering::Relaxed);
                        r
                    });
                    if let Err(e) = r {
                        log(&i, &id, "warn", &format!("tick failed: {}", e.message));
                    }
                    let mut slots = i.slots.lock().unwrap();
                    if let Some(s) = slots.get_mut(&id) {
                        s.ticking = false;
                        s.last_used = Instant::now();
                    }
                });
            }
        }
    }
}
