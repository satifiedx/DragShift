//! The daemon: one hidden window, the tray icon, the trigger, the radial menu, notifications,
//! media jobs and the on-demand Python worker, all driven from the UI thread's message loop.

use crate::actions::{self, Cat};
use crate::hooks::{self, *};
use crate::i18n::{t, tf};
use crate::jobs::{self, JobDone, WM_JOB_DONE, WM_JOB_PROGRESS};
use crate::media::{self, Op};
use crate::menu::{Activation, Menu, WM_MENU_ACTIVATE, WM_MENU_RENDERED};
use crate::settings;
use crate::toast::{Outcome, Thumb, Toasts, WM_TOAST_THUMB};
use crate::util::{self, wide};
use crate::vsync::{self, WM_FRAME};
use crate::worker::{Worker, WM_WORKER_EVENT, WM_WORKER_EXIT};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Arc;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW, COPYDATASTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const MAIN_CLASS: &str = "DragShiftDaemon";
const WM_TRAY: u32 = WM_APP + 60;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_TRIM: usize = 10;
const TIMER_WORKER_IDLE: usize = 11;
const TIMER_FILES: usize = 12;

pub const COPY_FILES: usize = 1;
pub const COPY_QUIT: usize = 2;
pub const COPY_SETTINGS: usize = 3;
pub const COPY_SHOW: usize = 4;
/// first line: a menu key, then the files
pub const COPY_ACTION: usize = 5;

const ID_ENABLED: usize = 1;
const ID_OPEN: usize = 2;
const ID_SETTINGS: usize = 3;
const ID_HELP: usize = 4;
const ID_ABOUT: usize = 5;
const ID_QUIT: usize = 9;
const ID_SHOT: usize = 100;    // + index into IMAGE_TARGETS: to Pictures\Screenshots
const ID_SHOT_TO: usize = 200; // + index into IMAGE_TARGETS: to a folder picked now

pub const REPO_URL: &str = "https://github.com/satifiedx/DragShift";

static MAIN: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct JobInfo {
    cancel: Option<Arc<AtomicBool>>, // native jobs; worker jobs are cancelled over the pipe
    files: usize,
}

pub struct App {
    pub main_hwnd: HWND,
    pub menu: Menu,
    pub toasts: Toasts,
    worker: Worker,
    jobs: HashMap<u64, JobInfo>,
    next_job: u64,
    /// background saves started by an editor window: the worker's tag → our job id
    tags: HashMap<String, u64>,
}

thread_local! {
    /// Explorer's right-click entry starts us once per selected file: gather them for one menu
    static PENDING: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };

    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Run `f` on the app unless it is already borrowed further up the stack (a window message sent
/// synchronously from inside another handler); those messages just get default handling.
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Give unused memory back a moment after the last window closes.
pub fn schedule_trim() {
    let h = MAIN.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            SetTimer(HWND(h as *mut _), TIMER_TRIM, 1500, None);
        }
    }
}

fn main_hwnd() -> HWND {
    HWND(MAIN.load(Ordering::Relaxed) as *mut _)
}

// ------------------------------------------------------------------ startup

