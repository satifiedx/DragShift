//! Per-pixel-alpha top-level windows: we paint a Canvas and hand it to UpdateLayeredWindow.
//! The DIB only exists while the window is visible.

use crate::gfx::Canvas;
use crate::util::wide;
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const CLASS: &str = "DragShiftSurface";

pub fn register_class(wndproc: WNDPROC) {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let name = wide(CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: wndproc,
            hInstance: hinst.into(),
            lpszClassName: PCWSTR(name.as_ptr()),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        std::mem::forget(name);
    }
}

pub fn create_window(ex_extra: WINDOW_EX_STYLE) -> HWND {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let name = wide(CLASS);
        let title = wide("DragShift");
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | ex_extra,
            PCWSTR(name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            0, 0, 1, 1,
            None, None, hinst, None,
        ).unwrap_or_default()
    }
}

pub struct Surface {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pub w: usize,
    pub h: usize,
}

impl Surface {
    pub fn new(w: usize, h: usize) -> Option<Surface> {
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(screen);
            ReleaseDC(None, screen);
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let old = SelectObject(dc, bmp);
            Some(Surface { dc, bmp, old, bits: bits as *mut u8, w, h })
        }
    }

    /// Copy the canvas in and update the window at screen position (x, y).
    pub fn present(&mut self, hwnd: HWND, cv: &Canvas, x: i32, y: i32, alpha: u8) {
        if cv.w != self.w || cv.h != self.h {
            return;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(cv.px.as_ptr() as *const u8, self.bits, self.w * self.h * 4);
            let pos = POINT { x, y };
            let size = SIZE { cx: self.w as i32, cy: self.h as i32 };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha,
                                        AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(hwnd, None, Some(&pos), Some(&size), self.dc, Some(&src), COLORREF(0),
                                        Some(&blend), ULW_ALPHA);
        }
        dump(hwnd, cv);
    }
}

/// Debugging aid: with DRAGSHIFT_DUMP=<folder>, keep the last frame of every surface as a raw
/// BGRA file (`<hwnd>-<w>x<h>.bgra`), so the menu and toasts can be checked without a screen.
fn dump(hwnd: HWND, cv: &Canvas) {
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    let Some(dir) = DIR.get_or_init(|| std::env::var_os("DRAGSHIFT_DUMP").map(Into::into)) else { return };
    let bytes = unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.w * cv.h * 4) };
    let _ = std::fs::write(dir.join(format!("{}-{}x{}.bgra", hwnd.0 as usize, cv.w, cv.h)), bytes);
}


impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp);
            let _ = DeleteDC(self.dc);
        }
    }
}

/// DPI scale (1.0 = 96 dpi) of the monitor at a screen point, and its work area.
pub fn monitor_at(x: i32, y: i32) -> (f32, RECT, RECT) {
    use windows::Win32::UI::HiDpi::*;
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        (dx as f32 / 96.0, mi.rcMonitor, mi.rcWork)
    }
}

/// Keep a window out of screen captures (so we can re-capture what is behind it).
pub fn exclude_from_capture(hwnd: HWND) -> bool {
    unsafe { SetWindowDisplayAffinity(hwnd, WINDOW_DISPLAY_AFFINITY(0x11)).is_ok() }
}
