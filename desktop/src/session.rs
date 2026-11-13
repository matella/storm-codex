//! Windows session end (shutdown, restart, log off): a GUI process gets no console-control
//! event, only `WM_QUERYENDSESSION` / `WM_ENDSESSION` on its top-level windows. A hidden
//! top-level window on its own thread receives them and stops the stack before Windows ends
//! the process, so Postgres gets a fast (clean) shutdown instead of a kill.

#[cfg(windows)]
pub use imp::watch_session_end;

#[cfg(windows)]
mod imp {
    use std::sync::{Arc, OnceLock};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        TranslateMessage, MSG, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW,
    };

    use crate::Shell;

    static SHELL: OnceLock<Arc<Shell>> = OnceLock::new();

    /// Starts the watcher thread (once per process).
    pub fn watch_session_end(shell: Arc<Shell>) {
        if SHELL.set(shell).is_err() {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("session-end".into())
            .spawn(|| {
                // SAFETY: Win32 window creation and message loop on this thread only; every
                // pointer passed lives for the duration of the call.
                unsafe { run() }
            });
        if let (Err(e), Some(shell)) = (spawned, SHELL.get()) {
            shell.log(&format!("session-end watcher not started: {e}"));
        }
    }

    unsafe fn run() {
        let class: Vec<u16> = "StormCodexSessionWatcher\0".encode_utf16().collect();
        let instance = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        if RegisterClassW(&wc) == 0 {
            log("session-end watcher: RegisterClassW failed");
            return;
        }
        // Top-level (no parent, not HWND_MESSAGE) so it receives the session broadcasts;
        // never shown.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            log("session-end watcher: CreateWindowExW failed");
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            // Never veto the end of the session.
            WM_QUERYENDSESSION => 1,
            WM_ENDSESSION => {
                if wparam != 0 {
                    if let Some(shell) = SHELL.get() {
                        crate::stop_stack(shell, "Windows session end");
                    }
                }
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    fn log(msg: &str) {
        if let Some(shell) = SHELL.get() {
            shell.log(msg);
        }
    }
}
