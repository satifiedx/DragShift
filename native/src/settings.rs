//! Settings shared with the Python UI: %APPDATA%\DragShift\settings.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub trigger: String,     // shift | ctrl | alt
    pub output: String,      // next | folder
    pub jpg_quality: u32,
    pub pdf_dpi: u32,
    pub welcomed: bool,
    pub lang: String,        // auto | en | ru
    pub theme: String,       // auto | dark | light
    pub gpu: bool,           // use NVENC / Quick Sync / AMF when the PC has them
    pub ui_idle_seconds: u32, // how long the editor process stays warm after its last window
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            trigger: "shift".into(),
            output: "next".into(),
            jpg_quality: 90,
            pdf_dpi: 200,
            welcomed: false,
            lang: "auto".into(),
            theme: "auto".into(),
            gpu: true,
            ui_idle_seconds: 60,
            enabled: true,
        }
    }
}

static CURRENT: RwLock<Option<Settings>> = RwLock::new(None);

pub fn path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join("DragShift");
    let _ = std::fs::create_dir_all(&d);
    d.join("settings.json")
}

pub fn load() {
    let s = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    *CURRENT.write().unwrap() = Some(s);
}

pub fn get() -> Settings {
    if let Some(s) = CURRENT.read().unwrap().as_ref() {
        return s.clone();
    }
    load();
    CURRENT.read().unwrap().clone().unwrap_or_default()
}

pub fn update(f: impl FnOnce(&mut Settings)) {
    let mut s = get();
    f(&mut s);
    if let Ok(t) = serde_json::to_string_pretty(&s) {
        let tmp = path().with_extension("json.tmp");
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }
    *CURRENT.write().unwrap() = Some(s);
}

// ------------------------------------------------------------------ autostart

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

pub fn set_autostart(on: bool) {
    use crate::util::wide;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::*;
    unsafe {
        let mut key = HKEY::default();
        let k = wide(RUN_KEY);
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), 0, KEY_SET_VALUE, &mut key).is_err() {
            return;
        }
        let name = wide("DragShift");
        if on {
            let exe = std::env::current_exe().unwrap_or_default();
            let cmd = wide(format!("\"{}\" --quiet", exe.display()));
            let bytes = std::slice::from_raw_parts(cmd.as_ptr() as *const u8, cmd.len() * 2);
            let _ = RegSetValueExW(key, PCWSTR(name.as_ptr()), 0, REG_SZ, Some(bytes));
        } else {
            let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
        }
        let _ = RegCloseKey(key);
    }
}
