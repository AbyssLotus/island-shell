use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;

use crate::taskbar;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn shutdown_exe(args: &[&str]) -> Result<(), String> {
    Command::new("shutdown.exe").args(args).creation_flags(CREATE_NO_WINDOW).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Hand the tray and taskbar back to Explorer before the session ends.
fn leave(dir: &Path) {
    crate::tray::shutdown();
    taskbar::restore(dir);
}

pub fn run(action: &str, dir: &Path) -> Result<(), String> {
    match action {
        "lock" => unsafe { LockWorkStation() }.map_err(|e| e.message().to_string()),
        "sleep" => {
            if unsafe { SetSuspendState(false, false, false) } {
                Ok(())
            } else {
                Err("Windows refused to sleep".into())
            }
        }
        "restart" => {
            leave(dir);
            shutdown_exe(&["/r", "/t", "0"])
        }
        "shutdown" => {
            leave(dir);
            shutdown_exe(&["/s", "/t", "0"])
        }
        "signout" => {
            leave(dir);
            shutdown_exe(&["/l"])
        }
        _ => Err("Unknown power action".into()),
    }
}
