//! Benchmarks for the Task Manager's Benchmarks page. Each runs only when asked, on a background
//! thread, and reports a live reading while it runs (for the gauges).
//!
//! - CPU: a fixed mix of integer and floating-point work, on one thread and then on every logical
//!   processor, counted in millions of loop rounds per second.
//! - Memory: copying a 256 MB block (GB/s) and following a random chain through 128 MB (latency).
//! - Disk: a 1 GB file written and read straight to and from the drive (no cache), then 4 KB reads
//!   at random places for a few seconds. The file goes in the temp folder on the Windows drive, or
//!   a folder at the root of another drive, and is deleted afterwards.
//! - Graphics: Direct3D 11 on the card the user picks, which needn't be the one drawing the island
//!   (on a PC with two, that's often the one built into the CPU): a compute shader of multiply-adds
//!   (GFLOPS), then blended full-screen fills of a 2048 × 2048 picture (Gpixels/s).
//! - Internet: Cloudflare's public speed test (speed.cloudflare.com): latency, then download and
//!   upload with four connections each. Nothing else is sent.

use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::core::{s, Interface, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIAdapter1};

#[derive(Serialize, Clone, Default)]
pub struct Results {
    /// Millions of work rounds per second.
    pub cpu_single: f64,
    pub cpu_multi: f64,
    pub threads: u32,
    /// GB/s and nanoseconds.
    pub mem_copy: f64,
    pub mem_latency: f64,
    pub disk_drive: String,
    /// MB/s, and 4 KB random reads per second.
    pub disk_read: f64,
    pub disk_write: f64,
    pub disk_iops: f64,
    /// GFLOPS and Gpixels/s, and the card they're from.
    pub gpu_gflops: f64,
    pub gpu_fill: f64,
    pub gpu_name: String,
    /// Milliseconds and megabits per second.
    pub net_ping: f64,
    pub net_down: f64,
    pub net_up: f64,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub running: bool,
    /// "cpu", "memory", "disk", "gpu", "net" (what's running or ran last).
    pub kind: String,
    pub stage: String,
    pub progress: f64,
    /// The current reading, in `unit`.
    pub live: f64,
    pub unit: String,
    pub results: Results,
    pub error: String,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

fn set(f: impl FnOnce(&mut Status)) {
    let mut g = STATUS.lock().unwrap();
    f(g.get_or_insert_with(Status::default));
}

pub fn status() -> Status {
    STATUS.lock().unwrap().clone().unwrap_or_default()
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// `drive` is the disk test's drive letter, `card` the graphics test's card (a `GpuChoice` id).
pub fn start(kind: &str, drive: Option<String>, card: Option<String>) -> Result<(), String> {
    if status().running {
        return Err("A benchmark is already running".into());
    }
    if !["cpu", "memory", "disk", "gpu", "net"].contains(&kind) {
        return Err("Unknown benchmark".into());
    }
    CANCEL.store(false, Ordering::SeqCst);
    let kind = kind.to_string();
    set(|s| {
        s.running = true;
        s.kind = kind.clone();
        s.stage = "Starting".into();
        s.progress = 0.0;
        s.live = 0.0;
        s.error.clear();
    });
    std::thread::spawn(move || {
        let r = match kind.as_str() {
            "cpu" => cpu(),
            "memory" => memory(),
            "disk" => disk(drive.as_deref().unwrap_or("C:")),
            "gpu" => gpu(card.as_deref()),
            _ => net(),
        };
        set(|s| {
            s.running = false;
            s.progress = 1.0;
            s.stage = if CANCEL.load(Ordering::SeqCst) { "Stopped".into() } else { "Done".into() };
            if let Err(e) = r {
                s.error = e;
            }
        });
        crate::util::log_line(&format!("Benchmark: {kind} finished"));
    });
    Ok(())
}

/* ---------- CPU ---------- */

/// One "round" is one pass of this loop: an xorshift step, a multiply-add, a float step.
#[inline(never)]
fn crunch(seed: u64, n: u32) -> u64 {
    let mut x = seed | 1;
    let mut f = 1.000_1f64 + (seed & 7) as f64;
    let mut acc = 0u64;
    for i in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        acc = acc.wrapping_add(x.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 7);
        f = f * 1.000_000_7 + (i & 15) as f64 * 1e-9;
        if f > 2.0 {
            f -= 1.0;
        }
    }
    acc ^ f.to_bits()
}

const CHUNK: u32 = 1 << 20;

fn cpu() -> Result<(), String> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    set(|s| {
        s.results.cpu_single = 0.0;
        s.results.cpu_multi = 0.0;
        s.results.threads = threads as u32;
        s.unit = "Mrounds/s".into();
    });
    // Single core: 4 seconds.
    set(|s| s.stage = "Single core".into());
    let t0 = Instant::now();
    let mut rounds = 0u64;
    let mut seed = 0x1234u64;
    while t0.elapsed() < Duration::from_secs(4) && !CANCEL.load(Ordering::Relaxed) {
        seed = std::hint::black_box(crunch(seed, CHUNK));
        rounds += CHUNK as u64;
        let rate = rounds as f64 / t0.elapsed().as_secs_f64() / 1e6;
        set(|s| {
            s.live = rate;
            s.progress = t0.elapsed().as_secs_f64() / 8.5;
        });
    }
    let single = rounds as f64 / t0.elapsed().as_secs_f64() / 1e6;
    set(|s| s.results.cpu_single = single);
    if CANCEL.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Every logical processor: 4 seconds.
    set(|s| s.stage = format!("All {threads} threads"));
    let total = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let t0 = Instant::now();
    let workers: Vec<_> = (0..threads)
        .map(|i| {
            let (total, stop) = (total.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut seed = 0x9876u64 + i as u64;
                while !stop.load(Ordering::Relaxed) {
                    seed = std::hint::black_box(crunch(seed, CHUNK));
                    total.fetch_add(CHUNK as u64, Ordering::Relaxed);
                }
            })
        })
        .collect();
    while t0.elapsed() < Duration::from_secs(4) && !CANCEL.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
        let rate = total.load(Ordering::Relaxed) as f64 / t0.elapsed().as_secs_f64() / 1e6;
        set(|s| {
            s.live = rate;
            s.progress = 0.5 + t0.elapsed().as_secs_f64() / 8.5;
        });
    }
    stop.store(true, Ordering::Relaxed);
    for w in workers {
        let _ = w.join();
    }
    let multi = total.load(Ordering::Relaxed) as f64 / t0.elapsed().as_secs_f64() / 1e6;
    set(|s| s.results.cpu_multi = multi);
    Ok(())
}

