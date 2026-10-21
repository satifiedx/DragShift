//! The Python UI process (DragShiftUI.exe): image and PDF work plus the editor windows.
//!
//! It is started on demand and talks JSON lines over its stdin/stdout. The daemon alone decides
//! when it goes away: after `ui_idle_seconds` with no job and no window it closes the pipe and
//! the worker exits, so between uses it costs no memory at all.

use crate::util;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_WORKER_EVENT: u32 = WM_APP + 50; // lparam = Box<(gen, line)>
pub const WM_WORKER_EXIT: u32 = WM_APP + 51;  // wparam = gen

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Worker {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    pub gen: u32,
    pub jobs: usize,
    pub windows: usize,
}

/// (program, args, working dir) for the UI process.
fn locate() -> Option<(PathBuf, Vec<String>, PathBuf)> {
    let dir = util::exe_dir();
    if let Some(py) = std::env::var_os("DRAGSHIFT_UI") {
        let root = std::env::var_os("DRAGSHIFT_ROOT").map(PathBuf::from).unwrap_or_else(|| dir.clone());
        return Some((PathBuf::from(py), vec!["-m".into(), "dragshift.worker".into()], root));
    }
    let exe = dir.join("ui").join("DragShiftUI.exe");
    if exe.is_file() {
        return Some((exe, vec![], dir));
    }
    // running from a source checkout (native/target/release): use the project's venv
    let mut d = dir.as_path();
    for _ in 0..5 {
        let py = d.join(".venv").join("Scripts").join("python.exe");
        if d.join("dragshift").join("worker.py").is_file() && py.is_file() {
            return Some((py, vec!["-m".into(), "dragshift.worker".into()], d.to_path_buf()));
        }
        d = d.parent()?;
    }
    None
}

impl Worker {
    pub fn new() -> Worker {
        Worker { child: None, stdin: None, gen: 0, jobs: 0, windows: 0 }
    }

    pub fn running(&self) -> bool {
        self.stdin.is_some()
    }

    pub fn busy(&self) -> bool {
        self.jobs > 0 || self.windows > 0
    }

    fn spawn(&mut self, hwnd: HWND) -> bool {
        let Some((prog, args, cwd)) = locate() else {
            crate::logf!("worker: DragShiftUI not found next to {}", util::exe_dir().display());
            return false;
        };
        let log = std::fs::OpenOptions::new().create(true).append(true).open(util::local_dir().join("ui-log.txt"));
        let mut c = Command::new(&prog);
        c.args(&args).current_dir(&cwd).stdin(Stdio::piped()).stdout(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .env("DRAGSHIFT_DAEMON", std::env::current_exe().unwrap_or_default())
            .env("DRAGSHIFT_LANG", crate::i18n::code());
        if let Some(f) = crate::media::ffmpeg() {
            c.env("DRAGSHIFT_FFMPEG", f);
        }
        match log {
            Ok(f) => c.stderr(Stdio::from(f)),
            Err(_) => c.stderr(Stdio::null()),
        };
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) => {
                crate::logf!("worker: could not start {}: {e}", prog.display());
                return false;
            }
        };
        self.gen += 1;
        let gen = self.gen;
        let out = child.stdout.take().unwrap();
        let h = hwnd.0 as isize;
        std::thread::Builder::new().name("worker-read".into()).stack_size(128 * 1024).spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                let b = Box::into_raw(Box::new((gen, line)));
                unsafe {
                    if PostMessageW(HWND(h as *mut _), WM_WORKER_EVENT, WPARAM(0), LPARAM(b as isize)).is_err() {
                        drop(Box::from_raw(b));
                    }
                }
            }
            unsafe {
                let _ = PostMessageW(HWND(h as *mut _), WM_WORKER_EXIT, WPARAM(gen as usize), LPARAM(0));
            }
        }).ok();
        crate::logf!("worker: started {} (pid {})", prog.display(), child.id());
        self.stdin = child.stdin.take();
        self.child = Some(child);
        self.jobs = 0;
        self.windows = 0;
        true
    }

    /// Send one message, starting the worker if needed. False if it can't be reached.
    pub fn send(&mut self, hwnd: HWND, msg: &Value) -> bool {
        let line = format!("{msg}\n");
        for _ in 0..2 {
            if self.stdin.is_none() && !self.spawn(hwnd) {
                return false;
            }
            if let Some(s) = self.stdin.as_mut() {
                if s.write_all(line.as_bytes()).and_then(|_| s.flush()).is_ok() {
                    return true;
                }
            }
            crate::logf!("worker: pipe broken, restarting");
            self.kill();
        }
        false
    }

    /// Close the pipe: the worker finishes up and exits by itself.
    pub fn stop(&mut self) {
        if self.stdin.take().is_some() {
            crate::logf!("worker: idle, letting it exit");
        }
        if let Some(mut c) = self.child.take() {
            // reap it in the background so it never lingers as a zombie handle
            std::thread::spawn(move || {
                for _ in 0..50 {
                    if let Ok(Some(_)) = c.try_wait() {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                let _ = c.kill();
                let _ = c.wait();
            });
        }
        self.jobs = 0;
        self.windows = 0;
    }

    pub fn kill(&mut self) {
        self.stdin = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.jobs = 0;
        self.windows = 0;
    }

    /// The process ended (crash or normal exit after `stop`).
    pub fn exited(&mut self, gen: u32) -> bool {
        if gen != self.gen || self.child.is_none() && self.stdin.is_none() {
            return false;
        }
        crate::logf!("worker: exited");
        self.kill();
        true
    }
}
