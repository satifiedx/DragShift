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
    pub lang: String,        // auto | en | ru | uk
    pub theme: String,       // auto | dark | light
    pub gpu: bool,           // use NVENC / Quick Sync / AMF when the PC has them
    pub ui_idle_seconds: u32, // how long the editor process stays warm after its last window
    pub enabled: bool,
    pub shot_folder: String, // last folder picked for "Save screenshot to a folder"
    pub video_codec: String, // hevc | h264: what Compress encodes video to
    pub video_presets: Vec<VideoPreset>,
}

/// A compress preset made in the "New preset" window; shown in the video Compress ring.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct VideoPreset {
    pub name: String,
    pub vcodec: String, // hevc | h264 | av1 | vp9
    pub vkbps: u32,     // 0 = by quality
    pub height: u32,    // short side; 0 = original
    pub acodec: String, // aac | opus | mp3 | copy
    pub akbps: u32,
}

impl Default for VideoPreset {
    fn default() -> Self {
        VideoPreset { name: String::new(), vcodec: "hevc".into(), vkbps: 0, height: 0, acodec: "aac".into(), akbps: 128 }
    }
}

impl VideoPreset {
    /// The menu key that carries the whole preset (so a running job never looks it up again).
    pub fn key(&self) -> String {
        format!("enc:{},{},{},{},{}", self.vcodec, self.vkbps, self.height, self.acodec, self.akbps)
    }

    pub fn from_spec(spec: &str) -> Option<VideoPreset> {
        let p: Vec<&str> = spec.split(',').collect();
        let [vc, vk, h, ac, ak] = p.as_slice() else { return None };
        Some(VideoPreset { name: String::new(), vcodec: vc.to_string(), vkbps: vk.parse().ok()?, height: h.parse().ok()?,
                           acodec: ac.to_string(), akbps: ak.parse().ok()? })
    }

    pub fn codec_name(&self) -> &'static str {
        match self.vcodec.as_str() {
            "h264" => "H.264",
            "av1" => "AV1",
            "vp9" => "VP9",
            _ => "H.265",
        }
    }
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
            shot_folder: String::new(),
            video_codec: "hevc".into(),
            video_presets: vec![],
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
    // keys only the Python side knows stay in the file
    let mut doc = std::fs::read_to_string(path()).ok()
        .and_then(|t| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&t).ok()).unwrap_or_default();
    if let Ok(serde_json::Value::Object(m)) = serde_json::to_value(&s) {
        doc.extend(m);
    }
    if let Ok(t) = serde_json::to_string_pretty(&doc) {
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