/* ---------- memory ---------- */

fn memory() -> Result<(), String> {
    set(|s| {
        s.results.mem_copy = 0.0;
        s.results.mem_latency = 0.0;
        s.unit = "GB/s".into();
        s.stage = "Copy".into();
    });
    const SIZE: usize = 256 << 20;
    let src = vec![1u8; SIZE];
    let mut dst = vec![0u8; SIZE];
    dst.copy_from_slice(&src); // touch every page first
    let t0 = Instant::now();
    let mut bytes = 0u64;
    while t0.elapsed() < Duration::from_secs(3) && !CANCEL.load(Ordering::Relaxed) {
        dst.copy_from_slice(std::hint::black_box(&src));
        bytes += SIZE as u64;
        let rate = bytes as f64 / t0.elapsed().as_secs_f64() / 1e9;
        set(|s| {
            s.live = rate;
            s.progress = t0.elapsed().as_secs_f64() / 6.0;
        });
    }
    let copy = bytes as f64 / t0.elapsed().as_secs_f64() / 1e9;
    drop((src, dst));
    set(|s| s.results.mem_copy = copy);
    if CANCEL.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Latency: one random cycle through 128 MB (Sattolo's shuffle), followed one step at a time.
    set(|s| {
        s.stage = "Latency".into();
        s.unit = "ns".into();
    });
    let n = (128usize << 20) / 8;
    let mut next: Vec<usize> = (0..n).collect();
    let mut rng = 0x2545_F491_4F6C_DD1Du64;
    for i in (1..n).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let j = (rng % i as u64) as usize;
        next.swap(i, j);
    }
    set(|s| s.progress = 0.75);
    let steps = 20_000_000u64;
    let t0 = Instant::now();
    let mut p = 0usize;
    for k in 0..steps {
        p = next[p];
        if k % 4_000_000 == 0 && CANCEL.load(Ordering::Relaxed) {
            return Ok(());
        }
    }
    std::hint::black_box(p);
    let ns = t0.elapsed().as_nanos() as f64 / steps as f64;
    set(|s| {
        s.results.mem_latency = ns;
        s.live = ns;
    });
    Ok(())
}

/* ---------- disk ---------- */

const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
const FILE_FLAG_WRITE_THROUGH: u32 = 0x8000_0000;

/// A page-aligned buffer (unbuffered I/O needs sector-aligned memory).
struct Aligned {
    ptr: *mut u8,
    len: usize,
}
impl Aligned {
    fn new(len: usize) -> Self {
        let layout = std::alloc::Layout::from_size_align(len, 4096).unwrap();
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        Aligned { ptr, len }
    }
    fn slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}
