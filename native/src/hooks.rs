//! The trigger: "left button held + mouse moved + Shift pressed" anywhere on screen.
//!
//! Low-level mouse and keyboard hooks on their own thread replace the old 15 ms polling timer:
//! when nothing moves, nothing runs. The callbacks only update a few atomics and post a message
//! to the UI thread, so they always return in microseconds (Windows drops slow hooks).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering::*};
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRIGGER_SHOW: u32 = WM_APP + 1;   // lparam = packed x,y
pub const WM_TRIGGER_TOGGLE: u32 = WM_APP + 2;
pub const WM_TRIGGER_UP: u32 = WM_APP + 3;     // left button released
pub const WM_TRIGGER_ESC: u32 = WM_APP + 4;
pub const WM_TRIGGER_DOWN: u32 = WM_APP + 5;   // a click anywhere while the menu is up; lparam = x,y

const DRAG_THRESHOLD: i32 = 10;

static TARGET: AtomicIsize = AtomicIsize::new(0);
pub static ENABLED: AtomicBool = AtomicBool::new(true);
/// The UI sets this while the menu is up (trigger key then flips rings instead of opening).
pub static MENU_UP: AtomicBool = AtomicBool::new(false);
static KEY: AtomicU32 = AtomicU32::new(0x10); // VK_SHIFT / VK_CONTROL / VK_MENU
static DOWN: AtomicBool = AtomicBool::new(false);
static FIRED: AtomicBool = AtomicBool::new(false);
static DOWN_X: AtomicI32 = AtomicI32::new(0);
static DOWN_Y: AtomicI32 = AtomicI32::new(0);
static MOVED: AtomicBool = AtomicBool::new(false);
static KEY_HELD: AtomicBool = AtomicBool::new(false);
static CUR_X: AtomicI32 = AtomicI32::new(0);
static CUR_Y: AtomicI32 = AtomicI32::new(0);
/// Timestamp (GetTickCount clock) of the last event either hook saw.
static LAST_EVENT: AtomicU32 = AtomicU32::new(0);

pub fn set_key(name: &str) {
    KEY.store(match name { "ctrl" => 0x11, "alt" => 0x12, _ => 0x10 }, Relaxed);
}

fn is_trigger_vk(vk: u32) -> bool {
    match KEY.load(Relaxed) {
        0x11 => matches!(vk, 0x11 | 0xA2 | 0xA3),
        0x12 => matches!(vk, 0x12 | 0xA4 | 0xA5),
        _ => matches!(vk, 0x10 | 0xA0 | 0xA1),
    }
}

fn post(msg: u32, lp: isize) {
    let h = TARGET.load(Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(HWND(h as *mut _), msg, WPARAM(0), LPARAM(lp));
        }
    }
}

pub fn pack(x: i32, y: i32) -> isize {
    ((x as u32 as u64) | ((y as u32 as u64) << 32)) as isize
}

pub fn unpack(lp: isize) -> (i32, i32) {
    let v = lp as u64;
    (v as u32 as i32, (v >> 32) as u32 as i32)
}

fn try_fire() {
    if ENABLED.load(Relaxed) && DOWN.load(Relaxed) && MOVED.load(Relaxed) && !FIRED.load(Relaxed)
        && !MENU_UP.load(Relaxed) {
        FIRED.store(true, Relaxed);
        post(WM_TRIGGER_SHOW, pack(CUR_X.load(Relaxed), CUR_Y.load(Relaxed)));
    }
}

