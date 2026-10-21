//! Frame clock locked to the refresh of the monitor the animation is on (60, 144, 240 Hz…).
//!
//! A thread waits for that monitor's vertical blank (DXGI) and posts one WM_FRAME per refresh
//! to the main window. It never posts a second frame before the first was handled, so a busy UI
//! thread can't pile them up, and it sleeps on a condvar while nothing animates: idle costs
//! nothing. The DXGI objects are dropped again whenever it goes to sleep.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering::*};
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_FRAME: u32 = WM_APP + 80;

static WANT: Mutex<bool> = Mutex::new(false);
static WAKE: Condvar = Condvar::new();
static PENDING: AtomicBool = AtomicBool::new(false);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static MONITOR: AtomicIsize = AtomicIsize::new(0);

pub fn start(target: HWND) {
    TARGET.store(target.0 as isize, Relaxed);
    let _ = std::thread::Builder::new().name("vsync".into()).stack_size(64 * 1024).spawn(clock);
}

/// Keep frames coming (on the monitor `mon`), or let the clock sleep.
pub fn run(on: bool, mon: HMONITOR) {
    if on && !mon.is_invalid() {
        MONITOR.store(mon.0 as isize, Relaxed);
    }
    let mut w = WANT.lock().unwrap();
    if *w != on {
        *w = on;
        if on {
            WAKE.notify_one();
        }
    }
}

/// Something started moving: make sure frames are coming.
pub fn kick(mon: HMONITOR) {
    run(true, mon);
}

/// The UI thread took the last WM_FRAME: the next one may be posted.
pub fn handled() {
    PENDING.store(false, Relaxed);
}

pub fn monitor_of(x: i32, y: i32) -> HMONITOR {
    unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) }
}

fn output_for(mon: HMONITOR) -> Option<IDXGIOutput> {
    unsafe {
        let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut i = 0;
        while let Ok(ad) = f.EnumAdapters1(i) {
            let mut j = 0;
            while let Ok(o) = ad.EnumOutputs(j) {
                if o.GetDesc().map(|d| d.Monitor == mon).unwrap_or(false) {
                    return Some(o);
                }
                j += 1;
            }
            i += 1;
        }
        None
    }
}

/// The monitor's refresh rate, for when there is no vblank to wait on (remote desktop, odd drivers).
fn period_of(mon: HMONITOR) -> Duration {
    unsafe {
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        let hz = if GetMonitorInfoW(mon, &mut mi as *mut _ as *mut MONITORINFO).as_bool()
            && EnumDisplaySettingsW(windows::core::PCWSTR(mi.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm).as_bool() {
            dm.dmDisplayFrequency
        } else {
            60
        };
        Duration::from_micros(1_000_000 / hz.clamp(30, 1000) as u64)
    }
}

fn clock() {
    let mut out: Option<(isize, Option<IDXGIOutput>, Duration)> = None;
    loop {
        {
            let mut w = WANT.lock().unwrap();
            if !*w {
                out = None;
                while !*w {
                    w = WAKE.wait(w).unwrap();
                }
            }
        }
        let mon = MONITOR.load(Relaxed);
        if out.as_ref().map(|o| o.0) != Some(mon) {
            let m = HMONITOR(mon as *mut _);
            out = Some((mon, output_for(m), period_of(m)));
        }
        let (_, o, period) = out.as_ref().unwrap();
        let waited = o.as_ref().map(|o| unsafe { o.WaitForVBlank().is_ok() }).unwrap_or(false);
        if !waited {
            std::thread::sleep(*period);
        }
        if !PENDING.swap(true, Relaxed) {
            let h = TARGET.load(Relaxed);
            unsafe {
                if PostMessageW(HWND(h as *mut _), WM_FRAME, WPARAM(0), LPARAM(0)).is_err() {
                    PENDING.store(false, Relaxed);
                }
            }
        }
    }
}
