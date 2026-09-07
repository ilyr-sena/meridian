//! Process privilege detection and UAC elevation handling.

use tracing::info;

/// Check if current process has administrative/root privileges.
pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::IsUserAnAdmin;
        unsafe { IsUserAnAdmin() != 0 }
    }
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
}

/// On Windows, if not admin, relaunch self with UAC elevation ("runas").
/// Returns true if self was already elevated, or exits the process if relaunching.
pub fn ensure_admin_or_relaunch() -> bool {
    if is_admin() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOASYNC};
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_NORMAL;

        info!("Not running as Administrator. Requesting UAC elevation...");

        if let Ok(exe_path) = std::env::current_exe() {
            let exe_wide: Vec<u16> = exe_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
            let verb_wide: Vec<u16> = OsStr::new("runas").encode_wide().chain(std::iter::once(0)).collect();

            // Pass through current CLI arguments
            let args: Vec<String> = std::env::args().skip(1).collect();
            let args_joined = args.join(" ");
            let args_wide: Vec<u16> = OsStr::new(&args_joined).encode_wide().chain(std::iter::once(0)).collect();

            let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOASYNC;
            info.lpVerb = verb_wide.as_ptr();
            info.lpFile = exe_wide.as_ptr();
            info.lpParameters = if args.is_empty() { std::ptr::null() } else { args_wide.as_ptr() };
            info.nShow = SW_NORMAL as i32;

            let ok = unsafe { ShellExecuteExW(&mut info) };
            if ok != 0 {
                // Relaunched successfully, exit un-elevated parent
                std::process::exit(0);
            } else {
                warn!("User declined UAC elevation prompt.");
            }
        }
    }

    #[cfg(unix)]
    {
        info!("Running on Unix as unprivileged user (UID: {})", unsafe { libc::geteuid() });
    }

    false
}
