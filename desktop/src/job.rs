//! Windows process helpers: no console window for children, and a kill-on-close job object so
//! the server dies with the shell even when the shell is killed hard (no orphan server holding
//! the port and the upload ledger after a crash).

use std::process::{Child, Command};

#[cfg(windows)]
pub fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut Command) {}

/// A job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` holding one child. Dropping it (or
/// the shell process exiting for any reason) kills the child if it is still running.
#[derive(Debug)]
pub struct KillOnClose {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: a job object handle is a kernel handle usable from any thread; it is only closed in
// `Drop`, once.
#[cfg(windows)]
unsafe impl Send for KillOnClose {}

impl KillOnClose {
    /// Puts `child` in a new kill-on-close job. `None` when the OS refuses (the child then only
    /// stops through `Stack::stop`); this never fails the start.
    #[cfg(windows)]
    pub fn adopt(child: &Child) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: plain Win32 calls on handles we own; `info` outlives the call that reads it
        // and its size is passed exactly.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0
                && AssignProcessToJobObject(handle, child.as_raw_handle()) != 0;
            if !ok {
                CloseHandle(handle);
                return None;
            }
            Some(Self { handle })
        }
    }

    #[cfg(not(windows))]
    pub fn adopt(_child: &Child) -> Option<Self> {
        None
    }
}

#[cfg(windows)]
impl Drop for KillOnClose {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed exactly once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
