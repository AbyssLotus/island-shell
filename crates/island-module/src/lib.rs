//! The SDK for Island Shell module backends (docs/design/modules-handoff.md §4.6).
//!
//! A backend is a small windowless exe (`#![windows_subsystem = "windows"]`). It implements
//! [`Module`] and hands it to [`run`], which talks to the base over the process's stdin and stdout and
//! returns only to exit. Anything the module prints to stdout is thrown away, so it can't break the
//! protocol; stderr is the module's log, which the base keeps.
//!
//! ```no_run
//! use island_module::{json, Host, Module, Value, CallResult, RpcError};
//!
//! struct Hello;
//! impl Module for Hello {
//!     fn call(&self, _host: &Host, method: &str, _params: Value) -> CallResult {
//!         match method {
//!             "hello" => Ok(json!({ "text": "Hi from a module" })),
//!             _ => Err(RpcError::unknown_method(method)),
//!         }
//!     }
//! }
//!
//! fn main() {
//!     island_module::run(Hello);
//! }
//! ```
//!
//! For tests and debugging, run the exe in a terminal and type JSON-RPC lines, or call [`run_with`]
//! on any reader and writer.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

pub use island_module_proto as proto;
pub use island_module_proto::msg::{Activity, CalendarEvent, Init, Notify};
pub use island_module_proto::RpcError;
pub use serde_json::{json, Value};

use island_module_proto::msg::{CalendarPut, Emit, Images, Log, TopicData};
use island_module_proto::{method, write_message, FrameReader, Message};
use serde::Serialize;

pub mod http;
pub mod secrets;

pub type CallResult = Result<Value, RpcError>;

/// A module backend. Every method has a default except [`Module::call`].
pub trait Module: Send + Sync + 'static {
    /// Once, before anything else: load settings, take over legacy files.
    fn init(&self, _host: &Host, _init: &Init) -> Result<(), RpcError> {
        Ok(())
    }
    /// A call from the module's UI (`ctx.call(method, params)`). Each call runs on its own thread, so
    /// a slow one doesn't hold the others up.
    fn call(&self, host: &Host, method: &str, params: Value) -> CallResult;
    /// The scheduled wake-up (run mode `scheduled`).
    fn tick(&self, _host: &Host) -> Result<(), RpcError> {
        Ok(())
    }
    /// A subscribed topic changed. Events arrive in order on one thread: keep this quick.
    fn event(&self, _host: &Host, _topic: &str, _data: Value) {}
    /// The base is stopping the module. Save what needs saving; the process exits right after.
    fn shutdown(&self, _host: &Host) {}
}

/// The module's line back to the base. Cheap to clone and safe to use from any thread.
#[derive(Clone)]
pub struct Host {
    out: Arc<Mutex<Box<dyn Write + Send>>>,
    init: Arc<OnceLock<Init>>,
}

impl Host {
    fn new(out: Box<dyn Write + Send>) -> Self {
        Host { out: Arc::new(Mutex::new(out)), init: Arc::new(OnceLock::new()) }
    }

    fn send(&self, m: &Message) {
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = write_message(&mut **out, m) {
            eprintln!("[module] couldn't write to the base: {e}");
        }
    }

    fn notify_host(&self, name: &str, params: impl Serialize) {
        self.send(&Message::notification(name, params));
    }

    /// Sends `data` to the module's UI as the event `name` (the page sees `mod:<id>:<name>`).
    pub fn emit(&self, name: &str, data: impl Serialize) {
        self.notify_host(method::EMIT, Emit { name: name.into(), data: serde_json::to_value(data).unwrap_or(Value::Null) });
    }

    /// A notification on the island.
    pub fn notify(&self, n: Notify) {
        self.notify_host(method::NOTIFY, n);
    }

    /// Sets a live activity on the pill (now playing, in a call). Same id replaces it.
    pub fn activity(&self, a: Activity) {
        self.notify_host(method::ACTIVITY, a);
    }

    pub fn clear_activity(&self, id: &str) {
        self.notify_host(method::ACTIVITY, Activity { id: id.into(), clear: true, ..Default::default() });
    }

    /// Replaces all of `source`'s events in the calendar (the base keeps them while the module isn't running).
    pub fn calendar(&self, source: &str, events: Vec<CalendarEvent>) {
        self.notify_host(method::CALENDAR, CalendarPut { source: source.into(), events });
    }

    /// Shared state on a topic the manifest `publishes`.
    pub fn publish(&self, topic: &str, data: impl Serialize) {
        self.notify_host(method::PUBLISH, TopicData { topic: topic.into(), data: serde_json::to_value(data).unwrap_or(Value::Null) });
    }