/// The event's own timestamp (the same clock GetLastInputInfo uses).
unsafe fn info_time(lp: LPARAM, mouse: bool) -> u32 {
    if mouse {
        (*(lp.0 as *const MSLLHOOKSTRUCT)).time
    } else {
        (*(lp.0 as *const KBDLLHOOKSTRUCT)).time
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    LAST_EVENT.store(info_time(lp, true), Relaxed);
    if code >= 0 {
        let info = &*(lp.0 as *const MSLLHOOKSTRUCT);
        let (x, y) = (info.pt.x, info.pt.y);
        CUR_X.store(x, Relaxed);
        CUR_Y.store(y, Relaxed);
        match wp.0 as u32 {
            WM_RBUTTONDOWN | WM_MBUTTONDOWN if MENU_UP.load(Relaxed) => post(WM_TRIGGER_DOWN, pack(x, y)),
            WM_LBUTTONDOWN => {
                if MENU_UP.load(Relaxed) {
                    post(WM_TRIGGER_DOWN, pack(x, y));
                }
                DOWN.store(true, Relaxed);
                FIRED.store(false, Relaxed);
                MOVED.store(false, Relaxed);
                DOWN_X.store(x, Relaxed);
                DOWN_Y.store(y, Relaxed);
            }
            WM_LBUTTONUP => {
                DOWN.store(false, Relaxed);
                FIRED.store(false, Relaxed);
                if MENU_UP.load(Relaxed) {
                    post(WM_TRIGGER_UP, 0);
                }
            }
            WM_MOUSEMOVE if DOWN.load(Relaxed) && !FIRED.load(Relaxed) => {
                if !MOVED.load(Relaxed)
                    && (x - DOWN_X.load(Relaxed)).abs() + (y - DOWN_Y.load(Relaxed)).abs() >= DRAG_THRESHOLD {
                    MOVED.store(true, Relaxed);
                }
                // key pressed first, then the drag started
                if MOVED.load(Relaxed) && KEY_HELD.load(Relaxed) {
                    try_fire();
                }
            }
            _ => {}
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

unsafe extern "system" fn key_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    LAST_EVENT.store(info_time(lp, false), Relaxed);
    if code >= 0 {
        let info = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let vk = info.vkCode;
        let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        if is_trigger_vk(vk) {
            let was = KEY_HELD.swap(down, Relaxed);
            if down && !was {
                if MENU_UP.load(Relaxed) {
                    post(WM_TRIGGER_TOGGLE, 0);
                } else {
                    try_fire();
                }
            }
        } else if vk == VK_ESCAPE.0 as u32 && down && MENU_UP.load(Relaxed) {
            post(WM_TRIGGER_ESC, 0);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Is the left mouse button physically down right now?
pub fn left_down() -> bool {
    DOWN.load(Relaxed)
}

unsafe fn install() -> (Option<HHOOK>, Option<HHOOK>) {
    let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0);
    let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), None, 0);
    if m.is_err() || k.is_err() {
        crate::logf!("hooks: failed to install (mouse {:?}, keyboard {:?})", m.is_ok(), k.is_ok());
    }
    (m.ok(), k.ok())
}

/// How long the system may see input our hooks never got before they count as lost.
const LOST_AFTER_MS: u32 = 2000;
const WATCH_MS: u32 = 3000;

pub fn start(target: HWND) {
    TARGET.store(target.0 as isize, Relaxed);
    std::thread::Builder::new()
        .name("hooks".into())
        .stack_size(64 * 1024)
        .spawn(|| unsafe {
            let mut hooks = install();
            let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            if GetLastInputInfo(&mut li).as_bool() {
                LAST_EVENT.store(li.dwTime, Relaxed); // input from before we started isn't ours to see
            }
            // Windows silently unhooks a low-level hook that once answered too slowly (a busy
            // boot is enough), and then only the right-click menu works. Watch for input that
            // never reached us and hook again.
            let _ = SetTimer(None, 0, WATCH_MS, None);
            let mut retry_at_event = u32::MAX;
            let mut quiet_since = std::time::Instant::now();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_TIMER && msg.hwnd.0.is_null() {
                    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
                    let seen = LAST_EVENT.load(Relaxed);
                    if !GetLastInputInfo(&mut li).as_bool() || (li.dwTime.wrapping_sub(seen) as i32) < LOST_AFTER_MS as i32 {
                        continue;
                    }
                    // still nothing since the last re-hook: input may be going to an elevated
                    // window our hooks can't see, so only try again once a minute
                    if seen == retry_at_event && quiet_since.elapsed().as_secs() < 60 {
                        continue;
                    }
                    crate::logf!("hooks: input for {} ms never reached the hooks, installing them again",
                                 li.dwTime.wrapping_sub(seen));
                    for h in [hooks.0, hooks.1].into_iter().flatten() {
                        let _ = UnhookWindowsHookEx(h);
                    }
                    hooks = install();
                    retry_at_event = seen;
                    quiet_since = std::time::Instant::now();
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .ok();
}
