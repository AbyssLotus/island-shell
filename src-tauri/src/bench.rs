//! Benchmarks for the Task Manager's Benchmarks page. Each runs only when asked, on a background
//! thread, and reports a live reading while it runs (for the gauges).
//!
//! - CPU: a fixed mix of integer and floating-point work, on one thread and then on every logical
//!   processor, counted in millions of loop rounds per second.
//! - Memory: copying a 256 MB block (GB/s) and following a random chain through 128 MB (latency).
//! - Disk: a 1 GB file written and read straight to and from the drive (no cache), then 4 KB reads
//!   at random places for a few seconds. The file goes in the temp folder on the Windows drive, or
//!   a folder at the root of another drive, and is deleted afterwards.
//! - Internet: Cloudflare's public speed test (speed.cloudflare.com): latency, then download and
//!   upload with four connections each. Nothing else is sent.
//! The graphics benchmark runs in the page itself (WebGL), on the card that draws the island.

use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

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
    /// Milliseconds and megabits per second.
    pub net_ping: f64,
    pub net_down: f64,
    pub net_up: f64,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub running: bool,
    /// "cpu", "memory", "disk", "net" (what's running or ran last).
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

pub fn start(kind: &str, drive: Option<String>) -> Result<(), String> {
    if status().running {
        return Err("A benchmark is already running".into());
    }
    if !["cpu", "memory", "disk", "net"].contains(&kind) {
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

    /// `BENCH=cpu,memory,disk,net cargo test --release bench::tests::live -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a real PC: runs the benchmarks (BENCH=cpu,memory,disk,net)"]
    fn live() {
        let kinds = std::env::var("BENCH").unwrap_or_else(|_| "cpu,memory".into());
        for k in kinds.split(',') {
            start(k, Some("G:".into())).unwrap();
            let s = wait();
            println!("{k}: {}", serde_json::to_string(&s.results).unwrap());
            assert!(s.error.is_empty(), "{}", s.error);
        }
    }
}
