//! Screen snapshots for the glass, taken just before a surface appears.
//!
//! While dragging, Windows draws a translucent picture of the file under the cursor. It is hidden
//! for the instant of the capture (any layered, click-through top-level window of another process
//! near the cursor, plus the known drag-image classes) and the spot is inpainted afterwards in
//! case the compositor had not dropped it yet.

use crate::gfx::blur_gray;
use crate::liquid::Backdrop;
use crate::util::from_wide;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn grab(x: i32, y: i32, w: i32, h: i32) -> Option<Backdrop> {
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        let ok = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT).is_ok();
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        SelectObject(mem, old);
        let lines = if ok {
            GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS)
        } else {
            0
        };
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if lines == 0 {
            return None;
        }
        let px = buf.chunks_exact(4).map(|p| [p[2], p[1], p[0]]).collect();
        Some(Backdrop { w: w as usize, h: h as usize, px })
    }
}

const DRAG_CLASSES: [&str; 3] = ["SysDragImage", "DragImageWindow", "DragWindow"];

struct Sweep {
    pt: POINT,
    me: u32,
    found: Vec<(HWND, RECT)>,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
    let s = &mut *(lp.0 as *mut Sweep);
    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }
    let mut r = RECT::default();
    let _ = GetWindowRect(hwnd, &mut r);
    let near = r.left - 40 <= s.pt.x && s.pt.x <= r.right + 40 && r.top - 40 <= s.pt.y && s.pt.y <= r.bottom + 40;
    if !near || r.right - r.left > 900 || r.bottom - r.top > 900 {
        return TRUE;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == s.me {
        return TRUE;
    }
    let mut cls = [0u16; 64];
    GetClassNameW(hwnd, &mut cls);
    let cls = from_wide(&cls);
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let drag_like = ex & WS_EX_LAYERED.0 != 0 && ex & WS_EX_TRANSPARENT.0 != 0;
    if DRAG_CLASSES.contains(&cls.as_str()) || drag_like {
        s.found.push((hwnd, r));
    }
    TRUE
}

/// Capture the rect with drag images hidden. Returns the backdrop and the screen rects that were
/// hidden (to inpaint).
pub fn grab_without_drag_image(x: i32, y: i32, w: i32, h: i32) -> Option<Backdrop> {
    let mut sweep = Sweep { pt: POINT::default(), me: unsafe { GetCurrentProcessId() }, found: Vec::new() };
    unsafe {
        let _ = GetCursorPos(&mut sweep.pt);
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut sweep as *mut _ as isize));
        for (hw, _) in &sweep.found {
            let _ = ShowWindow(*hw, SW_HIDE);
        }
        if !sweep.found.is_empty() {
            let _ = GdiFlush();
            let _ = DwmFlush();
            let _ = DwmFlush();
        }
    }
    let shot = grab(x, y, w, h);
    unsafe {
        for (hw, _) in &sweep.found {
            let _ = ShowWindow(*hw, SW_SHOWNA);
        }
    }
    let mut shot = shot?;
    if !sweep.found.is_empty() {
        let rects: Vec<(i32, i32, i32, i32)> =
            sweep.found.iter().map(|(_, r)| (r.left - x, r.top - y, r.right - x, r.bottom - y)).collect();
        inpaint(&mut shot, &rects, 6);
    }
    Some(shot)
}

/// Fill rectangles from their surroundings (normalised convolution) with a feathered seam:
/// a smooth, colour-matched patch, invisible once refracted and blurred by the glass.
pub fn inpaint(img: &mut Backdrop, rects: &[(i32, i32, i32, i32)], pad: i32) {
    let (w, h) = (img.w, img.h);
    let mut mask = vec![1f32; w * h];
    let mut any = false;
    for &(x0, y0, x1, y1) in rects {
        let (x0, y0) = ((x0 - pad).max(0) as usize, (y0 - pad).max(0) as usize);
        let (x1, y1) = (((x1 + pad).max(0) as usize).min(w), ((y1 + pad).max(0) as usize).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                mask[y * w + x] = 0.0;
                any = true;
            }
        }
    }
    if !any {
        return;
    }
    let mut num: Vec<[f32; 3]> = img.px.iter().zip(&mask)
        .map(|(p, m)| [p[0] as f32 * m, p[1] as f32 * m, p[2] as f32 * m]).collect();
    crate::gfx::blur_rgb(&mut num, w, h, 28.0);
    let mut den = mask.clone();
    blur_gray(&mut den, w, h, 28.0);
    let mut fill: Vec<[f32; 3]> = num.iter().zip(&den).map(|(n, d)| {
        let d = d.max(1e-3);
        [n[0] / d, n[1] / d, n[2] / d]
    }).collect();
    crate::gfx::blur_rgb(&mut fill, w, h, 6.0);
    let mut soft = mask.clone();
    blur_gray(&mut soft, w, h, 12.0);
    for i in 0..w * h {
        let s = soft[i].min(mask[i]);
        let p = &mut img.px[i];
        for c in 0..3 {
            p[c] = (p[c] as f32 * s + fill[i][c] * (1.0 - s)).clamp(0.0, 255.0) as u8;
        }
    }
}
