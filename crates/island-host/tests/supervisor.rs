//! The supervisor against a real backend process (src/bin/echo-module.rs), with short timings.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use island_host::proto::code;
use island_host::proto::msg::{CalendarPut, Notify};
use island_host::{ActivityOf, ModuleInfo, Paths, Sink, State, Supervisor, Timing};
use serde_json::{json, Value};

#[derive(Default)]
struct Rec {
    emits: Mutex<Vec<(String, String, Value)>>,
    activities: Mutex<Vec<ActivityOf>>,
    logs: Mutex<Vec<(String, String, String)>>,
    calendars: Mutex<Vec<(String, CalendarPut)>>,
}

impl Sink for Rec {
    fn emit(&self, module: &str, name: &str, data: Value) {
        self.emits.lock().unwrap().push((module.into(), name.into(), data));
    }
    fn notify(&self, _: &str, _: Notify) {}
    fn activities(&self, list: Vec<ActivityOf>) {
        *self.activities.lock().unwrap() = list;
    }
    fn calendar(&self, module: &str, put: CalendarPut) {
        self.calendars.lock().unwrap().push((module.into(), put));
    }
    fn log(&self, module: &str, level: &str, msg: &str) {
        self.logs.lock().unwrap().push((module.into(), level.into(), msg.into()));
    }
    fn changed(&self, _: &ModuleInfo) {}
}

impl Rec {
    fn logged(&self, module: &str, needle: &str) -> bool {
        self.logs.lock().unwrap().iter().any(|(m, _, msg)| m == module && msg.contains(needle))
    }
}

fn timing() -> Timing {
    Timing {
        init: Duration::from_millis(1500),
        call: Duration::from_secs(5),
        tick: Duration::from_secs(5),
        shutdown: Duration::from_millis(800),
        backoff: [Duration::from_millis(100), Duration::from_millis(200), Duration::from_millis(300)],
        crash_window: Duration::from_secs(60),
        process_grace: Duration::from_millis(300),
        scheduled_idle: Duration::from_millis(400),
        first_tick: Duration::from_millis(100),
        poll: Duration::from_millis(50),
    }
}

