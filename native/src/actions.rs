//! What the menu offers for a file: rings of items per file category.

use crate::i18n::{t, tf};
use crate::util::{ext_lower, file_size, human_size};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    Image,
    Pdf,
    Docx,
    Office,
    Text,
    Video,
    Audio,
}

impl Cat {
    pub fn name(self) -> &'static str {
        match self {
            Cat::Image => "image",
            Cat::Pdf => "pdf",
            Cat::Docx => "docx",
            Cat::Office => "office",
            Cat::Text => "text",
            Cat::Video => "video",
            Cat::Audio => "audio",
        }
    }
    pub fn is_media(self) -> bool {
        matches!(self, Cat::Video | Cat::Audio)
    }
}

pub const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpg", "mpeg", "ts",
                                  "mts", "m2ts", "3gp", "ogv", "vob", "asf"];
pub const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "aiff", "aif", "wma",
                                  "amr", "ac3", "mka", "caf", "m4r", "alac", "ape", "wv"];
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "heic", "heif", "tif", "tiff", "avif", "bmp", "gif", "ico"];
const OFFICE_EXTS: &[&str] = &["xlsx", "xlsm", "xls", "pptx", "ppt"];
const TEXT_EXTS: &[&str] = &["txt", "md", "csv", "log", "json", "xml", "html", "htm"];

pub fn category(p: &Path) -> Option<Cat> {
    let e = ext_lower(p);
    let e = e.as_str();
    if IMAGE_EXTS.contains(&e) {
        Some(Cat::Image)
    } else if e == "pdf" {
        Some(Cat::Pdf)
    } else if e == "docx" {
        Some(Cat::Docx)
    } else if OFFICE_EXTS.contains(&e) {
        Some(Cat::Office)
    } else if TEXT_EXTS.contains(&e) {
        Some(Cat::Text)
    } else if VIDEO_EXTS.contains(&e) {
        Some(Cat::Video)
    } else if AUDIO_EXTS.contains(&e) {
        Some(Cat::Audio)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub label: String,
    pub glyph: &'static str,
    pub key: String,
    pub hint: String,
}

impl Item {
    fn fmt(label: &str, key: String, hint: &str) -> Item {
        Item { label: label.into(), glyph: "", key, hint: hint.into() }
    }
    fn tool(label: &str, glyph: &'static str, key: String, hint: &str) -> Item {
        Item { label: label.into(), glyph, key, hint: hint.into() }
    }
}

#[derive(Clone, Debug)]
pub struct Ring {
    pub name: String,
    pub items: Vec<Item>,
}

pub const IMAGE_TARGETS: &[&str] = &["PNG", "JPG", "WEBP", "HEIC", "TIFF", "AVIF", "BMP", "PDF", "ICO", "GIF"];
const PDF_TARGETS: &[&str] = &["JPG", "PNG", "DOCX", "TXT"];
pub const VIDEO_TARGETS: &[&str] = &["MP4", "MKV", "WEBM", "MOV", "AVI", "GIF", "MP3", "M4A", "WAV"];
pub const AUDIO_TARGETS: &[&str] = &["MP3", "M4A", "WAV", "FLAC", "OGG", "OPUS", "AIFF", "WMA", "ALAC"];
const SIZES_MB: &[u64] = &[10, 25, 50, 100];
const BITRATES: &[u32] = &[320, 256, 192, 128, 96, 64];
const MAX_PRESETS: usize = 4;

/// "H.265 · 720p · 2500 kbps" for a preset's petal.
fn preset_hint(p: &crate::settings::VideoPreset) -> String {
    let mut bits = vec![p.codec_name().to_string()];
    if p.height > 0 {
        bits.push(format!("{}p", p.height));
    }
    if p.vkbps > 0 {
        bits.push(format!("{} {}", p.vkbps, t("kbps")));
    }
    bits.join(" · ")
}

pub fn format_hint(f: &str) -> &'static str {
    match f {
        "JPG" => t("Small, universal"),
        "PNG" => t("Lossless · transparency"),
        "WEBP" => t("Smallest for web"),
        "HEIC" => t("iPhone format"),
        "TIFF" => t("Print · archival"),
        "AVIF" => t("Next-gen, tiny"),
        "BMP" => t("Uncompressed"),
        "PDF" => t("Document"),
        "ICO" => t("Windows icon"),
        "GIF" => t("Animated · legacy web"),
        "DOCX" => t("Editable Word"),
        "TXT" => t("Plain text"),
        "MP4" => t("Plays everywhere"),
        "MKV" => t("Keeps every track"),
        "WEBM" => t("Open web video"),
        "MOV" => t("Apple · editing"),
        "AVI" => t("Old players"),
        "MP3" => t("Universal audio"),
        "M4A" => t("AAC · Apple, phones"),
        "WAV" => t("Uncompressed audio"),
        "FLAC" => t("Lossless, smaller"),
        "OGG" => t("Vorbis · games, web"),
        "OPUS" => t("Best quality per MB"),
        "AIFF" => t("Uncompressed · Mac"),
        "WMA" => t("Windows Media"),
        "ALAC" => t("Lossless · Apple"),
        _ => "",
    }
}

