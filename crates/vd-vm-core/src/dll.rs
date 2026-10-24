//! The real `VoicemeeterRemote64.dll` behind the [`VoicemeeterApi`] trait.

use crate::api::{LevelKind, LoginStatus, Result, VmError, VoicemeeterApi, VoicemeeterKind};
use libloading::{Library, Symbol};
use std::ffi::{c_char, c_long, CString};
use std::path::{Path, PathBuf};

/// Voicemeeter registers itself here; `UninstallString` points at the setup exe
/// inside the install folder, which is where the DLL lives too.
const UNINSTALL_KEYS: &[&str] = &[
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\VB:Voicemeeter {17359A74-1236-5467}",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\VB:Voicemeeter {17359A74-1236-5467}",
];

const DLL_NAME: &str = "VoicemeeterRemote64.dll";

/// Fallbacks for installs that did not leave a registry trail.
fn default_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(base) = std::env::var(var) {
            out.push(Path::new(&base).join("VB").join("Voicemeeter").join(DLL_NAME));
        }
    }
    out
}

/// Find the DLL: registry first, then the usual install locations.
pub fn locate_dll() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
        use winreg::RegKey;

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        for key_path in UNINSTALL_KEYS {
            // Try both registry views: a 32-bit installer writes into the
            // WOW6432Node side, and which one we land on depends on the host.
            for flags in [KEY_READ | KEY_WOW64_32KEY, KEY_READ | KEY_WOW64_64KEY] {
                let Ok(key) = hklm.open_subkey_with_flags(key_path, flags) else {
                    continue;
                };
                let Ok(uninstall) = key.get_value::<String, _>("UninstallString") else {
                    continue;
                };
                // e.g. C:\Program Files (x86)\VB\Voicemeeter\Voicemeeter8Setup.exe
                if let Some(dir) = Path::new(uninstall.trim_matches('"')).parent() {
                    let candidate = dir.join(DLL_NAME);
                    if candidate.is_file() {
                        tracing::debug!(path = %candidate.display(), "found Voicemeeter DLL via registry");
                        return Some(candidate);
                    }
                }
            }
        }
    }

    default_paths().into_iter().find(|p| p.is_file())
}

// Raw signatures from VoicemeeterRemote.h. All are cdecl.
type FnVoid = unsafe extern "C" fn() -> c_long;
type FnLong = unsafe extern "C" fn(c_long) -> c_long;
type FnOutLong = unsafe extern "C" fn(*mut c_long) -> c_long;
type FnGetFloat = unsafe extern "C" fn(*const c_char, *mut f32) -> c_long;
type FnSetFloat = unsafe extern "C" fn(*const c_char, f32) -> c_long;
type FnGetString = unsafe extern "C" fn(*const c_char, *mut c_char) -> c_long;
type FnSetString = unsafe extern "C" fn(*const c_char, *const c_char) -> c_long;
type FnSetParams = unsafe extern "C" fn(*const c_char) -> c_long;
type FnGetLevel = unsafe extern "C" fn(c_long, c_long, *mut f32) -> c_long;

/// Loaded function pointers. Valid only while `_lib` is alive, which is why
/// they live in the same struct.
struct Fns {
    login: FnVoid,
    logout: FnVoid,
    run_voicemeeter: FnLong,
    get_type: FnOutLong,
    is_dirty: FnVoid,
    get_float: FnGetFloat,
    set_float: FnSetFloat,
    get_string: FnGetString,
    set_string: FnSetString,
    set_parameters: FnSetParams,
    get_level: FnGetLevel,
}

pub struct RemoteDll {
    fns: Fns,
    /// Must outlive `fns`; keeping it here guarantees that.
    _lib: Library,
}

impl RemoteDll {
    /// Load the DLL from wherever it is installed.
    pub fn load() -> Result<Self> {
        let path = locate_dll().ok_or(VmError::NotInstalled)?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        // SAFETY: we are loading a known-good system DLL and immediately
        // resolving symbols with signatures taken from its public header.
        unsafe {
            let lib = Library::new(path).map_err(|e| VmError::Load(e.to_string()))?;

            macro_rules! sym {
                ($name:literal, $ty:ty) => {{
                    let s: Symbol<$ty> = lib
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|e| VmError::Load(format!("{}: {}", $name, e)))?;
                    *s
                }};
            }

            let fns = Fns {
                login: sym!("VBVMR_Login", FnVoid),
                logout: sym!("VBVMR_Logout", FnVoid),
                run_voicemeeter: sym!("VBVMR_RunVoicemeeter", FnLong),
                get_type: sym!("VBVMR_GetVoicemeeterType", FnOutLong),
                is_dirty: sym!("VBVMR_IsParametersDirty", FnVoid),
                get_float: sym!("VBVMR_GetParameterFloat", FnGetFloat),
                set_float: sym!("VBVMR_SetParameterFloat", FnSetFloat),
                get_string: sym!("VBVMR_GetParameterStringA", FnGetString),
                set_string: sym!("VBVMR_SetParameterStringA", FnSetString),
                set_parameters: sym!("VBVMR_SetParameters", FnSetParams),
                get_level: sym!("VBVMR_GetLevel", FnGetLevel),
            };

            tracing::info!(path = %path.display(), "loaded Voicemeeter Remote DLL");
            Ok(Self { fns, _lib: lib })
        }
    }
}