impl Drop for Aligned {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.ptr, std::alloc::Layout::from_size_align(self.len, 4096).unwrap()) }
    }
}

fn disk(drive: &str) -> Result<(), String> {
    use std::os::windows::fs::OpenOptionsExt;
    let letter = drive.trim().trim_end_matches(['\\', ':']).to_ascii_uppercase();
    if letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err("Choose a drive".into());
    }
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).trim_end_matches(':').to_ascii_uppercase();
    // The Windows drive's root isn't writable without admin rights; its temp folder is.
    let (dir, made) = if letter == sys {
        (std::env::temp_dir(), false)
    } else {
        let d = std::path::PathBuf::from(format!("{letter}:\\island-bench"));
        let made = !d.exists();
        std::fs::create_dir_all(&d).map_err(|e| format!("Can't write to {letter}: ({e})"))?;
        (d, made)
    };
    let path = dir.join("island-bench.tmp");
    let (mut free, mut _total) = (0u64, 0u64);
    unsafe {
        let _ = windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(&windows::core::HSTRING::from(format!("{letter}:\\")), Some(&mut free), Some(&mut _total), None);
    }
    const SIZE: u64 = 1 << 30;
    if free < SIZE * 2 {
        return Err(format!("{letter}: needs 2 GB free for the test"));
    }
    set(|s| {
        s.results.disk_drive = format!("{letter}:");
        s.results.disk_read = 0.0;
        s.results.disk_write = 0.0;
        s.results.disk_iops = 0.0;
        s.unit = "MB/s".into();
        s.stage = "Sequential write".into();
    });
    let result = (|| -> Result<(), String> {
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH)
            .open(&path)
            .map_err(|e| e.to_string())?;
        const BLOCK: usize = 8 << 20;
        let mut buf = Aligned::new(BLOCK);
        let mut x = 0x9E37_79B9u32;
        for b in buf.slice().iter_mut() {
            // Incompressible data, so drives that compress don't look faster than they are.
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = x as u8;
        }
        let t0 = Instant::now();
        let mut done = 0u64;
        while done < SIZE {
            if CANCEL.load(Ordering::Relaxed) {
                return Ok(());
            }
            f.write_all(buf.slice()).map_err(|e| e.to_string())?;
            done += BLOCK as u64;
            let rate = done as f64 / t0.elapsed().as_secs_f64() / 1e6;
            set(|s| {
                s.live = rate;
                s.progress = done as f64 / SIZE as f64 * 0.33;
            });
        }
        let write = SIZE as f64 / t0.elapsed().as_secs_f64() / 1e6;
        set(|s| {
            s.results.disk_write = write;
            s.stage = "Sequential read".into();
        });
        f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        let mut done = 0u64;
        while done < SIZE {
            if CANCEL.load(Ordering::Relaxed) {
                return Ok(());
            }
            f.read_exact(buf.slice()).map_err(|e| e.to_string())?;
            done += BLOCK as u64;
            let rate = done as f64 / t0.elapsed().as_secs_f64() / 1e6;
            set(|s| {
                s.live = rate;
                s.progress = 0.33 + done as f64 / SIZE as f64 * 0.33;
            });
        }
        let read = SIZE as f64 / t0.elapsed().as_secs_f64() / 1e6;
        set(|s| {
            s.results.disk_read = read;
            s.stage = "Random 4 KB reads".into();
            s.unit = "IOPS".into();
        });
        let mut small = Aligned::new(4096);
        let blocks = SIZE / 4096;
        let t0 = Instant::now();
        let mut ops = 0u64;
        let mut r = 0x2545_F491u64;
        while t0.elapsed() < Duration::from_secs(4) && !CANCEL.load(Ordering::Relaxed) {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            f.seek(SeekFrom::Start((r % blocks) * 4096)).map_err(|e| e.to_string())?;
            f.read_exact(small.slice()).map_err(|e| e.to_string())?;
            ops += 1;
            if ops % 256 == 0 {
                let rate = ops as f64 / t0.elapsed().as_secs_f64();
                set(|s| {
                    s.live = rate;
                    s.progress = 0.66 + t0.elapsed().as_secs_f64() / 4.0 * 0.34;
                });
            }
        }
        let iops = ops as f64 / t0.elapsed().as_secs_f64();
        set(|s| s.results.disk_iops = iops);
        Ok(())
    })();
    // Our own test file, and the folder if we made it.
    let _ = std::fs::remove_file(&path);
    if made {
        let _ = std::fs::remove_dir(&dir);
    }
    result
}