    /// A line in island.log, as `[mod:<id>] …`.
    pub fn log(&self, level: &str, msg: impl Into<String>) {
        self.notify_host(method::LOG, Log { level: level.into(), msg: msg.into() });
    }
    /// Lets the module's UI show the images in these folders (see `ctx.imageUrl`). Replaces the
    /// last list; the base keeps it until the module is turned off.
    pub fn share_images(&self, dirs: &[std::path::PathBuf]) {
        let dirs: Vec<String> = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
        self.notify_host(method::IMAGES, Images { dirs });
    }
    pub fn info(&self, msg: impl Into<String>) {
        self.log("info", msg);
    }
    pub fn warn(&self, msg: impl Into<String>) {
        self.log("warn", msg);
    }
    pub fn error(&self, msg: impl Into<String>) {
        self.log("error", msg);
    }

    /// What the base said in `island.init` (empty before init).
    pub fn info_from_base(&self) -> Init {
        self.init.get().cloned().unwrap_or_default()
    }
    /// Settings, keys and anything else to keep: `%APPDATA%\league.seaweed.islandshell\modules\<id>\`.
    pub fn data_dir(&self) -> PathBuf {
        self.init.get().map(|i| PathBuf::from(&i.data_dir)).unwrap_or_default()
    }
    /// Images and big JSON that can be fetched again: `%LOCALAPPDATA%\…\modules-cache\<id>\`.
    pub fn cache_dir(&self) -> PathBuf {
        self.init.get().map(|i| PathBuf::from(&i.cache_dir)).unwrap_or_default()
    }
    /// Where 0.1.x kept its files.
    pub fn legacy_dir(&self) -> PathBuf {
        self.init.get().map(|i| PathBuf::from(&i.legacy_dir)).unwrap_or_default()
    }

    /// Takes over a 0.1.x file (`name`, relative to the legacy folder) into `to`: copies it, checks the
    /// copy, then deletes the original. Does nothing when `to` already exists or there's nothing to take.
    /// Returns whether it moved something.
    pub fn take_legacy(&self, name: &str, to: &std::path::Path) -> std::io::Result<bool> {
        take_legacy(&self.legacy_dir().join(name), to)
    }
}