/// A scratch folder under target/tmp (not the system temp folder), so one antivirus exclusion of the
/// build folder covers the echo backend's copies too.
fn root(name: &str) -> PathBuf {
    let r = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("island-host-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&r);
    std::fs::create_dir_all(&r).unwrap();
    r
}

const EXE: &str = if cfg!(windows) { "echo-module.exe" } else { "echo-module" };

/// A module folder with the echo backend and a manifest; `extra` is merged into module.json.
fn module(root: &Path, id: &str, extra: Value) -> PathBuf {
    let dir = root.join("modules").join(id);
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    // On Linux a copy can fail to start ("text file busy") while other tests fork; a link can't.
    #[cfg(unix)]
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_echo-module"), dir.join(EXE)).unwrap();
    #[cfg(not(unix))]
    std::fs::copy(env!("CARGO_BIN_EXE_echo-module"), dir.join(EXE)).unwrap();
    std::fs::write(dir.join("ui/icon.svg"), "<svg/>").unwrap();
    let mut m = json!({
        "id": id, "name": format!("Echo {id}"), "version": "1.0.0", "api": 1,
        "description": "A test module.", "icon": "ui/icon.svg",
        "backend": { "exe": EXE, "run": "on-demand" },
        "talksTo": []
    });
    for (k, v) in extra.as_object().unwrap() {
        if k == "backend" {
            for (bk, bv) in v.as_object().unwrap() {
                m["backend"][bk] = bv.clone();
            }
        } else {
            m[k] = v.clone();
        }
    }
    std::fs::write(dir.join("module.json"), serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    dir
}

fn host(root: &Path) -> (Supervisor, Arc<Rec>) {
    let rec = Arc::new(Rec::default());
    let paths = Paths { data_root: root.join("data"), cache_root: root.join("cache"), legacy_dir: root.join("legacy") };
    (Supervisor::new(paths, rec.clone(), "0.2.0", timing()), rec)
}

fn wait_for(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

fn state(sup: &Supervisor, id: &str) -> State {
    sup.info(id).unwrap().state
}

#[test]
fn call_round_trip() {
    let r = root("call");
    let (sup, rec) = host(&r);
    let id = sup.load(&module(&r, "echo-a", json!({})), true, true).unwrap();
    assert_eq!(state(&sup, &id), State::Stopped);
    assert_eq!(sup.call(&id, "echo", json!({ "hello": ["world", 1] })).unwrap(), json!({ "hello": ["world", 1] }));
    assert_eq!(state(&sup, &id), State::Running);
    // Stray stdout from a module doesn't break the protocol.
    assert_eq!(sup.call(&id, "print", Value::Null).unwrap(), json!("printed"));
    assert_eq!(sup.call(&id, "echo", json!(2)).unwrap(), json!(2));
    assert_eq!(sup.call(&id, "fail", Value::Null).unwrap_err().code, code::FAILED);
    assert_eq!(sup.call(&id, "nope", Value::Null).unwrap_err().code, code::UNKNOWN_METHOD);
    assert_eq!(sup.call(&id, "island.init", Value::Null).unwrap_err().code, code::UNKNOWN_METHOD);
    // host.log and stderr both reach the log.
    wait_for("logs", 5, || rec.logged(&id, "ready") && sup.logs(&id).iter().any(|l| l.contains("echo started for echo-a")));
    // Calls run side by side: a slow one doesn't hold a quick one up.
    let (s2, i2) = (sup.clone(), id.clone());
    let slow = std::thread::spawn(move || s2.call(&i2, "sleep", json!({ "ms": 1500 })));
    std::thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    assert_eq!(sup.call(&id, "echo", json!("quick")).unwrap(), json!("quick"));
    assert!(t.elapsed() < Duration::from_millis(1000));
    assert_eq!(slow.join().unwrap().unwrap(), json!("slept"));
    sup.shutdown();
    assert_eq!(state(&sup, &id), State::Stopped);
}

#[test]
fn crashes_back_off_then_stop() {
    let r = root("crash");
    let (sup, rec) = host(&r);
    let id = sup.load(&module(&r, "echo-b", json!({ "backend": { "run": "always" } })), true, true).unwrap();
    wait_for("always module to start", 5, || state(&sup, &id) == State::Running);
    let mut pid = sup.call(&id, "pid", Value::Null).unwrap();
    sup.call(&id, "activity", json!({ "id": "call", "kind": "call", "text": "In a call" })).unwrap();
    wait_for("activity", 5, || rec.activities.lock().unwrap().len() == 1);
    for n in 1..=2 {
        assert_eq!(sup.call(&id, "crash", Value::Null).unwrap_err().code, code::STOPPED);
        // Its activities go with it. (The crash is counted first, so the Running below is the
        // restarted process, not the one still on its way out.)
        wait_for("crash counted", 5, || sup.info(&id).unwrap().crashes == n);
        wait_for("activities cleared", 5, || rec.activities.lock().unwrap().is_empty());
        wait_for("restart after backoff", 5, || state(&sup, &id) == State::Running);
        let new_pid = sup.call(&id, "pid", Value::Null).unwrap();
        assert_ne!(new_pid, pid, "crash {n} restarts the process");
        pid = new_pid;
    }
    let _ = sup.call(&id, "crash", Value::Null);
    wait_for("failed after three crashes", 5, || state(&sup, &id) == State::Failed);
    assert!(sup.info(&id).unwrap().error.contains("3 times"));
    assert_eq!(sup.call(&id, "pid", Value::Null).unwrap_err().code, code::STOPPED);
    // Restart clears it.
    sup.restart(&id).unwrap();
    wait_for("running after restart", 5, || state(&sup, &id) == State::Running);
    assert_eq!(sup.info(&id).unwrap().crashes, 0);
    sup.shutdown();
}

#[test]
fn idle_exit_and_restart_on_call() {
    let r = root("idle");
    let (sup, _) = host(&r);
    let id = sup.load(&module(&r, "echo-c", json!({ "backend": { "idle": "1s" } })), true, true).unwrap();
    let first = sup.call(&id, "pid", Value::Null).unwrap();
    wait_for("idle exit", 6, || state(&sup, &id) == State::Stopped && sup.info(&id).unwrap().pid.is_none());
    // It wasn't a crash, and the next call starts it again.
    assert_eq!(sup.info(&id).unwrap().crashes, 0);
    assert_ne!(sup.call(&id, "pid", Value::Null).unwrap(), first);
    sup.shutdown();
}

#[test]
fn topics_follow_the_manifests() {
    let r = root("topics");
    let (sup, rec) = host(&r);
    let a = sup.load(&module(&r, "music", json!({ "backend": { "run": "always" }, "publishes": ["media.state"] })), true, true).unwrap();
    wait_for("publisher", 5, || state(&sup, &a) == State::Running);
    sup.call(&a, "publish", json!({ "topic": "media.state", "data": { "playing": true } })).unwrap();
    // A topic it didn't declare is refused.
    sup.call(&a, "publish", json!({ "topic": "presence", "data": { "state": "away" } })).unwrap();
    wait_for("rejection logged", 5, || rec.logged(&a, "topic presence rejected"));
    // A subscriber that starts later gets the last value straight away, then live changes.
    let b = sup.load(&module(&r, "discord", json!({ "subscribes": ["media.state", "presence", "fullscreen"] })), true, true).unwrap();
    sup.call(&b, "echo", Value::Null).unwrap();
    wait_for("last value", 5, || sup.call(&b, "events", Value::Null).unwrap().as_array().unwrap().len() == 1);
    sup.call(&a, "publish", json!({ "topic": "media.state", "data": { "playing": false } })).unwrap();
    sup.publish("fullscreen", json!({ "active": true }));
    sup.publish("theme", json!({ "dark": true }));
    wait_for("live events", 5, || sup.call(&b, "events", Value::Null).unwrap().as_array().unwrap().len() == 3);
    let events = sup.call(&b, "events", Value::Null).unwrap();
    assert_eq!(
        events,
        json!([
            { "topic": "media.state", "data": { "playing": true } },
            { "topic": "media.state", "data": { "playing": false } },
            { "topic": "fullscreen", "data": { "active": true } }
        ])
    );
    sup.shutdown();
}

#[test]
fn ui_events_activities_and_calendar() {
    let r = root("route");
    let (sup, rec) = host(&r);
    let id = sup.load(&module(&r, "echo-d", json!({ "contributes": { "calendar": [{ "source": "echo-d", "name": "Echo", "color": "#f5c26b" }] } })), true, true).unwrap();
    sup.call(&id, "emit", json!({ "name": "status", "data": { "n": 1 } })).unwrap();
    sup.call(&id, "emit", json!({ "name": "bad name!", "data": 1 })).unwrap();
    sup.call(&id, "activity", json!({ "id": "song", "kind": "media", "text": "Glass Harbor" })).unwrap();
    sup.call(&id, "activity", json!({ "id": "call", "kind": "call", "text": "In a call" })).unwrap();
    wait_for("routing", 5, || rec.emits.lock().unwrap().len() == 1 && rec.activities.lock().unwrap().len() == 2 && rec.logged(&id, "host.emit needs a name"));
    assert_eq!(rec.emits.lock().unwrap()[0], ("echo-d".into(), "status".into(), json!({ "n": 1 })));
    // Highest priority first: the call beats the song.
    let ids: Vec<_> = rec.activities.lock().unwrap().iter().map(|a| a.activity.id.clone()).collect();
    assert_eq!(ids, ["call", "song"]);
    sup.call(&id, "activity", json!({ "id": "call", "clear": true })).unwrap();
    wait_for("cleared", 5, || rec.activities.lock().unwrap().len() == 1);
    // Shared image folders: real absolute folders only, kept as their canonical paths.
    let art = r.join("art");
    std::fs::create_dir_all(&art).unwrap();
    sup.call(&id, "share", json!([art.to_string_lossy(), "relative/dir", r.join("missing").to_string_lossy()])).unwrap();
    wait_for("shared", 5, || sup.info(&id).unwrap().image_dirs == vec![art.canonicalize().unwrap()]);
    sup.shutdown();
}

#[test]
fn while_process_follows_the_process() {
    let r = root("process");
    let (sup, _) = host(&r);
    let id = sup.load(&module(&r, "echo-e", json!({ "backend": { "run": "while-process", "process": ["Fake.exe"], "idle": "1s" } })), true, true).unwrap();
    assert_eq!(sup.watched_processes(), vec!["Fake.exe".to_string()]);
    sup.set_processes(["fake.EXE".to_string(), "other.exe".to_string()]);
    wait_for("start with its process", 5, || state(&sup, &id) == State::Running);
    sup.set_processes(["other.exe".to_string()]);
    wait_for("stop after the grace period", 5, || state(&sup, &id) == State::Stopped);
    assert_eq!(sup.info(&id).unwrap().crashes, 0);
    // Without its process, a UI call starts it (to open the app, or set it up), and it stays only
    // for its idle time.
    assert_eq!(sup.call(&id, "echo", json!(1)).unwrap(), json!(1));
    assert_eq!(state(&sup, &id), State::Running);
    wait_for("stop once idle", 5, || state(&sup, &id) == State::Stopped);
    sup.shutdown();
}

#[test]
fn scheduled_ticks_then_exits() {
    let r = root("tick");
    let (sup, _) = host(&r);
    let id = sup.load(&module(&r, "echo-f", json!({ "backend": { "run": "scheduled", "every": "1m", "idle": "2s" } })), true, true).unwrap();
    wait_for("first tick", 5, || state(&sup, &id) == State::Running);
    wait_for("exit after the tick", 5, || state(&sup, &id) == State::Stopped);
    // A UI call starts it again; the tick count lives in the process, so it's back to zero.
    assert_eq!(sup.call(&id, "ticks", Value::Null).unwrap(), json!(0));
    // After a call it keeps the manifest's idle time (2 s), not the short one after a tick.
    std::thread::sleep(Duration::from_millis(1000));
    assert_eq!(state(&sup, &id), State::Running);
    wait_for("exit once idle", 5, || state(&sup, &id) == State::Stopped);
    sup.shutdown();
}

#[test]
fn disable_and_failed_init() {
    let r = root("disable");
    let (sup, _) = host(&r);
    let id = sup.load(&module(&r, "echo-g", json!({})), true, true).unwrap();
    sup.call(&id, "echo", Value::Null).unwrap();
    sup.set_enabled(&id, false).unwrap();
    assert_eq!(state(&sup, &id), State::Disabled);
    assert!(sup.call(&id, "echo", Value::Null).unwrap_err().message.contains("turned off"));
    sup.set_enabled(&id, true).unwrap();
    assert_eq!(sup.call(&id, "echo", json!(5)).unwrap(), json!(5));
    // It works in its data folder, so nothing it opens holds its code folder (updates move that).
    let cwd = PathBuf::from(sup.call(&id, "cwd", Value::Null).unwrap().as_str().unwrap());
    assert_eq!(cwd.canonicalize().unwrap(), r.join("data").join(&id).canonicalize().unwrap());
    // An init that misses its timeout counts as a crash.
    let slow = module(&r, "echo-h", json!({}));
    std::fs::write(slow.join("slow-init"), "4000").unwrap();
    let h = sup.load(&slow, true, true).unwrap();
    assert_eq!(sup.call(&h, "echo", Value::Null).unwrap_err().code, code::STOPPED);
    wait_for("crash counted", 5, || sup.info(&h).unwrap().crashes == 1);
    // Loading the same id twice, or a folder without its backend, is refused.
    assert!(sup.load(&slow, true, true).unwrap_err().contains("already loaded"));
    let broken = module(&r, "echo-i", json!({}));
    std::fs::remove_file(broken.join(EXE)).unwrap();
    assert!(sup.load(&broken, true, true).unwrap_err().contains("missing"));
    sup.shutdown();
}

/// Killing the island (here a stand-in host process) ends its modules: the job object.
#[cfg(windows)]
#[test]
fn killing_the_base_leaves_no_module_behind() {
    use std::io::BufRead;
    let r = root("orphan");
    let dir = module(&r, "echo-j", json!({ "backend": { "run": "always" } }));
    let mut base = std::process::Command::new(env!("CARGO_BIN_EXE_echo-module")).arg("--host").arg(&dir).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    std::io::BufReader::new(base.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let pid: u32 = line.trim().parse().expect("module pid");
    assert!(alive(pid));
    base.kill().unwrap();
    let _ = base.wait();
    wait_for("module to end with the base", 10, || !alive(pid));
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
}