/* ---------- graphics ---------- */

/// A graphics card the test can run on.
#[derive(Serialize, Clone)]
pub struct GpuChoice {
    /// Its LUID, which stays the same until Windows restarts.
    pub id: String,
    pub name: String,
    /// The graphics built into the CPU.
    pub integrated: bool,
}

fn gpu_choices() -> Vec<(IDXGIAdapter1, GpuChoice)> {
    let cpu = crate::stats::cpu_name();
    crate::sensors::dxgi_adapters()
        .into_iter()
        .map(|a| {
            let id = format!("{:08x}{:08x}", a.luid.HighPart as u32, a.luid.LowPart);
            let integrated = crate::sensors::on_cpu(&a.name, a.vendor, &cpu);
            (a.adapter, GpuChoice { id, name: a.name, integrated })
        })
        .collect()
}

/// The cards to choose from, in DXGI's order.
pub fn gpus() -> Vec<GpuChoice> {
    gpu_choices().into_iter().map(|(_, c)| c).collect()
}

fn gpu(id: Option<&str>) -> Result<(), String> {
    let mut cards = gpu_choices();
    if cards.is_empty() {
        return Err("No graphics card to test".into());
    }
    // Without a choice, the first card that isn't the CPU's.
    let i = match id {
        Some(id) => cards.iter().position(|(_, c)| c.id == id).ok_or("That graphics card isn't there any more")?,
        None => cards.iter().position(|(_, c)| !c.integrated).unwrap_or(0),
    };
    let (adapter, card) = cards.swap_remove(i);
    set(|s| {
        s.results.gpu_gflops = 0.0;
        s.results.gpu_fill = 0.0;
        s.results.gpu_name = card.name.clone();
        s.unit = "GFLOPS".into();
        s.stage = "Shader math".into();
    });
    let g = Gpu::new(Some(&adapter))?;
    let gflops = g.shader_math(1 << 20, 4.0, |rate, done| {
        set(|s| {
            s.live = rate;
            s.progress = done * 0.6;
        })
    })?;
    set(|s| s.results.gpu_gflops = gflops);
    if CANCEL.load(Ordering::Relaxed) {
        return Ok(());
    }
    set(|s| {
        s.stage = "Fill rate".into();
        s.unit = "Gpixels/s".into();
    });
    let fill = g.fill_rate(3.0, |rate, done| {
        set(|s| {
            s.live = rate;
            s.progress = 0.6 + done * 0.4;
        })
    })?;
    set(|s| s.results.gpu_fill = fill);
    Ok(())
}