/// Turn a Rust string into the NUL-terminated form the C API wants.
fn c_string(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| VmError::Call {
        func: "parameter name",
        code: -1,
    })
}

fn check(func: &'static str, code: c_long) -> Result<()> {
    if code < 0 {
        Err(VmError::Call { func, code: code as i32 })
    } else {
        Ok(())
    }
}

impl VoicemeeterApi for RemoteDll {
    fn login(&self) -> Result<LoginStatus> {
        // 0 = ok, 1 = ok but Voicemeeter is not running, -2 = already logged in.
        let rc = unsafe { (self.fns.login)() };
        match rc {
            0 => Ok(LoginStatus::Ok),
            1 => Ok(LoginStatus::EngineNotRunning),
            -2 => Ok(LoginStatus::AlreadyLoggedIn),
            code => Err(VmError::Call { func: "VBVMR_Login", code: code as i32 }),
        }
    }

    fn logout(&self) -> Result<()> {
        check("VBVMR_Logout", unsafe { (self.fns.logout)() })
    }

    fn run_voicemeeter(&self, kind: VoicemeeterKind) -> Result<()> {
        check("VBVMR_RunVoicemeeter", unsafe {
            (self.fns.run_voicemeeter)(kind.run_arg() as c_long)
        })
    }

    fn kind(&self) -> Result<VoicemeeterKind> {
        let mut raw: c_long = 0;
        let rc = unsafe { (self.fns.get_type)(&mut raw) };
        // The engine has not published its type yet.
        if rc != 0 {
            return Err(VmError::EngineDown);
        }
        VoicemeeterKind::from_raw(raw as i32)
    }

    fn is_dirty(&self) -> Result<bool> {
        let rc = unsafe { (self.fns.is_dirty)() };
        if rc < 0 {
            return Err(VmError::EngineDown);
        }
        Ok(rc == 1)
    }

    fn get_float(&self, name: &str) -> Result<f32> {
        let cname = c_string(name)?;
        let mut value = 0.0f32;
        let rc = unsafe { (self.fns.get_float)(cname.as_ptr(), &mut value) };
        check("VBVMR_GetParameterFloat", rc)?;
        Ok(value)
    }

    fn set_float(&self, name: &str, value: f32) -> Result<()> {
        let cname = c_string(name)?;
        check("VBVMR_SetParameterFloat", unsafe {
            (self.fns.set_float)(cname.as_ptr(), value)
        })
    }

    fn get_string(&self, name: &str) -> Result<String> {
        let cname = c_string(name)?;
        // The API writes up to 512 bytes into a caller-provided buffer.
        let mut buf = vec![0u8; 512];
        let rc = unsafe { (self.fns.get_string)(cname.as_ptr(), buf.as_mut_ptr() as *mut c_char) };
        check("VBVMR_GetParameterStringA", rc)?;
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Ok(String::from_utf8_lossy(&buf[..end]).into_owned())
    }

    fn set_string(&self, name: &str, value: &str) -> Result<()> {
        let cname = c_string(name)?;
        let cvalue = c_string(value)?;
        check("VBVMR_SetParameterStringA", unsafe {
            (self.fns.set_string)(cname.as_ptr(), cvalue.as_ptr())
        })
    }

    fn set_parameters(&self, script: &str) -> Result<()> {
        let cscript = c_string(script)?;
        check("VBVMR_SetParameters", unsafe {
            (self.fns.set_parameters)(cscript.as_ptr())
        })
    }

    fn get_level(&self, kind: LevelKind, channel: usize) -> Result<f32> {
        let mut value = 0.0f32;
        let rc = unsafe { (self.fns.get_level)(kind as c_long, channel as c_long, &mut value) };
        // Out-of-range channels are common while the engine restarts; treat
        // them as silence rather than an error so meters just go quiet.
        if rc != 0 {
            return Ok(0.0);
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_finds_the_dll_when_voicemeeter_is_installed() {
        match locate_dll() {
            Some(p) => {
                assert!(p.is_file());
                assert_eq!(p.file_name().unwrap(), DLL_NAME);
            }
            // CI machines will not have Voicemeeter; that is not a failure.
            None => eprintln!("Voicemeeter not installed; skipping"),
        }
    }
}
