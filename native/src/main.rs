//! DragShift: Shift+drag any file to convert it. This is the always-running part: a few MB,
//! no polling, no Python until an editor window is needed.

#![windows_subsystem = "windows"]

mod actions;
mod app;
mod gfx;
mod hooks;
mod i18n;
mod jobs;
mod layered;
mod media;
mod menu;
mod selftest;
mod settings;
mod shape;
mod toast;
mod tray;
mod theme;
mod util;
mod vsync;
mod worker;

use std::path::PathBuf;
use util::wide;
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Hand the request to the copy that is already running. True if one answered.
fn forward(kind: usize, files: &[PathBuf]) -> bool {
    let cls = wide(app::MAIN_CLASS);
    for _ in 0..40 {
        let hwnd = unsafe { FindWindowW(PCWSTR(cls.as_ptr()), None) }.unwrap_or_default();
        if !hwnd.0.is_null() {
            let text: Vec<u16> = files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")
                .encode_utf16().collect();
            let cds = COPYDATASTRUCT { dwData: kind, cbData: (text.len() * 2) as u32, lpData: text.as_ptr() as *mut _ };
            unsafe {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let mut res = 0usize;
                SendMessageTimeoutW(hwnd, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize),
                                    SMTO_ABORTIFHUNG, 5000, Some(&mut res));
            }
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--selftest") => {
            util::init_log();
            std::process::exit(selftest::selftest(args.iter().any(|a| a == "--keep")));
        }
        Some("--run") if args.len() >= 3 => {
            util::init_log();
            let files: Vec<PathBuf> = args[2..].iter().map(PathBuf::from).collect();
            std::process::exit(selftest::run_one(&args[1], &files));
        }
        _ => {}
    }
    let mut quiet = false;
    let mut quit = false;
    let mut open_settings = false;
    let mut files = Vec::new();
    let mut action: Option<String> = None;
    let mut it = std::env::args_os().skip(1);
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--do") => action = it.next().and_then(|k| k.into_string().ok()),
            Some("--quiet") => quiet = true,
            Some("--quit") => quit = true,
            Some("--settings") => open_settings = true,
            Some(s) if s.starts_with("--") => {}
            _ => files.push(PathBuf::from(a)),
        }
    }
    let name = wide("Local\\DragShift.Daemon");
    let _mutex = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already {
        if let (Some(key), false) = (&action, files.is_empty()) {
            let mut payload = vec![PathBuf::from(key)];
            payload.extend(files);
            forward(app::COPY_ACTION, &payload);
            return;
        }
        let kind = if quit {
            app::COPY_QUIT
        } else if open_settings {
            app::COPY_SETTINGS
        } else if !files.is_empty() {
            app::COPY_FILES
        } else if quiet {
            return;
        } else {
            app::COPY_SHOW
        };
        forward(kind, &files);
        return;
    }
    if quit {
        return;
    }
    util::init_log();
    settings::load();
    app::run(files, quiet, open_settings, action);
}
