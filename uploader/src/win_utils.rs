/// Windows-native show/hide via FindWindowW + ShowWindow.
/// Bypasses eframe's viewport commands which don't work on hidden windows.

#[cfg(windows)]
mod inner {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    type HWND = isize;
    type BOOL = i32;

    const SW_HIDE: i32 = 0;
    const SW_RESTORE: i32 = 9;

    extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> HWND;
        fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
        fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    }

    fn find_hwnd(title: &str) -> Option<HWND> {
        let wide: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
        let hwnd = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
        if hwnd == 0 {
            None
        } else {
            Some(hwnd)
        }
    }

    pub fn show_window(title: &str) {
        if let Some(hwnd) = find_hwnd(title) {
            unsafe {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
    }

    pub fn hide_window(title: &str) {
        if let Some(hwnd) = find_hwnd(title) {
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
}

#[cfg(not(windows))]
mod inner {
    pub fn show_window(_title: &str) {}
    pub fn hide_window(_title: &str) {}
}

pub use inner::*;

pub const WINDOW_TITLE: &str = "HotS Replay Client";
