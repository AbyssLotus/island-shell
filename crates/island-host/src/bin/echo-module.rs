//! A module backend for the host's tests: it does whatever its calls ask, including crashing.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::sync::Mutex;

use island_module::{json, Activity, CallResult, Host, Init, Module, RpcError, Value};

#[derive(Default)]
struct Echo {
    events: Mutex<Vec<Value>>,
    ticks: Mutex<u32>,
}

impl Module for Echo {
    fn init(&self, host: &Host, init: &Init) -> Result<(), RpcError> {
        // A file named slow-init in the module folder holds how many ms init should take (the host
        // treats a missed init as a crash).
        if let Some(ms) = std::fs::read_to_string("slow-init").ok().and_then(|v| v.trim().parse().ok()) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        eprintln!("echo started for {}", init.id);
        host.info("ready");
        Ok(())
    }

    fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
        match method {
            "echo" => Ok(params),
            "pid" => Ok(json!(std::process::id())),
            "crash" => std::process::exit(3),
            "sleep" => {
                std::thread::sleep(std::time::Duration::from_millis(params["ms"].as_u64().unwrap_or(100)));
                Ok(json!("slept"))
            }
            "emit" => {
                host.emit(params["name"].as_str().unwrap_or("ping"), &params["data"]);
                Ok(Value::Null)
            }
            "publish" => {
                host.publish(params["topic"].as_str().unwrap_or(""), &params["data"]);
                Ok(Value::Null)
            }
            "activity" => {
                host.activity(serde_json::from_value::<Activity>(params).map_err(RpcError::bad_params)?);
                Ok(Value::Null)
            }
            "events" => Ok(json!(*self.events.lock().unwrap())),
            "ticks" => Ok(json!(*self.ticks.lock().unwrap())),
            "print" => {
                // Stray stdout must not break the protocol.
                println!("this goes nowhere");
                Ok(json!("printed"))
            }
            "fail" => Err(RpcError::failed("asked to fail")),
            _ => Err(RpcError::unknown_method(method)),
        }
    }

    fn tick(&self, _host: &Host) -> Result<(), RpcError> {
        *self.ticks.lock().unwrap() += 1;
        Ok(())
    }

    fn event(&self, _host: &Host, topic: &str, data: Value) {
        self.events.lock().unwrap().push(json!({ "topic": topic, "data": data }));
    }
}

/// `echo-module --host <module dir>`: a stand-in island that runs one module and prints its pid,
/// so a test can kill this process and check the module went with it (the job object, on Windows).
fn host(dir: &str) {
    use island_host::{ActivityOf, ModuleInfo, Paths, Sink, Supervisor, Timing};
    use island_module::Notify;
    struct Quiet;
    impl Sink for Quiet {
        fn emit(&self, _: &str, _: &str, _: Value) {}
        fn notify(&self, _: &str, _: Notify) {}
        fn activities(&self, _: Vec<ActivityOf>) {}
        fn calendar(&self, _: &str, _: island_host::proto::msg::CalendarPut) {}
        fn log(&self, _: &str, _: &str, _: &str) {}
        fn changed(&self, _: &ModuleInfo) {}
    }
    let tmp = std::env::temp_dir().join(format!("echo-host-{}", std::process::id()));
    let paths = Paths { data_root: tmp.join("data"), cache_root: tmp.join("cache"), legacy_dir: tmp.join("legacy") };
    let sup = Supervisor::new(paths, std::sync::Arc::new(Quiet), "0.0.0", Timing::default());
    let id = sup.load(std::path::Path::new(dir), true, true).expect("load");
    let pid = sup.call(&id, "pid", Value::Null).expect("pid");
    // This process's stdout isn't a module protocol: plain println is fine here.
    println!("{pid}");
    std::thread::sleep(std::time::Duration::from_secs(120));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--host") {
        host(args.get(2).map(String::as_str).unwrap_or("."));
        return;
    }
    island_module::run(Echo::default());
}
