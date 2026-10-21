//! Small Windows helpers: wide strings, memory-mapped files, logging, output paths,
//! sizes, Explorer actions and the Recycle Bin.

use std::ffi::OsStr;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Memory::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// Map a file read-only for the life of the process. Pages are shared with the file cache and
/// only count towards our memory while they are actually touched.
pub fn map_file(path: &str) -> Option<&'static [u8]> {
    unsafe {
        let w = wide(path);
        let file = CreateFileW(PCWSTR(w.as_ptr()), GENERIC_READ.0, FILE_SHARE_READ, None, OPEN_EXISTING,
                               FILE_ATTRIBUTE_NORMAL, None).ok()?;
        let mut size = 0i64;
        if GetFileSizeEx(file, &mut size).is_err() || size == 0 {
            let _ = CloseHandle(file);
            return None;
        }
        let map = CreateFileMappingW(file, None, PAGE_READONLY, 0, 0, None);
        let _ = CloseHandle(file);
        let map = map.ok()?;
        let view = MapViewOfFile(map, FILE_MAP_READ, 0, 0, 0);
        let _ = CloseHandle(map);
        if view.Value.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(view.Value as *const u8, size as usize))
    }
}

// ------------------------------------------------------------------ locations

pub fn local_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join("DragShift");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default()
}

// ------------------------------------------------------------------ logging

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn init_log() {
    let d = local_dir();
    let path = d.join("log.txt");
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&path, d.join("log.old.txt"));
    }
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        *LOG.lock().unwrap() = Some(f);
    }
    log(&format!("==== DragShift {} (native) started ====", env!("CARGO_PKG_VERSION")));
}

pub fn log(msg: &str) {
    if let Ok(mut g) = LOG.lock() {
        if let Some(f) = g.as_mut() {
            let _ = writeln!(f, "{msg}");
        }
    }
    if cfg!(debug_assertions) {
        eprintln!("{msg}");
    }
}

#[macro_export]
macro_rules! logf {
    ($($t:tt)*) => { $crate::util::log(&format!($($t)*)) };
}

// ------------------------------------------------------------------ sizes and names

pub fn human_size(n: u64) -> String {
    let mut v = n as f64;
    for unit in [crate::i18n::t("B"), crate::i18n::t("KB"), crate::i18n::t("MB")] {
        if v < 1024.0 {
            return if unit == crate::i18n::t("B") { format!("{v:.0} {unit}") } else { format!("{v:.1} {unit}") };
        }
        v /= 1024.0;
    }
    format!("{v:.1} {}", crate::i18n::t("GB"))
}

pub fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

pub fn fmt_duration(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub fn ext_lower(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// `name (1).ext`, `name (2).ext`… so nothing is ever overwritten.
pub fn unique_path(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
    (1..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|c| !c.exists()).unwrap()
}

/// Output path beside `src` (or in a DragShift subfolder): sibling("a.mp4", "Trimmed", "mp4").
pub fn sibling(src: &Path, label: &str, ext: &str) -> PathBuf {
    let mut dir = src.parent().map(Path::to_path_buf).unwrap_or_default();
    if crate::settings::get().output == "folder" {
        dir = dir.join("DragShift");
        let _ = std::fs::create_dir_all(&dir);
    }
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = if label.is_empty() { stem } else { format!("{stem} {label}") };
    unique_path(dir.join(format!("{name}.{}", ext.trim_start_matches('.'))))
}

// ------------------------------------------------------------------ shell actions

pub fn shell_open(path: &Path) {
    let w = wide(path);
    let verb = wide("open");
    unsafe {
        ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(w.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

pub fn open_url(url: &str) {
    shell_open(Path::new(url));
}

/// Explorer window with the file selected.
pub fn reveal(path: &Path) {
    unsafe {
        let w = wide(path);
        let pidl = ILCreateFromPathW(PCWSTR(w.as_ptr()));
        if !pidl.is_null() {
            let _ = SHOpenFolderAndSelectItems(pidl, None, 0);
            ILFree(Some(pidl));
        } else if let Some(dir) = path.parent() {
            shell_open(dir);
        }
    }
}

/// Move files to the Recycle Bin; returns how many went. Empty output folders are removed.
pub fn recycle(paths: &[PathBuf]) -> usize {
    let mut n = 0;
    for p in paths.iter().filter(|p| p.is_file()) {
        let mut from = wide(p);
        from.push(0); // double-NUL terminated list
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
            ..Default::default()
        };
        if unsafe { SHFileOperationW(&mut op) } == 0 && !op.fAnyOperationsAborted.as_bool() {
            n += 1;
        }
    }
    for d in paths.iter().filter_map(|p| p.parent()) {
        if std::fs::read_dir(d).map(|mut r| r.next().is_none()).unwrap_or(false) {
            let _ = std::fs::remove_dir(d);
        }
    }
    n
}

/// Desktop shortcuts stand for the file they point to.
pub fn resolve_shortcut(p: &Path) -> PathBuf {
    if ext_lower(p) != "lnk" {
        return p.to_path_buf();
    }
    use windows::Win32::System::Com::*;
    unsafe {
        let link: windows::core::Result<IShellLinkW> = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER);
        let Ok(link) = link else { return p.to_path_buf() };
        let Ok(pf) = windows::core::Interface::cast::<IPersistFile>(&link) else { return p.to_path_buf() };
        let w = wide(p);
        if pf.Load(PCWSTR(w.as_ptr()), STGM_READ).is_err() {
            return p.to_path_buf();
        }
        let mut buf = [0u16; 1024];
        if link.GetPath(&mut buf, std::ptr::null_mut(), 0).is_ok() {
            let t = PathBuf::from(from_wide(&buf));
            if t.is_file() {
                return t;
            }
        }
    }
    p.to_path_buf()
}



/// Hand the pages we no longer touch back to Windows (called when going idle).
pub fn trim_working_set() {
    crate::gfx::forget_glyphs();
    unsafe {
        use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
        use windows::Win32::System::Threading::GetCurrentProcess;
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }
}