fn take_legacy(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<bool> {
    if to.exists() || !from.is_file() {
        return Ok(false);
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = std::fs::read(from)?;
    let tmp = to.with_extension("legacy-tmp");
    std::fs::write(&tmp, &bytes)?;
    if std::fs::read(&tmp)? != bytes {
        let _ = std::fs::remove_file(&tmp);
        return Err(std::io::Error::other("the copy didn't match"));
    }
    std::fs::rename(&tmp, to)?;
    std::fs::remove_file(from)?;
    Ok(true)
}

/// Unix ms now.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// This PC's offset from UTC right now, in ms (local = UTC + offset), in whole quarter hours.
#[cfg(windows)]
pub fn local_offset_ms() -> i64 {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    let ms = |t: SYSTEMTIME| {
        let (y, m, d) = (t.wYear as i64, t.wMonth as i64, t.wDay as i64);
        let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
        let era = y2.div_euclid(400);
        let yoe = y2 - era * 400;
        let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * m2 + 2) / 5 + d - 1 - 719_468;
        ((days * 24 + t.wHour as i64) * 60 + t.wMinute as i64) * 60_000 + t.wSecond as i64 * 1000
    };
    let (local, utc) = unsafe { (GetLocalTime(), GetSystemTime()) };
    // The two clock reads can straddle a second.
    ((ms(local) - ms(utc)) as f64 / 900_000.0).round() as i64 * 900_000
}

#[cfg(not(windows))]
pub fn local_offset_ms() -> i64 {
    0
}

/// Runs the module on stdin and stdout until the base says `island.shutdown` or goes away, then exits.
pub fn run<M: Module>(module: M) -> ! {
    let out = take_stdout();
    let input = BufReader::new(std::io::stdin());
    run_with(module, input, out);
    std::process::exit(0)
}

/// The protocol's stdout, with the process's own stdout pointed at nothing, so stray prints can't
/// break the framing. The protocol pipes stop being inheritable, so an app the module starts can't
/// hold them open after the module is gone (the base would think it still running).
#[cfg(windows)]
fn take_stdout() -> Box<dyn Write + Send> {
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};
    use windows::Win32::Foundation::{SetHandleInformation, HANDLE_FLAGS, HANDLE_FLAG_INHERIT};
    use windows::Win32::System::Console::{GetStdHandle, SetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    unsafe {
        if let Ok(h) = GetStdHandle(STD_INPUT_HANDLE) {
            let _ = SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
        }
        match GetStdHandle(STD_OUTPUT_HANDLE) {
            Ok(h) if !h.is_invalid() && !h.0.is_null() => {
                let _ = SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
                let file = std::fs::File::from_raw_handle(h.0 as _);
                if let Ok(nul) = std::fs::OpenOptions::new().write(true).open("NUL") {
                    let _ = SetStdHandle(STD_OUTPUT_HANDLE, windows::Win32::Foundation::HANDLE(nul.into_raw_handle() as _));
                }
                Box::new(file)
            }
            _ => Box::new(std::io::stdout()),
        }
    }
}

#[cfg(not(windows))]
fn take_stdout() -> Box<dyn Write + Send> {
    Box::new(std::io::stdout())
}

/// The run loop on any reader and writer. Returns when the input ends or after `island.shutdown`.
pub fn run_with<M: Module>(module: M, input: impl BufRead, output: impl Write + Send + 'static) {
    let module = Arc::new(module);
    let host = Host::new(Box::new(output));
    let mut threads = Vec::new();
    for frame in FrameReader::new(input) {
        let msg = match frame {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[module] {e}");
                continue;
            }
        };
        match msg {
            Message::Request { id, method, params } => match method.as_str() {
                method::INIT => {
                    let reply = match serde_json::from_value::<Init>(params) {
                        Ok(init) => {
                            http::set_hosts(init.hosts.clone(), PathBuf::from(&init.data_dir));
                            let _ = host.init.set(init.clone());
                            module.init(&host, &init).map(|_| json!({}))
                        }
                        Err(e) => Err(RpcError::bad_params(e)),
                    };
                    host.send(&reply_for(id, reply));
                }
                method::TICK => {
                    let (m, h) = (module.clone(), host.clone());
                    threads.push(std::thread::spawn(move || h.send(&reply_for(id, m.tick(&h).map(|_| json!({}))))));
                }
                name if method::is_ui_call(name) => {
                    let (m, h) = (module.clone(), host.clone());
                    threads.push(std::thread::spawn(move || {
                        // A panicking call answers with an error instead of hanging the UI.
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m.call(&h, &method, params)))
                            .unwrap_or_else(|_| Err(RpcError::failed(format!("{method} crashed"))));
                        h.send(&reply_for(id, result));
                    }));
                }
                other => host.send(&Message::err(id, RpcError::unknown_method(other))),
            },
            Message::Notification { method, params } => match method.as_str() {
                method::EVENT => {
                    if let Ok(ev) = serde_json::from_value::<TopicData>(params) {
                        module.event(&host, &ev.topic, ev.data);
                    }
                }
                method::SHUTDOWN => {
                    module.shutdown(&host);
                    return;
                }
                _ => {}
            },
            // The base answers nothing the module asks: every host.* message is a notification.
            Message::Response { .. } => {}
        }
        threads.retain(|t| !t.is_finished());
    }
    // The base went away: let calls in flight finish writing, then stop.
    for t in threads {
        let _ = t.join();
    }
    module.shutdown(&host);
}