/// `action`: run this menu key on `files` right away (`DragShift.exe --do convert:MP3 a.wav`).
pub fn run(files: Vec<PathBuf>, quiet: bool, settings_now: bool, action: Option<String>) {
    unsafe {
        let _ = windows::Win32::System::Ole::OleInitialize(None);
        crate::tray::allow_dark_menus();
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let cls = wide(MAIN_CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(main_proc),
            hInstance: hinst.into(),
            lpszClassName: PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        crate::layered::register_class(Some(surface_proc));
        let title = wide("DragShift");
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), PCWSTR(cls.as_ptr()), PCWSTR(title.as_ptr()), WS_POPUP,
                                   0, 0, 0, 0, None, None, hinst, None).unwrap_or_default();
        MAIN.store(hwnd.0 as isize, Ordering::Relaxed);
        let tb = wide("TaskbarCreated");
        TASKBAR_CREATED.store(RegisterWindowMessageW(PCWSTR(tb.as_ptr())), Ordering::Relaxed);

        let s = settings::get();
        hooks::set_key(&s.trigger);
        hooks::ENABLED.store(s.enabled, Ordering::Relaxed);
        let app = App { main_hwnd: hwnd, menu: Menu::new(), toasts: Toasts::new(), worker: Worker::new(),
                        jobs: HashMap::new(), next_job: 1, tags: HashMap::new() };
        APP.with(|a| *a.borrow_mut() = Some(app));
        crate::tray::add(hwnd, WM_TRAY);
        hooks::start(hwnd);
        vsync::start(hwnd);

        if settings_now {
            open_settings();
        } else if let (Some(key), false) = (action, files.is_empty()) {
            with_app(|a| a.run_item(key, files));
        } else if !files.is_empty() {
            // more copies may be starting for the rest of an Explorer selection
            PENDING.with(|p| *p.borrow_mut() = files);
            SetTimer(hwnd, TIMER_FILES, 300, None);
        } else if !quiet || !s.welcomed {
            welcome(!s.welcomed);
        }
        schedule_trim();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        crate::tray::remove(hwnd);
        with_app(|a| {
            for j in a.jobs.values() {
                if let Some(c) = &j.cancel {
                    c.store(true, Ordering::Relaxed);
                }
            }
            a.worker.kill();
        });
        // give cancelled ffmpeg runs a moment to delete their temp files
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn welcome(first: bool) {
    let trig = trigger_name();
    with_app(|a| {
        if first {
            a.toasts.info(t("DragShift is ready"), &tf("Start dragging any file, then press {}. The menu opens under the cursor.", &[&trig]), 9000);
        } else {
            a.toasts.info(t("DragShift is running"), &tf("Drag a file and press {}. Settings are in the tray icon.", &[&trig]), 5000);
        }
    });
    if first {
        settings::update(|s| s.welcomed = true);
        // the installer asked about autostart already; a portable copy turns it on once
        if !util::exe_dir().join("unins000.exe").is_file() {
            settings::set_autostart(true);
        }
    }
}

fn trigger_name() -> String {
    match settings::get().trigger.as_str() {
        "ctrl" => "Ctrl".into(),
        "alt" => "Alt".into(),
        _ => "Shift".into(),
    }
}

/// Files from the command line, Send To or the tray: the menu opens at the cursor in click mode.
fn open_files(files: Vec<PathBuf>) {
    let files: Vec<PathBuf> = files.into_iter().filter(|p| p.is_file()).collect();
    if files.is_empty() {
        return;
    }
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    with_app(|a| {
        a.menu.close();
        a.menu.popup(pt.x, pt.y, Some(files));
    });
}

// ------------------------------------------------------------------ running an item

impl App {
    fn new_job(&mut self, cancel: Option<Arc<AtomicBool>>, files: usize) -> u64 {
        let id = self.next_job;
        self.next_job += 1;
        self.jobs.insert(id, JobInfo { cancel, files });
        id
    }

    fn names(paths: &[PathBuf]) -> String {
        if paths.len() == 1 {
            paths[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            tf("{} files", &[&paths.len().to_string()])
        }
    }

    pub fn run_item(&mut self, key: String, paths: Vec<PathBuf>) {
        let Some(cat) = paths.first().and_then(|p| actions::category(p)) else { return };
        let paths: Vec<PathBuf> = paths.into_iter().filter(|p| actions::category(p) == Some(cat)).collect();
        crate::logf!("run {key} on {} file(s)", paths.len());
        if cat.is_media() {
            if let Some(op) = Op::from_key(&key) {
                self.start_media(op, paths);
                return;
            }
        }
        self.start_worker(&key, cat, paths);
    }

    fn start_media(&mut self, op: Op, paths: Vec<PathBuf>) {
        if media::ffmpeg().is_none() {
            self.toasts.error(t("ffmpeg is missing"), t("Video and audio need ffmpeg, and DragShift couldn't find it. Reinstall DragShift."));
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let id = self.new_job(Some(cancel.clone()), paths.len());
        self.toasts.start(id, &op.working_text(), &Self::names(&paths), op.cancellable() || paths.len() > 1);
        jobs::spawn_media(id, op, paths, cancel, self.main_hwnd);
    }

    fn start_worker(&mut self, key: &str, cat: Cat, paths: Vec<PathBuf>) {
        let (kind, name) = key.split_once(':').unwrap_or(("", key));
        let window = match cat {
            Cat::Image => kind == "tool",
            Cat::Pdf => matches!(name, "Metadata" | "Protect"),
            Cat::Video | Cat::Audio => true,
            _ => false,
        };
        let title = if window {
            tf("Opening {}…", &[t(name)])
        } else if kind == "convert" {
            tf("Converting to {}…", &[name])
        } else {
            t("Working…").into()
        };
        let id = self.new_job(None, paths.len());
        self.toasts.start(id, &title, &Self::names(&paths), false);
        let msg = json!({"op": "run", "job": id, "key": key, "cat": cat.name(),
                         "paths": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()});
        self.worker_send(msg, true);
        if !self.jobs.contains_key(&id) {
            return;
        }
        self.worker.jobs += 1;
        self.idle_check();
    }

    fn save_clipboard(&mut self, fmt: &str, folder: Option<PathBuf>) {
        let id = self.new_job(None, 1);
        self.toasts.start(id, t("Saving screenshot…"), fmt, false);
        let folder = folder.map(|d| d.display().to_string()).unwrap_or_default();
        self.worker_send(json!({"op": "clipboard", "job": id, "format": fmt, "folder": folder}), true);
        if !self.jobs.contains_key(&id) {
            return;
        }
        self.worker.jobs += 1;
        self.idle_check();
    }

    fn worker_send(&mut self, msg: Value, is_job: bool) -> bool {
        unsafe {
            // let the editor windows come to the front
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }
        if self.worker.send(self.main_hwnd, &msg) {
            return true;
        }
        let job = msg.get("job").and_then(Value::as_u64);
        if let (true, Some(j)) = (is_job, job) {
            self.jobs.remove(&j);
            self.toasts.job_error(j, t("Couldn't start the editor"), t("Part of DragShift is missing. Reinstall DragShift."));
        } else {
            self.toasts.error(t("Couldn't start the editor"), t("Part of DragShift is missing. Reinstall DragShift."));
        }
        false
    }

    fn cancel(&mut self, job: u64) {
        let Some(j) = self.jobs.get(&job) else { return };
        match &j.cancel {
            Some(c) => c.store(true, Ordering::Relaxed),
            None => {
                self.worker_send(json!({"op": "cancel", "job": job}), false);
            }
        }
        self.toasts.job_progress(job, 1.0, Some(t("Cancelling…").into()));
    }

    // ------------------------------------------------------------ results

    fn on_media_done(&mut self, d: JobDone) {
        self.jobs.remove(&d.id);
        match d.result {
            Ok(o) => self.toasts.job_done(d.id, o),
            Err(e) if media::is_cancel(&e) => self.toasts.job_info(d.id, t("Cancelled"), t("Nothing was changed."), 1800),
            Err(e) if media::note_text(&e).is_some() => {
                self.toasts.job_info(d.id, t("Nothing to do"), media::note_text(&e).unwrap_or_default(), 4500)
            }
            Err(e) => self.toasts.job_error(d.id, t("Couldn't finish"), &e),
        }
    }

    fn on_progress(&mut self, job: u64, lp: isize) {
        let (i, frac) = jobs::unpack_progress(lp);
        let files = self.jobs.get(&job).map(|j| j.files).unwrap_or(1);
        let detail = (files > 1).then(|| tf("File {} of {}", &[&(i + 1).to_string(), &files.to_string()]));
        self.toasts.job_progress(job, frac, detail);
    }

    fn worker_job_finished(&mut self, job: Option<u64>) {
        if let Some(j) = job {
            if self.jobs.remove(&j).is_some() {
                self.worker.jobs = self.worker.jobs.saturating_sub(1);
            }
        }
    }

    fn on_worker(&mut self, v: Value) {
        let ev = v.get("ev").and_then(Value::as_str).unwrap_or("");
        let tag = v.get("tag").and_then(Value::as_str).map(str::to_string);
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        if let (Some(tag), "start") = (&tag, ev) {
            let id = self.new_job(None, 1);
            self.tags.insert(tag.clone(), id);
            self.toasts.start(id, &s("title"), &s("detail"), false);
            self.worker.jobs += 1;
            self.idle_check();
            return;
        }
        let job = match &tag {
            Some(t) if matches!(ev, "done" | "error" | "info") => self.tags.remove(t),
            Some(t) => self.tags.get(t).copied(),
            None => v.get("job").and_then(Value::as_u64),
        };
        match ev {
            "progress" => {
                if let Some(j) = job {
                    let frac = v.get("frac").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    let detail = v.get("detail").and_then(Value::as_str).map(str::to_string);
                    self.toasts.job_progress(j, frac, detail);
                }
            }
            "done" => {
                let outs: Vec<PathBuf> = v.get("outs").and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(PathBuf::from).collect()).unwrap_or_default();
                let o = Outcome { title: s("title"), detail: s("detail"), good: s("good"), outs };
                match job {
                    Some(j) if self.jobs.contains_key(&j) => self.toasts.job_done(j, o),
                    _ => self.toasts.done(o),
                }
                self.worker_job_finished(job);
            }
            "error" => {
                let title = if s("title").is_empty() { t("Couldn't finish").to_string() } else { s("title") };
                match job {
                    Some(j) if self.jobs.contains_key(&j) => self.toasts.job_error(j, &title, &s("detail")),
                    _ => self.toasts.error(&title, &s("detail")),
                }
                self.worker_job_finished(job);
            }
            "info" => {
                let ms = v.get("ms").and_then(Value::as_u64).unwrap_or(3000);
                match job {
                    Some(j) if self.jobs.contains_key(&j) => self.toasts.job_info(j, &s("title"), &s("detail"), ms),
                    _ => self.toasts.info(&s("title"), &s("detail"), ms),
                }
                self.worker_job_finished(job);
            }
            "window" | "closed" => {
                if let Some(j) = job {
                    self.toasts.job_forget(j);
                }
                self.worker_job_finished(job);
            }
            "windows" => {
                self.worker.windows = v.get("n").and_then(Value::as_u64).unwrap_or(0) as usize;
            }
            "media" => {
                // an editor window (Trim, Crop, Speed) chose the settings; ffmpeg runs here
                let path = PathBuf::from(s("path"));
                let o = v.get("op").cloned().unwrap_or(Value::Null);
                let f = |k: &str| o.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                let op = match o.get("kind").and_then(Value::as_str) {
                    Some("trim") => Some(Op::Trim { start: f("start"), end: f("end"), fade_in: f("fade_in"),
                                                    fade_out: f("fade_out"), fast: o.get("fast").and_then(Value::as_bool).unwrap_or(false) }),
                    Some("crop") => Some(Op::Crop { x: f("x") as u32, y: f("y") as u32, w: f("w") as u32, h: f("h") as u32 }),
                    Some("speed") => Some(Op::Speed { factor: f("factor"),
                                                      keep_pitch: o.get("keep_pitch").and_then(Value::as_bool).unwrap_or(true) }),
                    Some("frame") => Some(Op::Frame(Some(f("at")))),
                    Some("encode") => serde_json::from_value(o.clone()).ok().map(Op::Encode),
                    _ => None,
                };
                match op {
                    Some(op) if path.is_file() => self.start_media(op, vec![path]),
                    _ => crate::logf!("worker: bad media request {v}"),
                }
            }
            "settings" => apply_settings(),
            "quit" => unsafe { PostQuitMessage(0) },
            "log" => crate::logf!("ui: {}", s("text")),
            _ => crate::logf!("worker: unknown event {v}"),
        }
        self.idle_check();
    }

    fn on_worker_exit(&mut self, gen: u32) {
        if !self.worker.exited(gen) {
            return;
        }
        self.tags.clear();
        // anything still waiting on the worker won't get an answer now
        let orphans: Vec<u64> = self.jobs.iter().filter(|(_, j)| j.cancel.is_none()).map(|(id, _)| *id).collect();
        for id in orphans {
            self.jobs.remove(&id);
            self.toasts.job_error(id, t("Couldn't finish"), t("The editor closed unexpectedly. Please try again."));
        }
        self.idle_check();
    }

    /// Arm (or disarm) the timer that lets an idle worker go.
    fn idle_check(&mut self) {
        unsafe {
            if self.worker.running() && !self.worker.busy() {
                let secs = settings::get().ui_idle_seconds.clamp(5, 3600);
                SetTimer(self.main_hwnd, TIMER_WORKER_IDLE, secs * 1000, None);
            } else {
                let _ = KillTimer(self.main_hwnd, TIMER_WORKER_IDLE);
            }
        }
    }
}

fn apply_settings() {
    settings::load();
    let s = settings::get();
    hooks::set_key(&s.trigger);
    hooks::ENABLED.store(s.enabled, Ordering::Relaxed);
    crate::i18n::reload();
    crate::tray::update_tip(main_hwnd(), s.enabled);
}

// ------------------------------------------------------------------ tray

fn tray_menu(hwnd: HWND) {
    let enabled = settings::get().enabled;
    let choice = unsafe {
        let m = CreatePopupMenu().unwrap_or_default();
        let add = |id: usize, text: &str, flags: MENU_ITEM_FLAGS| {
            let w = wide(text);
            let _ = AppendMenuW(m, flags, id, PCWSTR(w.as_ptr()));
        };
        add(ID_ENABLED, t("Enabled"), if enabled { MF_CHECKED } else { MF_UNCHECKED });
        add(0, "", MF_SEPARATOR);
        add(ID_OPEN, t("Open a file…"), MF_STRING);
        let grayed = if clipboard_has_picture() { MF_ENABLED } else { MF_GRAYED };
        for (base, label) in [(ID_SHOT, t("Save screenshot to Pictures")), (ID_SHOT_TO, t("Save screenshot to a folder…"))] {
            let formats = CreatePopupMenu().unwrap_or_default();
            for (i, f) in actions::IMAGE_TARGETS.iter().enumerate() {
                let w = wide(&format!("{f}	{}", actions::format_hint(f)));
                let _ = AppendMenuW(formats, MF_STRING, base + i, PCWSTR(w.as_ptr()));
            }
            let w = wide(label);
            let _ = AppendMenuW(m, MF_POPUP | grayed, formats.0 as usize, PCWSTR(w.as_ptr()));
        }
        add(ID_SETTINGS, t("Settings…"), MF_STRING);
        add(ID_HELP, t("How to use"), MF_STRING);
        add(ID_ABOUT, t("About DragShift"), MF_STRING);
        add(0, "", MF_SEPARATOR);
        add(ID_QUIT, t("Quit"), MF_STRING);
        let _ = SetMenuDefaultItem(m, ID_OPEN as u32, 0);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let r = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, None);
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        r.0 as usize
    };
    match choice {
        ID_ENABLED => {
            settings::update(|s| s.enabled = !enabled);
            apply_settings();
            with_app(|a| {
                if enabled {
                    a.toasts.info(t("DragShift is paused"), t("Turn it back on from the tray icon."), 2500);
                } else {
                    a.toasts.info(t("DragShift is on"), &tf("Drag a file and press {}.", &[&trigger_name()]), 2500);
                }
            });
        }
        ID_OPEN => {
            let files = crate::tray::pick_files(hwnd);
            open_files(files);
        }
        ID_SETTINGS => open_settings(),
        i if (ID_SHOT..ID_SHOT + actions::IMAGE_TARGETS.len()).contains(&i) => {
            let fmt = actions::IMAGE_TARGETS[i - ID_SHOT];
            with_app(|a| a.save_clipboard(fmt, None));
        }
        i if (ID_SHOT_TO..ID_SHOT_TO + actions::IMAGE_TARGETS.len()).contains(&i) => {
            let fmt = actions::IMAGE_TARGETS[i - ID_SHOT_TO];
            if let Some(dir) = crate::tray::pick_folder(hwnd, &settings::get().shot_folder) {
                settings::update(|s| s.shot_folder = dir.display().to_string());
                with_app(|a| a.save_clipboard(fmt, Some(dir)));
            }
        }
        ID_HELP => {
            with_app(|a| a.toasts.info(t("How to use DragShift"),
                &tf("Start dragging a file, press {} and drop it on a petal. Press {} again for more options.", &[&trigger_name(), &trigger_name()]),
                12000));
        }
        ID_ABOUT => util::open_url(REPO_URL),
        ID_QUIT => unsafe { PostQuitMessage(0) },
        _ => {}
    }
}

/// A screenshot (Win+Shift+S, Print Screen), a copied picture or copied files.
fn clipboard_has_picture() -> bool {
    const CF_BITMAP: u32 = 2;
    const CF_DIB: u32 = 8;
    const CF_HDROP: u32 = 15;
    const CF_DIBV5: u32 = 17;
    unsafe {
        let png = RegisterClipboardFormatW(w!("PNG"));
        [CF_BITMAP, CF_DIB, CF_DIBV5, CF_HDROP, png].iter().any(|&f| f != 0 && IsClipboardFormatAvailable(f).is_ok())
    }
}

fn open_settings() {
    with_app(|a| {
        a.worker_send(json!({"op": "settings"}), false);
        a.idle_check();
    });
}

// ------------------------------------------------------------------ window procedures

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRIGGER_SHOW => {
            let (x, y) = hooks::unpack(lp.0);
            with_app(|a| {
                if !a.menu.visible {
                    a.menu.popup(x, y, None)
                }
            });
        }
        WM_FRAME => {
            vsync::handled();
            with_app(|a| {
                a.menu.on_frame();
                let cards = a.toasts.on_frame();
                if a.menu.visible {
                    vsync::run(true, a.menu.mon);
                } else {
                    vsync::run(cards, a.toasts.monitor());
                }
            });
        }
        WM_TRIGGER_TOGGLE => {
            with_app(|a| a.menu.trigger_toggle());
        }
        WM_TRIGGER_UP => {
            with_app(|a| a.menu.trigger_up());
        }
        WM_TRIGGER_ESC => {
            with_app(|a| a.menu.close());
        }
        WM_TRIGGER_DOWN => {
            let (x, y) = hooks::unpack(lp.0);
            with_app(|a| a.menu.outside_click(x, y));
        }
        WM_MENU_ACTIVATE => {
            let act = Box::from_raw(lp.0 as *mut Activation);
            with_app(|a| a.run_item(act.key, act.paths));
        }
        WM_JOB_PROGRESS => {
            with_app(|a| a.on_progress(wp.0 as u64, lp.0));
        }
        WM_JOB_DONE => {
            let d = Box::from_raw(lp.0 as *mut JobDone);
            with_app(|a| a.on_media_done(*d));
        }
        WM_WORKER_EVENT => {
            let b = Box::from_raw(lp.0 as *mut (u32, String));
            let (gen, line) = *b;
            match serde_json::from_str::<Value>(&line) {
                Ok(v) => {
                    with_app(|a| {
                        if gen == a.worker.gen {
                            a.on_worker(v)
                        }
                    });
                }
                Err(_) => crate::logf!("ui: {line}"),
            }
        }
        WM_WORKER_EXIT => {
            with_app(|a| a.on_worker_exit(wp.0 as u32));
        }
        WM_TRAY => match (lp.0 as u32) & 0xFFFF {
            WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(hwnd),
            _ => {}
        },
        WM_TIMER => match wp.0 {
            TIMER_TRIM => {
                let _ = KillTimer(hwnd, TIMER_TRIM);
                let idle = with_app(|a| {
                    a.menu.forget();
                    !a.menu.visible && a.toasts.cards.is_empty()
                }).unwrap_or(false);
                if idle {
                    util::trim_working_set();
                }
            }
            TIMER_FILES => {
                let _ = KillTimer(hwnd, TIMER_FILES);
                let files = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
                open_files(files);
            }
            TIMER_WORKER_IDLE => {
                let _ = KillTimer(hwnd, TIMER_WORKER_IDLE);
                with_app(|a| {
                    if !a.worker.busy() {
                        a.worker.stop();
                        schedule_trim();
                    }
                });
            }
            _ => {}
        },
        WM_COPYDATA => {
            let cds = &*(lp.0 as *const COPYDATASTRUCT);
            match cds.dwData {
                COPY_FILES if !cds.lpData.is_null() => {
                    let words = std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2);
                    let files: Vec<PathBuf> = String::from_utf16_lossy(words).split('\n').filter(|s| !s.is_empty())
                        .map(PathBuf::from).collect();
                    // the sender is blocked until we return: open the menu on the next message
                    let b = Box::into_raw(Box::new(files));
                    let _ = PostMessageW(hwnd, WM_APP + 70, WPARAM(0), LPARAM(b as isize));
                }
                COPY_ACTION if !cds.lpData.is_null() => {
                    let words = std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2);
                    let text = String::from_utf16_lossy(words);
                    let mut lines = text.split('\n').filter(|s| !s.is_empty());
                    if let Some(key) = lines.next() {
                        let files: Vec<PathBuf> = lines.map(PathBuf::from).filter(|p| p.is_file()).collect();
                        if !files.is_empty() {
                            let b = Box::into_raw(Box::new((key.to_string(), files)));
                            let _ = PostMessageW(hwnd, WM_APP + 73, WPARAM(0), LPARAM(b as isize));
                        }
                    }
                }
                COPY_QUIT => PostQuitMessage(0),
                COPY_SETTINGS => {
                    let _ = PostMessageW(hwnd, WM_APP + 71, WPARAM(0), LPARAM(0));
                }
                COPY_SHOW => {
                    let _ = PostMessageW(hwnd, WM_APP + 72, WPARAM(0), LPARAM(0));
                }
                _ => {}
            }
            return LRESULT(1);
        }
        m if m == WM_APP + 70 => {
            let files = Box::from_raw(lp.0 as *mut Vec<PathBuf>);
            PENDING.with(|p| {
                let mut p = p.borrow_mut();
                for f in *files {
                    if !p.contains(&f) {
                        p.push(f);
                    }
                }
            });
            SetTimer(hwnd, TIMER_FILES, 300, None);
        }
        m if m == WM_APP + 71 => open_settings(),
        m if m == WM_APP + 73 => {
            let b = Box::from_raw(lp.0 as *mut (String, Vec<PathBuf>));
            let (key, files) = *b;
            with_app(|a| a.run_item(key, files));
        }
        m if m == WM_APP + 72 => welcome(false),
        WM_ENDSESSION if wp.0 != 0 => PostQuitMessage(0),
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            crate::tray::add(hwnd, WM_TRAY);
        }
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    LRESULT(0)
}