/// Each round is 16 multiply-adds on all 4 lanes, each needing the one before, so no compiler can
/// fold them. The numbers soon overflow to infinity, which the card works through at full speed.
const MATH_CS: &str = "
RWBuffer<float4> o : register(u0);
[numthreads(256, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    float4 x = float4(id.x * 1e-7, 0.1, 0.2, 0.3);
    float4 y = float4(0.4, id.x * 1e-7, 0.5, 0.6);
    [loop] for (uint i = 0; i < ROUNDS; i++) {
        x = mad(y, x, y); y = mad(x, y, x); x = mad(y, x, y); y = mad(x, y, x);
        x = mad(y, x, y); y = mad(x, y, x); x = mad(y, x, y); y = mad(x, y, x);
        x = mad(y, x, y); y = mad(x, y, x); x = mad(y, x, y); y = mad(x, y, x);
        x = mad(y, x, y); y = mad(x, y, x); x = mad(y, x, y); y = mad(x, y, x);
    }
    o[id.x] = x + y;
}";
const MATH_ROUNDS: u32 = 256;

/// One triangle over the whole picture, its corners worked out from the vertex number (no buffer).
const FILL_VS: &str = "
float4 main(uint id : SV_VertexID) : SV_Position {
    float2 p = float2((id << 1) & 2, id & 2);
    return float4(p * 2.0 - 1.0, 0.0, 1.0);
}";
const FILL_PS: &str = "
float4 main() : SV_Target { return float4(0.9, 0.5, 0.25, 0.1); }";

fn d3d(e: windows::core::Error) -> String {
    format!("The graphics test failed: {} ({:#010x})", e.message().trim_end(), e.code().0)
}

fn compile(src: &str, target: PCSTR) -> Result<Vec<u8>, String> {
    let (mut code, mut errors) = (None, None);
    let r = unsafe {
        D3DCompile(
            src.as_ptr() as *const _,
            src.len(),
            PCSTR::null(),
            None,
            None,
            s!("main"),
            target,
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    let bytes = |b: &ID3DBlob| unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec() };
    match (r, code) {
        (Ok(()), Some(code)) => Ok(bytes(&code)),
        (r, _) => {
            let why = errors.map(|e| String::from_utf8_lossy(&bytes(&e)).trim_end_matches(['\0', '\n']).to_string());
            Err(format!("Couldn't build the graphics test: {}", why.unwrap_or_else(|| r.err().map(|e| e.message()).unwrap_or_default())))
        }
    }
}

/// A Direct3D 11 device, and a query to wait for it with.
struct Gpu {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    done: ID3D11Query,
}

impl Gpu {
    /// A device on `adapter`, or on WARP (Windows' software renderer, for the tests) without one.
    fn new(adapter: Option<&IDXGIAdapter1>) -> Result<Gpu, String> {
        let (adapter, driver) = match adapter {
            Some(a) => (Some(a.cast::<IDXGIAdapter>().map_err(d3d)?), D3D_DRIVER_TYPE_UNKNOWN),
            None => (None, D3D_DRIVER_TYPE_WARP),
        };
        let (mut dev, mut ctx, mut done) = (None, None, None);
        unsafe {
            D3D11CreateDevice(
                adapter.as_ref(),
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut dev),
                None,
                Some(&mut ctx),
            )
        }
        .map_err(|e| format!("This card can't run the test, which needs Direct3D 11 ({})", e.message().trim_end()))?;
        let (Some(dev), Some(ctx)) = (dev, ctx) else { return Err("Direct3D 11 didn't start".into()) };
        unsafe { dev.CreateQuery(&D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 }, Some(&mut done)) }.map_err(d3d)?;
        Ok(Gpu {
            dev,
            ctx,
            done: done.ok_or("Direct3D 11 didn't start")?,
        })
    }

    /// Waits until the card has done everything it's been given.
    fn finish(&self) -> Result<(), String> {
        let t0 = Instant::now();
        unsafe { self.ctx.End(&self.done) };
        loop {
            // A BOOL, set once the card gets to the End above.
            let mut done = 0u32;
            unsafe { self.ctx.GetData(&self.done, Some(&mut done as *mut u32 as *mut _), 4, 0) }.map_err(d3d)?;
            if done != 0 {
                return Ok(());
            }
            if t0.elapsed() > Duration::from_secs(10) {
                return Err("The graphics card stopped answering".into());
            }
            std::thread::yield_now();
        }
    }

    /// Queues `step` in batches, waiting for each, and returns steps per second over `secs`. That
    /// starts once the driver has built the shader and the card has had half a second to reach
    /// full clocks. `report` gets the rate so far and the share of the time gone.
    fn timed(&self, secs: f64, mut step: impl FnMut(), mut report: impl FnMut(f64, f64)) -> Result<f64, String> {
        const WARM: f64 = 0.5;
        let share = |t: f64| (t / (secs + WARM)).min(1.0);
        let batch = |n: u32, step: &mut dyn FnMut()| {
            for _ in 0..n {
                step();
            }
            self.finish()
        };
        batch(1, &mut step)?;
        // Batches of 25 to 50 ms: long enough that the wait between them costs nothing, short
        // enough for the dial to move and Stop to answer.
        let mut n = 1u32;
        while n < 1 << 14 {
            let t = Instant::now();
            batch(n, &mut step)?;
            if t.elapsed() >= Duration::from_millis(25) {
                break;
            }
            n *= 2;
        }
        let warm = Instant::now();
        while warm.elapsed().as_secs_f64() < WARM && !CANCEL.load(Ordering::Relaxed) {
            batch(n, &mut step)?;
            report(0.0, share(warm.elapsed().as_secs_f64()));
        }
        let t0 = Instant::now();
        let mut steps = 0u64;
        while t0.elapsed().as_secs_f64() < secs && !CANCEL.load(Ordering::Relaxed) {
            batch(n, &mut step)?;
            steps += n as u64;
            let t = t0.elapsed().as_secs_f64();
            report(steps as f64 / t, share(WARM + t));
        }
        Ok(steps as f64 / t0.elapsed().as_secs_f64())
    }

    /// The first `len` bytes of `src` (a buffer, or a texture's top-left pixel), through
    /// `staging`: a small one of the same kind that the CPU can read.
    fn peek(&self, staging: &ID3D11Resource, src: &ID3D11Resource, region: D3D11_BOX, len: usize) -> Result<Vec<u8>, String> {
        unsafe {
            self.ctx.CopySubresourceRegion(staging, 0, 0, 0, 0, src, 0, Some(&region));
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut m)).map_err(d3d)?;
            let out = std::slice::from_raw_parts(m.pData as *const u8, len).to_vec();
            self.ctx.Unmap(staging, 0);
            Ok(out)
        }
    }

    fn buffer(&self, len: u32, usage: D3D11_USAGE, bind: D3D11_BIND_FLAG, cpu: D3D11_CPU_ACCESS_FLAG) -> Result<ID3D11Buffer, String> {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: len,
            Usage: usage,
            BindFlags: bind.0 as u32,
            CPUAccessFlags: cpu.0 as u32,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let mut b = None;
        unsafe { self.dev.CreateBuffer(&desc, None, Some(&mut b)) }.map_err(d3d)?;
        b.ok_or_else(|| "Couldn't make a buffer for the graphics test".into())
    }

    fn texture(&self, size: u32, usage: D3D11_USAGE, bind: D3D11_BIND_FLAG, cpu: D3D11_CPU_ACCESS_FLAG) -> Result<ID3D11Texture2D, String> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: size,
            Height: size,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: usage,
            BindFlags: bind.0 as u32,
            CPUAccessFlags: cpu.0 as u32,
            MiscFlags: 0,
        };
        let mut t = None;
        unsafe { self.dev.CreateTexture2D(&desc, None, Some(&mut t)) }.map_err(d3d)?;
        t.ok_or_else(|| "Couldn't make a picture for the graphics test".into())
    }

    /// `MATH_CS` over `threads` threads (a multiple of 256); GFLOPS.
    fn shader_math(&self, threads: u32, secs: f64, mut report: impl FnMut(f64, f64)) -> Result<f64, String> {
        let code = compile(&MATH_CS.replace("ROUNDS", &MATH_ROUNDS.to_string()), s!("cs_5_0"))?;
        let out = self.buffer(threads * 16, D3D11_USAGE_DEFAULT, D3D11_BIND_UNORDERED_ACCESS, D3D11_CPU_ACCESS_FLAG(0))?;
        let staging = self.buffer(16, D3D11_USAGE_STAGING, D3D11_BIND_FLAG(0), D3D11_CPU_ACCESS_READ)?;
        let view = D3D11_UNORDERED_ACCESS_VIEW_DESC {
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
            Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_UAV {
                    FirstElement: 0,
                    NumElements: threads,
                    Flags: 0,
                },
            },
        };
        let (mut cs, mut uav) = (None, None);
        unsafe {
            self.dev.CreateComputeShader(&code, None, Some(&mut cs)).map_err(d3d)?;
            self.dev.CreateUnorderedAccessView(&out, Some(&view), Some(&mut uav)).map_err(d3d)?;
            self.ctx.CSSetShader(cs.as_ref(), None);
            self.ctx.CSSetUnorderedAccessViews(0, 1, Some(&uav), None);
        }
        // Per thread: rounds × 16 multiply-adds × 4 lanes × 2 operations.
        let flops = threads as f64 * MATH_ROUNDS as f64 * 128.0;
        let rate = self.timed(secs, || unsafe { self.ctx.Dispatch(threads / 256, 1, 1) }, |r, done| report(r * flops / 1e9, done))?;
        // It ran: the first thread's answer is in the buffer, which started at zero.
        let first = self.peek(
            &staging.cast().map_err(d3d)?,
            &out.cast().map_err(d3d)?,
            D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: 16,
                bottom: 1,
                back: 1,
            },
            16,
        )?;
        if first.iter().all(|&b| b == 0) {
            return Err("The graphics card didn't run the test".into());
        }
        Ok(rate * flops / 1e9)
    }

    /// Blended full-screen fills of a 2048 × 2048 picture; Gpixels/s.
    fn fill_rate(&self, secs: f64, mut report: impl FnMut(f64, f64)) -> Result<f64, String> {
        const SIZE: u32 = 2048;
        let (vs, ps) = (compile(FILL_VS, s!("vs_5_0"))?, compile(FILL_PS, s!("ps_5_0"))?);
        let pic = self.texture(SIZE, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_FLAG(0))?;
        let staging = self.texture(1, D3D11_USAGE_STAGING, D3D11_BIND_FLAG(0), D3D11_CPU_ACCESS_READ)?;
        // Blending, so every fill reads the picture as well as writing it.
        let mut blend = D3D11_BLEND_DESC::default();
        blend.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_SRC_ALPHA,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        // The triangle winds counter-clockwise, which the default state would cull.
        let raster = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let (mut v, mut p, mut rtv, mut b, mut r) = (None, None, None, None, None);
        unsafe {
            self.dev.CreateVertexShader(&vs, None, Some(&mut v)).map_err(d3d)?;
            self.dev.CreatePixelShader(&ps, None, Some(&mut p)).map_err(d3d)?;
            self.dev.CreateRenderTargetView(&pic, None, Some(&mut rtv)).map_err(d3d)?;
            self.dev.CreateBlendState(&blend, Some(&mut b)).map_err(d3d)?;
            self.dev.CreateRasterizerState(&raster, Some(&mut r)).map_err(d3d)?;
            let c = &self.ctx;
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.VSSetShader(v.as_ref(), None);
            c.PSSetShader(p.as_ref(), None);
            c.RSSetState(r.as_ref());
            c.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: SIZE as f32,
                Height: SIZE as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            c.OMSetRenderTargets(Some(&[rtv.clone()]), None);
            c.OMSetBlendState(b.as_ref(), None, u32::MAX);
            if let Some(rtv) = &rtv {
                c.ClearRenderTargetView(rtv, &[0.0; 4]);
            }
        }
        let pixels = (SIZE * SIZE) as f64;
        let rate = self.timed(secs, || unsafe { self.ctx.Draw(3, 0) }, |r, done| report(r * pixels / 1e9, done))?;
        // It drew: the top-left pixel isn't the clear color any more.
        let px = self.peek(
            &staging.cast().map_err(d3d)?,
            &pic.cast().map_err(d3d)?,
            D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: 1,
                bottom: 1,
                back: 1,
            },
            4,
        )?;
        if px == [0, 0, 0, 0] {
            return Err("The graphics card didn't draw the test".into());
        }
        Ok(rate * pixels / 1e9)
    }
}