fn reply_for(id: u64, result: CallResult) -> Message {
    match result {
        Ok(v) => Message::ok(id, v),
        Err(e) => Message::err(id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use island_module_proto::code;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Echo {
        events: Mutex<Vec<String>>,
        ticks: AtomicUsize,
        stopped: AtomicUsize,
    }

    impl Module for Echo {
        fn init(&self, host: &Host, init: &Init) -> Result<(), RpcError> {
            host.info(format!("hello {}", init.id));
            Ok(())
        }
        fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
            match method {
                "echo" => Ok(params),
                "emit" => {
                    host.emit("ping", json!({ "n": 1 }));
                    Ok(Value::Null)
                }
                "boom" => panic!("boom"),
                "data" => Ok(json!(host.data_dir())),
                _ => Err(RpcError::unknown_method(method)),
            }
        }
        fn tick(&self, _host: &Host) -> Result<(), RpcError> {
            self.ticks.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn event(&self, _host: &Host, topic: &str, _data: Value) {
            self.events.lock().unwrap().push(topic.into());
        }
        fn shutdown(&self, _host: &Host) {
            self.stopped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Shared output buffer the test can read after the loop.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn session(lines: &[Message]) -> Vec<Message> {
        let mut input = Vec::new();
        for m in lines {
            write_message(&mut input, m).unwrap();
        }
        let sink = Sink::default();
        run_with(Echo::default(), Cursor::new(input), sink.clone());
        let bytes = sink.0.lock().unwrap().clone();
        FrameReader::new(Cursor::new(bytes)).map(Result::unwrap).collect()
    }

    fn init() -> Message {
        Message::request(1, method::INIT, Init { api: 1, id: "echo".into(), data_dir: "/data/echo".into(), ..Default::default() })
    }

    #[test]
    fn init_call_and_host_messages() {
        let out = session(&[init(), Message::request(2, "echo", json!({ "a": [1, 2] })), Message::request(3, "emit", json!({})), Message::request(4, "data", json!(null))]);
        assert!(out.contains(&Message::notification(method::LOG, Log { level: "info".into(), msg: "hello echo".into() })));
        assert!(out.contains(&Message::ok(1, json!({}))));
        assert!(out.contains(&Message::ok(2, json!({ "a": [1, 2] }))));
        assert!(out.contains(&Message::notification(method::EMIT, Emit { name: "ping".into(), data: json!({ "n": 1 }) })));
        assert!(out.contains(&Message::ok(3, Value::Null)));
        assert!(out.contains(&Message::ok(4, json!("/data/echo"))));
    }

    #[test]
    fn errors_and_panics_answer() {
        let out = session(&[init(), Message::request(2, "nope", json!({})), Message::request(3, "boom", json!({})), Message::request(4, "island.reboot", json!({}))]);
        let code_of = |id| out.iter().find_map(|m| match m {
            Message::Response { id: i, result: Err(e) } if *i == id => Some(e.code),
            _ => None,
        });
        assert_eq!(code_of(2), Some(code::UNKNOWN_METHOD));
        assert_eq!(code_of(3), Some(code::FAILED));
        assert_eq!(code_of(4), Some(code::UNKNOWN_METHOD));
    }

    #[test]
    fn ticks_events_and_shutdown() {
        let module = Arc::new(Echo::default());
        struct Shared(Arc<Echo>);
        impl Module for Shared {
            fn call(&self, h: &Host, m: &str, p: Value) -> CallResult {
                self.0.call(h, m, p)
            }
            fn tick(&self, h: &Host) -> Result<(), RpcError> {
                self.0.tick(h)
            }
            fn event(&self, h: &Host, t: &str, d: Value) {
                self.0.event(h, t, d)
            }
            fn shutdown(&self, h: &Host) {
                self.0.shutdown(h)
            }
        }
        let mut input = Vec::new();
        for m in [
            init(),
            Message::request(2, method::TICK, json!({})),
            Message::notification(method::EVENT, TopicData { topic: "presence".into(), data: json!({ "state": "away" }) }),
            Message::notification(method::SHUTDOWN, json!({})),
            // Nothing after shutdown is read.
            Message::request(9, "echo", json!(1)),
        ] {
            write_message(&mut input, &m).unwrap();
        }
        run_with(Shared(module.clone()), Cursor::new(input), Sink::default());
        assert_eq!(*module.events.lock().unwrap(), vec!["presence".to_string()]);
        assert_eq!(module.stopped.load(Ordering::Relaxed), 1);
        // The tick ran on its own thread; it had time to finish before shutdown was read, or not:
        // either way it ran at most once.
        assert!(module.ticks.load(Ordering::Relaxed) <= 1);
    }

    #[test]
    fn legacy_files_move_once() {
        let dir = std::env::temp_dir().join(format!("island-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("old")).unwrap();
        std::fs::write(dir.join("old/destiny.json"), b"{\"key\":\"x\"}").unwrap();
        let to = dir.join("new/destiny.json");
        assert!(take_legacy(&dir.join("old/destiny.json"), &to).unwrap());
        assert_eq!(std::fs::read(&to).unwrap(), b"{\"key\":\"x\"}");
        assert!(!dir.join("old/destiny.json").exists());
        // Nothing left to take, and an existing file is never overwritten.
        assert!(!take_legacy(&dir.join("old/destiny.json"), &to).unwrap());
        std::fs::write(dir.join("old/destiny.json"), b"older").unwrap();
        assert!(!take_legacy(&dir.join("old/destiny.json"), &to).unwrap());
        assert_eq!(std::fs::read(&to).unwrap(), b"{\"key\":\"x\"}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn end_of_input_shuts_down_once() {
        let module = Arc::new(Echo::default());
        struct Shared(Arc<Echo>);
        impl Module for Shared {
            fn call(&self, h: &Host, m: &str, p: Value) -> CallResult {
                self.0.call(h, m, p)
            }
            fn shutdown(&self, h: &Host) {
                self.0.shutdown(h)
            }
        }
        let mut input = Vec::new();
        write_message(&mut input, &init()).unwrap();
        input.extend_from_slice(b"garbage line\n");
        run_with(Shared(module.clone()), Cursor::new(input), Sink::default());
        assert_eq!(module.stopped.load(Ordering::Relaxed), 1);
    }
}