fn client_xy(lp: LPARAM) -> (f32, f32) {
    let x = (lp.0 & 0xFFFF) as u16 as i16 as f32;
    let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as f32;
    (x, y)
}

/// Shared by the menu and every notification card.
unsafe extern "system" fn surface_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_MENU_RENDERED => {
            // the box is ours either way: free it if the menu is busy or gone
            let handled = with_app(|a| {
                a.menu.on_rendered(lp);
            });
            if handled.is_none() {
                drop(Box::from_raw(lp.0 as *mut crate::menu::Rendered));
            }
            return LRESULT(0);
        }
        WM_TOAST_THUMB => {
            let th = Box::from_raw(lp.0 as *mut Thumb);
            with_app(|a| {
                if let Some(c) = a.toasts.by_hwnd(hwnd) {
                    c.on_thumb(*th);
                }
            });
            return LRESULT(0);
        }
        WM_TIMER => {
            with_app(|a| {
                if hwnd == a.menu.hwnd {
                    a.menu.on_timer(wp.0);
                } else {
                    a.toasts.tick(hwnd);
                }
            });
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let (x, y) = client_xy(lp);
            with_app(|a| {
                if hwnd == a.menu.hwnd {
                    a.menu.mouse_move(x, y);
                } else if let Some(c) = a.toasts.by_hwnd(hwnd) {
                    c.mouse_move(x, y);
                }
            });
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            with_app(|a| {
                if hwnd == a.menu.hwnd {
                    a.menu.mouse_leave();
                } else if let Some(c) = a.toasts.by_hwnd(hwnd) {
                    c.mouse_leave();
                }
            });
            return LRESULT(0);
        }
        WM_SETCURSOR => {
            let hand = with_app(|a| a.toasts.by_hwnd(hwnd).map(|c| c.wants_hand()).unwrap_or(false)).unwrap_or(false);
            let _ = SetCursor(LoadCursorW(None, if hand { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
            return LRESULT(1);
        }
        WM_LBUTTONUP | WM_RBUTTONUP => {
            let (x, y) = client_xy(lp);
            let right = msg == WM_RBUTTONUP;
            let act = with_app(|a| {
                if hwnd == a.menu.hwnd {
                    return a.menu.mouse_up(right, x, y);
                }
                if right {
                    return None;
                }
                let req = a.toasts.by_hwnd(hwnd).map(|c| c.click(x, y));
                if let Some(crate::toast::Request::Cancel(j)) = req {
                    a.cancel(j);
                }
                a.toasts.reap();
                None
            }).flatten();
            if let Some(act) = act {
                with_app(|a| a.run_item(act.key, act.paths));
            }
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