/* ---------- internet ---------- */

fn net() -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Foundation::Uri;
    use windows::Storage::Streams::{Buffer, DataWriter, InputStreamOptions};
    use windows::Web::Http::Filters::{HttpBaseProtocolFilter, HttpCacheReadBehavior};
    use windows::Web::Http::{HttpBufferContent, HttpClient};
    fn e(e: windows::core::Error) -> String {
        format!("The speed test couldn't reach Cloudflare: {}", e.message())
    }
    fn client() -> Result<HttpClient, String> {
        let f = HttpBaseProtocolFilter::new().map_err(e)?;
        let _ = f.CacheControl().and_then(|c| c.SetReadBehavior(HttpCacheReadBehavior::NoCache));
        HttpClient::Create(&f).map_err(e)
    }
    fn uri(q: &str) -> Result<Uri, String> {
        Uri::CreateUri(&HSTRING::from(format!("https://speed.cloudflare.com/{q}"))).map_err(e)
    }
    set(|s| {
        s.results.net_ping = 0.0;
        s.results.net_down = 0.0;
        s.results.net_up = 0.0;
        s.stage = "Latency".into();
        s.unit = "ms".into();
    });
    let c = client()?;
    // Latency: the median of eight empty downloads after the first (which pays for the handshake).
    let mut pings = vec![];
    for i in 0..9 {
        if CANCEL.load(Ordering::Relaxed) {
            return Ok(());
        }
        let u = uri(&format!("__down?bytes=0&r={i}"))?;
        let t0 = Instant::now();
        let resp = c.GetAsync(&u).and_then(|op| op.get()).map_err(e)?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let _ = resp.Content().and_then(|x| x.ReadAsStringAsync()).and_then(|op| op.get());
        if i > 0 {
            pings.push(ms);
            set(|s| {
                s.live = ms;
                s.progress = i as f64 / 9.0 * 0.1;
            });
        }
    }
    pings.sort_by(|a, b| a.total_cmp(b));
    let ping = pings[pings.len() / 2];
    set(|s| {
        s.results.net_ping = ping;
        s.stage = "Download".into();
        s.unit = "Mbps".into();
    });
    // Download: four streams for up to 8 seconds.
    let got = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(Mutex::new(None::<String>));
    let t0 = Instant::now();
    let streams: Vec<_> = (0..4)
        .map(|i| {
            let (got, stop, failed) = (got.clone(), stop.clone(), failed.clone());
            std::thread::spawn(move || {
                let run = || -> Result<(), String> {
                    let c = client()?;
                    while !stop.load(Ordering::Relaxed) {
                        let u = uri(&format!("__down?bytes=50000000&r=d{i}{}", crate::util::now_ms()))?;
                        let input = c.GetInputStreamAsync(&u).and_then(|op| op.get()).map_err(e)?;
                        let buf = Buffer::Create(1 << 20).map_err(e)?;
                        loop {
                            if stop.load(Ordering::Relaxed) {
                                return Ok(());
                            }
                            let b = input.ReadAsync(&buf, 1 << 20, InputStreamOptions::Partial).and_then(|op| op.get()).map_err(e)?;
                            let n = b.Length().map_err(e)?;
                            if n == 0 {
                                break;
                            }
                            got.fetch_add(n as u64, Ordering::Relaxed);
                        }
                    }
                    Ok(())
                };
                if let Err(x) = run() {
                    *failed.lock().unwrap() = Some(x);
                }
            })
        })
        .collect();
    while t0.elapsed() < Duration::from_secs(8) && !CANCEL.load(Ordering::Relaxed) && failed.lock().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(150));
        let mbps = got.load(Ordering::Relaxed) as f64 * 8.0 / t0.elapsed().as_secs_f64() / 1e6;
        set(|s| {
            s.live = mbps;
            s.progress = 0.1 + t0.elapsed().as_secs_f64() / 8.0 * 0.45;
        });
    }
    stop.store(true, Ordering::Relaxed);
    let down = got.load(Ordering::Relaxed) as f64 * 8.0 / t0.elapsed().as_secs_f64() / 1e6;
    for s in streams {
        let _ = s.join();
    }
    if let Some(x) = failed.lock().unwrap().take() {
        if down == 0.0 {
            return Err(x);
        }
    }
    set(|s| {
        s.results.net_down = down;
        s.stage = "Upload".into();
    });
    if CANCEL.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Upload: four streams posting 8 MB at a time, for up to 7 seconds.
    let payload: Arc<Vec<u8>> = Arc::new({
        let mut x = 0x1357_9BDFu32;
        (0..8 << 20)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    });
    let sent = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let t0 = Instant::now();
    let streams: Vec<_> = (0..4)
        .map(|_| {
            let (sent, stop, payload) = (sent.clone(), stop.clone(), payload.clone());
            std::thread::spawn(move || {
                let run = || -> Result<(), String> {
                    let c = client()?;
                    while !stop.load(Ordering::Relaxed) {
                        let w = DataWriter::new().map_err(e)?;
                        w.WriteBytes(&payload).map_err(e)?;
                        let content = HttpBufferContent::CreateFromBuffer(&w.DetachBuffer().map_err(e)?).map_err(e)?;
                        c.PostAsync(&uri("__up")?, &content).and_then(|op| op.get()).map_err(e)?;
                        sent.fetch_add(payload.len() as u64, Ordering::Relaxed);
                    }
                    Ok(())
                };
                let _ = run();
            })
        })
        .collect();
    let mut last = 0.0;
    while t0.elapsed() < Duration::from_secs(7) && !CANCEL.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(150));
        last = sent.load(Ordering::Relaxed) as f64 * 8.0 / t0.elapsed().as_secs_f64() / 1e6;
        let l = last;
        set(|s| {
            s.live = l;
            s.progress = 0.55 + t0.elapsed().as_secs_f64() / 7.0 * 0.45;
        });
    }
    stop.store(true, Ordering::Relaxed);
    // Uploads finish their current 8 MB before stopping; count what was sent in the time.
    let up = if last > 0.0 { last } else { sent.load(Ordering::Relaxed) as f64 * 8.0 / t0.elapsed().as_secs_f64() / 1e6 };
    set(|s| s.results.net_up = up);
    for s in streams {
        let _ = s.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait() -> Status {
        while status().running {
            std::thread::sleep(Duration::from_millis(500));
            let s = status();
            println!("  {} {:.0}% live {:.1} {}", s.stage, s.progress * 100.0, s.live, s.unit);
        }
        status()
    }

    /// `BENCH=cpu,memory,disk,gpu,net cargo test --release bench::tests::live -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a real PC: runs the benchmarks (BENCH=cpu,memory,disk,gpu,net)"]
    fn live() {
        let kinds = std::env::var("BENCH").unwrap_or_else(|_| "cpu,memory".into());
        for k in kinds.split(',') {
            start(k, Some("G:".into()), None).unwrap();
            let s = wait();
            println!("{k}: {}", serde_json::to_string(&s.results).unwrap());
            assert!(s.error.is_empty(), "{}", s.error);
        }
    }

    /// The graphics test end to end, briefly, on Windows' software renderer: the shaders build,
    /// and the checks that the card really ran and drew them pass.
    #[test]
    fn gpu_on_warp() {
        let g = Gpu::new(None).unwrap();
        let gflops = g.shader_math(1 << 14, 0.2, |_, _| {}).unwrap();
        let fill = g.fill_rate(0.2, |_, _| {}).unwrap();
        println!("WARP: {gflops:.1} GFLOPS, {fill:.2} Gpixels/s");
        assert!(gflops > 0.0 && fill > 0.0);
    }
}
