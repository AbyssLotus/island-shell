//! Diagnostic: connects to the eye tracker through Tobii Stream Engine (installed with Tobii
//! Experience) and prints a few seconds of gaze points and presence changes.
//! Run outside the sandbox: `tobii_probe.exe > tobii.txt`.

use std::ffi::{c_char, c_int, c_void, CStr};

use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

#[repr(C)]
#[derive(Default, Debug)]
struct Version {
    major: c_int,
    minor: c_int,
    revision: c_int,
    build: c_int,
}

#[repr(C)]
struct GazePoint {
    timestamp_us: i64,
    validity: c_int,
    xy: [f32; 2],
}

type Err = c_int;
type GetVersion = unsafe extern "C" fn(*mut Version) -> Err;
type ApiCreate = unsafe extern "C" fn(*mut *mut c_void, *const c_void, *const c_void) -> Err;
type UrlReceiver = unsafe extern "C" fn(*const c_char, *mut c_void);
type Enumerate = unsafe extern "C" fn(*mut c_void, UrlReceiver, *mut c_void) -> Err;
type DeviceCreate = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void) -> Err;
type GazeCb = unsafe extern "C" fn(*const GazePoint, *mut c_void);
type GazeSub = unsafe extern "C" fn(*mut c_void, GazeCb, *mut c_void) -> Err;
type PresenceCb = unsafe extern "C" fn(c_int, i64, *mut c_void);
type PresenceSub = unsafe extern "C" fn(*mut c_void, PresenceCb, *mut c_void) -> Err;
type Process = unsafe extern "C" fn(*mut c_void) -> Err;

unsafe extern "C" fn on_url(url: *const c_char, data: *mut c_void) {
    let v = &mut *(data as *mut Vec<String>);
    v.push(CStr::from_ptr(url).to_string_lossy().into_owned());
}

static mut SAMPLES: Vec<(i32, f32, f32)> = Vec::new();

unsafe extern "C" fn on_gaze(g: *const GazePoint, _: *mut c_void) {
    let g = &*g;
    #[allow(static_mut_refs)]
    SAMPLES.push((g.validity, g.xy[0], g.xy[1]));
}

static mut T0: Option<std::time::Instant> = None;

unsafe extern "C" fn on_presence(status: c_int, _ts: i64, _: *mut c_void) {
    #[allow(static_mut_refs)]
    let t = T0.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
    println!("{t:6.2}s presence {}", match status { 1 => "away", 2 => "present", _ => "unknown" });
}

fn main() {
    unsafe {
        let lib = LoadLibraryW(w!("C:\\Program Files\\Tobii\\Tobii EyeX\\tobii_stream_engine.dll")).expect("load dll");
        macro_rules! f {
            ($name:expr, $t:ty) => {
                std::mem::transmute::<_, $t>(GetProcAddress(lib, $name).expect("missing export"))
            };
        }
        let get_version: GetVersion = f!(s!("tobii_get_api_version"), GetVersion);
        let mut v = Version::default();
        println!("tobii_get_api_version -> {} {v:?}", get_version(&mut v));
        let api_create: ApiCreate = f!(s!("tobii_api_create"), ApiCreate);
        let mut api = std::ptr::null_mut();
        println!("tobii_api_create -> {}", api_create(&mut api, std::ptr::null(), std::ptr::null()));
        let enumerate: Enumerate = f!(s!("tobii_enumerate_local_device_urls"), Enumerate);
        let mut urls: Vec<String> = vec![];
        println!("enumerate -> {} {urls:?}", enumerate(api, on_url, &mut urls as *mut _ as *mut c_void));
        println!("urls: {urls:?}");
        let Some(url) = urls.first() else { return };
        let device_create: DeviceCreate = f!(s!("tobii_device_create"), DeviceCreate);
        let mut dev = std::ptr::null_mut();
        let curl = std::ffi::CString::new(url.as_str()).unwrap();
        println!("tobii_device_create(interactive) -> {}", device_create(api, curl.as_ptr(), 1, &mut dev));
        if dev.is_null() {
            return;
        }
        let gaze_sub: GazeSub = f!(s!("tobii_gaze_point_subscribe"), GazeSub);
        println!("gaze subscribe -> {}", gaze_sub(dev, on_gaze, std::ptr::null_mut()));
        let pres_sub: PresenceSub = f!(s!("tobii_user_presence_subscribe"), PresenceSub);
        println!("presence subscribe -> {}", pres_sub(dev, on_presence, std::ptr::null_mut()));
        let process: Process = f!(s!("tobii_device_process_callbacks"), Process);
        let secs: f32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(3.0);
        let t = std::time::Instant::now();
        T0 = Some(t);
        let mut errs = 0;
        while t.elapsed().as_secs_f32() < secs {
            if process(dev) != 0 {
                errs += 1;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        #[allow(static_mut_refs)]
        let samples = &SAMPLES;
        let valid = samples.iter().filter(|s| s.0 == 1).count();
        println!("{} gaze samples in {secs}s ({valid} valid), process errors {errs}", samples.len());
        for s in samples.iter().step_by((samples.len() / 8).max(1)) {
            println!("  valid {} x {:.3} y {:.3}", s.0, s.1, s.2);
        }
    }
}