/// The format a file already is (so it is left out of its own Convert ring).
fn same_as(ext: &str) -> String {
    match ext {
        "jpeg" => "JPG".into(),
        "tif" => "TIFF".into(),
        "heif" => "HEIC".into(),
        "m4v" => "MP4".into(),
        "aif" => "AIFF".into(),
        "oga" => "OGG".into(),
        "aac" => "M4A".into(),
        e => e.to_uppercase(),
    }
}

fn convert_ring(targets: &[&str], ext: &str, multi_hint: Option<String>) -> Ring {
    let cur = same_as(ext);
    let items = targets.iter().filter(|f| **f != cur).map(|f| {
        let hint = match (&multi_hint, *f) {
            (Some(h), "PDF") => h.clone(),
            _ => format_hint(f).to_string(),
        };
        Item::fmt(f, format!("convert:{f}"), &hint)
    }).collect();
    Ring { name: t("Convert").into(), items }
}

pub fn rings_for(paths: &[PathBuf]) -> (Vec<Ring>, Option<Cat>) {
    let Some(first) = paths.first() else { return (vec![], None) };
    let Some(cat) = category(first) else { return (vec![], None) };
    let ext = ext_lower(first);
    let same = paths.iter().filter(|p| category(p) == Some(cat)).count();
    let mut rings = Vec::new();
    match cat {
        Cat::Image => {
            let multi = (same > 1).then(|| tf("{} images → 1 PDF", &[&same.to_string()]));
            rings.push(convert_ring(IMAGE_TARGETS, &ext, multi));
            let tools = [
                ("Compress", "⇲", "Shrink file size"),
                ("Resize", "⤢", "Change dimensions"),
                ("Crop", "⌗", "Trim to a ratio"),
                ("Edit", "☼", "Light · colour · rotate"),
                ("Annotate", "✎", "Arrows · text · steps"),
                ("Redact", "▮", "Hide private info"),
                ("Add BG", "▣", "Gradient backdrop"),
                ("Metadata", "ⓘ", "View · remove GPS"),
            ];
            rings.push(Ring {
                name: t("Tools").into(),
                items: tools.iter().map(|&(n, g, h)| Item::tool(t(n), g, format!("tool:{n}"), t(h))).collect(),
            });
        }
        Cat::Pdf => {
            rings.push(convert_ring(PDF_TARGETS, &ext, None));
            let mut tools = vec![
                ("Compress", "⇲", "Shrink file size"),
                ("Split", "✂", "One PDF per page"),
                ("Rotate", "⟳", "Turn pages 90°"),
            ];
            if same > 1 {
                tools.push(("Merge", "⧉", "Join into one PDF"));
            }
            tools.extend([("Protect", "⚿", "Add a password"), ("Extract", "⧈", "Save embedded images"),
                          ("Metadata", "ⓘ", "View · remove GPS")]);
            rings.push(Ring {
                name: t("Tools").into(),
                items: tools.iter().map(|&(n, g, h)| Item::tool(t(n), g, format!("tool:{n}"), t(h))).collect(),
            });
        }
        Cat::Docx => rings.push(convert_ring(&["PDF", "TXT"], &ext, None)),
        Cat::Office => rings.push(convert_ring(&["PDF"], &ext, None)),
        Cat::Text => rings.push(convert_ring(&["PDF", "DOCX"], &ext, None)),
        Cat::Video => {
            rings.push(convert_ring(VIDEO_TARGETS, &ext, None));
            let tools = [
                ("Trim", "✂", "Cut start and end"),
                ("Crop", "⌗", "Cut the frame"),
                ("Rotate", "⟳", "90° clockwise"),
                ("Speed", "⏩", "Faster or slower"),
                ("Mute", "🔇", "Remove the sound"),
                ("Mono", "◎", "Both channels into one"),
                ("Audio", "♫", "Save the soundtrack"),
                ("Frame", "▦", "Snapshot as PNG"),
            ];
            rings.push(Ring {
                name: t("Tools").into(),
                items: tools.iter().map(|&(n, g, h)| Item::tool(t(n), g, format!("tool:{n}"), t(h))).collect(),
            });
            let size = paths.iter().filter(|p| category(p) == Some(cat)).map(|p| file_size(p)).max().unwrap_or(0);
            let mut items: Vec<Item> = SIZES_MB.iter().filter(|mb| **mb * 1_000_000 < size)
                .map(|mb| {
                    let hint = match mb {
                        10 => t("Discord, email"),
                        25 => t("Email, messengers"),
                        50 => t("Messengers"),
                        _ => t("Uploads"),
                    };
                    Item::tool(&format!("{mb} {}", t("MB")), "⇲", format!("size:{mb}"), hint)
                })
                .collect();
            items.push(Item::tool("½", "⇲", "size:50%".into(), t("Half the size")));
            items.push(Item::tool("¼", "⇲", "size:25%".into(), t("A quarter of the size")));
            // the newest custom presets, then the button that makes one
            for p in crate::settings::get().video_presets.iter().rev().take(MAX_PRESETS) {
                let name = if p.name.trim().is_empty() { p.codec_name().to_string() } else { p.name.clone() };
                items.push(Item::tool(&name, "★", p.key(), &preset_hint(p)));
            }
            items.push(Item::tool(t("New preset"), "+", "tool:Preset".into(), t("Codec · bitrate · size")));
            rings.push(Ring { name: t("Compress").into(), items });
        }
        Cat::Audio => {
            rings.push(convert_ring(AUDIO_TARGETS, &ext, None));
            let mut items = vec![Item::tool(t("Trim"), "✂", "tool:Trim".into(), t("Waveform · fades")),
                                 Item::tool(t("Speed"), "⏩", "tool:Speed".into(), t("Faster or slower")),
                                 Item::tool(t("Mono"), "◎", "tool:Mono".into(), t("Both channels into one")),
                                 Item::tool(t("Normalize"), "≋", "tool:Normalize".into(), t("Even, steady loudness"))];
            for k in BITRATES {
                let hint = match k {
                    320 => t("Best MP3 quality"),
                    256 => t("Near transparent"),
                    192 => t("Great for music"),
                    128 => t("Good · small"),
                    96 => t("Podcasts, voice"),
                    _ => t("Voice, smallest"),
                };
                items.push(Item::tool(&format!("{k}k"), "♪", format!("kbps:{k}"), hint));
            }
            rings.push(Ring { name: t("Tools").into(), items });
        }
    }
    (rings, Some(cat))
}

pub fn describe(paths: &[PathBuf]) -> String {
    if paths.len() > 1 {
        return tf("{} files", &[&paths.len().to_string()]);
    }
    paths.first().map(|p| human_size(file_size(p))).unwrap_or_default()
}
