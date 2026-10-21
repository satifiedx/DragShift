//! Video and audio through ffmpeg (a bundled ffmpeg.exe run as a low-priority child process).
//!
//! Streams are copied whenever they already fit the target (MKV → MP4 is a remux, not an
//! encode). Video encodes use the GPU (NVENC, Quick Sync, AMF) when the PC has one, otherwise
//! x264. "Compress to 10 MB" plans the bitrate, picks a sensible resolution, checks the result
//! and re-runs once if it came out over the limit.

use crate::i18n::{t, tf};
use crate::settings::VideoPreset;
use crate::toast::Outcome;
use crate::util::{self, ext_lower, file_size, fmt_duration, human_size, sibling};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

const GIF_WIDTH: u32 = 640;
const GIF_FPS: f32 = 15.0;
const GIF_MAX_SECONDS: f64 = 60.0;
const CRF_CONVERT: u32 = 20;
const MIN_VIDEO_BPS: f64 = 60_000.0;

pub type Res<T> = Result<T, String>;

// ------------------------------------------------------------------ locating ffmpeg

pub fn ffmpeg() -> Option<PathBuf> {
    static P: OnceLock<Option<PathBuf>> = OnceLock::new();
    P.get_or_init(|| {
        let dir = util::exe_dir();
        let mut cands = vec![dir.join("ffmpeg").join("ffmpeg.exe"), dir.join("ffmpeg.exe")];
        if let Some(e) = std::env::var_os("DRAGSHIFT_FFMPEG") {
            cands.insert(0, PathBuf::from(e));
        }
        if let Some(path) = std::env::var_os("PATH") {
            cands.extend(std::env::split_paths(&path).map(|p| p.join("ffmpeg.exe")));
        }
        cands.into_iter().find(|p| p.is_file())
    }).clone()
}

fn missing() -> String {
    t("Video and audio need ffmpeg, and DragShift couldn't find it. Reinstall DragShift.").into()
}

fn unreadable() -> String {
    t("This file looks damaged, or it isn't a video or audio format DragShift can read.").into()
}

fn file_arg(p: &Path) -> String {
    // ffmpeg reads "-x.mp4" as an option and "name:..." as a protocol; "file:" is always a file
    format!("file:{}", p.display())
}

fn command(args: &[String]) -> Res<Command> {
    let exe = ffmpeg().ok_or_else(missing)?;
    let mut c = Command::new(exe);
    c.args(args).stdin(Stdio::null()).creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    Ok(c)
}

// ------------------------------------------------------------------ probing

#[derive(Clone, Debug, Default)]
pub struct Stream {
    pub index: u32,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub rotation: i32, // counter-clockwise degrees, as ffmpeg prints them
    pub sample_rate: u32,
    pub channels: u32,
    pub bits: u32,
    pub kbps: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Info {
    pub duration: f64,
    pub format: String,
    pub kbps: u32,
    pub video: Vec<Stream>,
    pub audio: Vec<Stream>,
    pub subs: Vec<Stream>,
    pub attachments: bool, // fonts for styled subtitles, inside MKV
}

impl Info {
    /// Display size (phone clips stored sideways report their upright size).
    pub fn size(&self) -> Option<(u32, u32)> {
        let v = self.video.first()?;
        Some(if v.rotation.rem_euclid(180) == 90 { (v.height, v.width) } else { (v.width, v.height) })
    }
}

/// Split "a, b(c, d), e" at top-level commas.
fn split_top(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn parse_hms(s: &str) -> Option<f64> {
    let mut parts = s.trim().split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

fn first_num<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.split_whitespace().next()?.parse().ok()
}

pub fn parse_probe(text: &str) -> Info {
    let mut info = Info::default();
    let mut last_video: Option<usize> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("Input #0, ") {
            info.format = rest.split(", from").next().unwrap_or("").to_string();
        } else if let Some(rest) = line.strip_prefix("Duration: ") {
            for part in rest.split(", ") {
                if let Some(b) = part.strip_prefix("bitrate: ") {
                    info.kbps = first_num(b).unwrap_or(0);
                } else if !part.starts_with("start:") {
                    info.duration = parse_hms(part).unwrap_or(0.0);
                }
            }
        } else if line.starts_with("Stream #0:") {
            let idx: u32 = line["Stream #0:".len()..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>()
                .parse().unwrap_or(0);
            if let Some(pos) = line.find(": Video: ") {
                if line.contains("(attached pic)") {
                    continue; // album art inside an MP3/M4A
                }
                let parts = split_top(&line[pos + 9..]);
                let mut s = Stream { index: idx, ..Default::default() };
                s.codec = parts.first().and_then(|p| p.split_whitespace().next()).unwrap_or("").to_string();
                for p in &parts[1..] {
                    let tok = p.split_whitespace().next().unwrap_or("");
                    if let Some((w, h)) = tok.split_once('x') {
                        if !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) && h.chars().all(|c| c.is_ascii_digit()) && !h.is_empty() && s.width == 0 {
                            s.width = w.parse().unwrap_or(0);
                            s.height = h.parse().unwrap_or(0);
                        }
                    }
                    if p.ends_with(" fps") {
                        s.fps = first_num(p).unwrap_or(0.0);
                    } else if p.ends_with(" kb/s") {
                        s.kbps = first_num(p).unwrap_or(0);
                    }
                }
                info.video.push(s);
                last_video = Some(info.video.len() - 1);
            } else if let Some(pos) = line.find(": Audio: ") {
                last_video = None;
                let parts = split_top(&line[pos + 9..]);
                let mut s = Stream { index: idx, ..Default::default() };
                s.codec = parts.first().and_then(|p| p.split_whitespace().next()).unwrap_or("").to_string();
                for p in &parts[1..] {
                    if p.ends_with(" Hz") {
                        s.sample_rate = first_num(p).unwrap_or(0);
                    } else if p.ends_with(" kb/s") {
                        s.kbps = first_num(p).unwrap_or(0);
                    } else if p == "mono" {
                        s.channels = 1;
                    } else if p == "stereo" {
                        s.channels = 2;
                    } else if p.starts_with("5.1") {
                        s.channels = 6;
                    } else if p.starts_with("7.1") {
                        s.channels = 8;
                    } else if p.ends_with(" channels") {
                        s.channels = first_num(p).unwrap_or(2);
                    } else if p.contains("(24 bit)") {
                        s.bits = 24;
                    } else if p.starts_with("s16") {
                        s.bits = 16;
                    } else if p.starts_with("s32") {
                        s.bits = 24;
                    }
                }
                if s.channels == 0 {
                    s.channels = 2;
                }
                info.audio.push(s);
            } else if let Some(pos) = line.find(": Subtitle: ") {
                last_video = None;
                let codec = line[pos + 12..].split([' ', ',']).next().unwrap_or("").to_string();
                info.subs.push(Stream { index: idx, codec, ..Default::default() });
            } else {
                last_video = None;
                if line.contains(": Attachment: ") {
                    info.attachments = true;
                }
            }
        } else if let Some(rest) = line.strip_prefix("displaymatrix: rotation of ") {
            if let Some(i) = last_video {
                let deg: f32 = first_num(rest.trim_end_matches(" degrees")).unwrap_or(0.0);
                info.video[i].rotation = deg.round() as i32;
            }
        }
    }
    // lossy codecs report no meaningful bit depth
    for a in info.audio.iter_mut() {
        if !(a.codec.starts_with("pcm") || a.codec == "flac" || a.codec == "alac" || a.codec == "wavpack") {
            a.bits = 0;
        }
    }
    info
}

pub fn probe(path: &Path) -> Res<Info> {
    let out = command(&["-hide_banner".into(), "-nostdin".into(), "-i".into(), file_arg(path)])?
        .stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| {
            crate::logf!("ffmpeg could not start: {e}");
            missing()
        })?;
    if !path.exists() {
        return Err(t("The file was moved or deleted.").into());
    }
    let info = parse_probe(&String::from_utf8_lossy(&out.stderr));
    if info.video.is_empty() && info.audio.is_empty() {
        crate::logf!("probe found no streams in {}", path.display());
        return Err(unreadable());
    }
    Ok(info)
}

// ------------------------------------------------------------------ encoders

fn encoders() -> &'static HashSet<String> {
    static E: OnceLock<HashSet<String>> = OnceLock::new();
    E.get_or_init(|| {
        let mut set = HashSet::new();
        let Ok(mut c) = command(&["-hide_banner".into(), "-encoders".into()]) else { return set };
        if let Ok(out) = c.stderr(Stdio::null()).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let mut it = line.split_whitespace();
                if let (Some(flags), Some(name)) = (it.next(), it.next()) {
                    if flags.len() == 6 && "VAS".contains(&flags[..1]) && name != "=" {
                        set.insert(name.to_string());
                    }
                }
            }
        }
        set
    })
}

fn has(enc: &str) -> bool {
    encoders().contains(enc)
}

/// Does a (hardware) encoder really work here? Tried once, remembered on disk per ffmpeg build.
fn encoder_works(enc: &str) -> bool {
    static CACHE: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
    let key = ffmpeg().and_then(|p| {
        let m = std::fs::metadata(&p).ok()?;
        let mt = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(format!("{}|{}|{mt}", p.display(), m.len()))
    }).unwrap_or_default();
    let file = util::local_dir().join("encoders.json");
    let mut g = CACHE.lock().unwrap();
    if g.is_none() {
        let mut map: HashMap<String, bool> = std::fs::read_to_string(&file).ok()
            .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if map.get("__key").is_none() || std::fs::read_to_string(util::local_dir().join("encoders.key")).ok().as_deref() != Some(&key) {
            map.clear();
            map.insert("__key".into(), true);
            let _ = std::fs::write(util::local_dir().join("encoders.key"), &key);
        }
        *g = Some(map);
    }
    let map = g.as_mut().unwrap();
    if let Some(v) = map.get(enc) {
        return *v;
    }
    let ok = has(enc) && command(&[
        "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=gray:s=256x256:r=10:d=0.5",
        "-c:v", enc, "-pix_fmt", if enc.ends_with("_qsv") { "nv12" } else { "yuv420p" }, "-f", "null", "-",
    ].iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .ok().and_then(|mut c| c.stdout(Stdio::null()).stderr(Stdio::null()).status().ok())
        .map(|s| s.success()).unwrap_or(false);
    crate::logf!("encoder {enc}: {}", if ok { "works" } else { "not usable" });
    map.insert(enc.to_string(), ok);
    let _ = std::fs::write(&file, serde_json::to_string(&*map).unwrap_or_default());
    ok
}

/// Video codec of an MP4/MKV encode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Vc {
    H264,
    Hevc,
    Av1,
}

impl Vc {
    fn from_name(name: &str) -> Vc {
        match name {
            "h264" => Vc::H264,
            "av1" => Vc::Av1,
            _ => Vc::Hevc,
        }
    }
}

/// Which encoder runs it: a GPU one, the CPU library (x264, x265, libaom), Media Foundation, or
/// plain MPEG-4 as the last resort.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Enc {
    Nvenc,
    Qsv,
    Amf,
    Cpu,
    Mf,
    Mpeg4,
}

fn enc_name(vc: Vc, e: Enc) -> &'static str {
    match (vc, e) {
        (_, Enc::Mpeg4) => "mpeg4",
        (Vc::H264, Enc::Nvenc) => "h264_nvenc",
        (Vc::H264, Enc::Qsv) => "h264_qsv",
        (Vc::H264, Enc::Amf) => "h264_amf",
        (Vc::H264, Enc::Cpu) => "libx264",
        (Vc::H264, Enc::Mf) => "h264_mf",
        (Vc::Hevc, Enc::Nvenc) => "hevc_nvenc",
        (Vc::Hevc, Enc::Qsv) => "hevc_qsv",
        (Vc::Hevc, Enc::Amf) => "hevc_amf",
        (Vc::Hevc, Enc::Cpu) => "libx265",
        (Vc::Hevc, Enc::Mf) => "hevc_mf",
        (Vc::Av1, Enc::Nvenc) => "av1_nvenc",
        (Vc::Av1, Enc::Qsv) => "av1_qsv",
        (Vc::Av1, Enc::Amf) => "av1_amf",
        (Vc::Av1, Enc::Cpu) => "libaom-av1",
        (Vc::Av1, Enc::Mf) => "av1_mf",
    }
}

/// The best encoder this PC has for the codec; without any, H.264 stands in.
fn pick_encoder(vc: Vc, allow_gpu: bool) -> (Vc, Enc) {
    if allow_gpu && crate::settings::get().gpu {
        for e in [Enc::Nvenc, Enc::Qsv, Enc::Amf] {
            if encoder_works(enc_name(vc, e)) {
                return (vc, e);
            }
        }
    }
    if has(enc_name(vc, Enc::Cpu)) {
        (vc, Enc::Cpu)
    } else if encoder_works(enc_name(vc, Enc::Mf)) {
        (vc, Enc::Mf)
    } else if vc != Vc::H264 {
        pick_encoder(Vc::H264, allow_gpu)
    } else {
        (Vc::H264, Enc::Mpeg4)
    }
}

#[derive(Clone, Copy)]
enum Quality {
    Crf(u32),
    Bitrate(u64),
}

fn s<T: ToString>(v: T) -> String {
    v.to_string()
}

/// Bitrate H.265 and AV1 need for the look H.264 gets at the same rate.
fn rate_share(vc: Vc) -> f64 {
    match vc {
        Vc::H264 => 1.0,
        Vc::Hevc => 0.6,
        Vc::Av1 => 0.5,
    }
}

fn video_args(vc: Vc, enc: Enc, q: Quality, info: &Info) -> Vec<String> {
    let rate = |b: u64| vec![s("-b:v"), s(b), s("-maxrate"), s(b * 3 / 2), s("-bufsize"), s(b * 2)];
    // Media Foundation has no quality mode, and AMD's AV1 counts QP on a 0-255 scale: both get a bitrate
    let q = match (vc, enc, q) {
        (Vc::Av1, Enc::Amf, Quality::Crf(c)) | (_, Enc::Mf, Quality::Crf(c)) =>
            Quality::Bitrate((estimate_bitrate(info, c) as f64 * rate_share(vc)) as u64),
        _ => q,
    };
    // about the same look on each codec's own quality scale
    let bump = match vc {
        Vc::H264 => 0,
        Vc::Hevc => 3,
        Vc::Av1 => 8,
    };
    let mut a = vec![s("-c:v"), s(enc_name(vc, enc))];
    match (enc, q) {
        (Enc::Nvenc, Quality::Crf(c)) => a.extend(["-preset", "p5", "-tune", "hq", "-rc", "vbr", "-cq"].iter().map(|x| s(x))
            .chain([s(c + 2 + bump), s("-b:v"), s(0)])),
        (Enc::Nvenc, Quality::Bitrate(b)) => a.extend(["-preset", "p5", "-tune", "hq", "-rc", "vbr", "-multipass", "fullres"]
            .iter().map(|x| s(x)).chain(rate(b))),
        (Enc::Qsv, Quality::Crf(c)) => a.extend([s("-preset"), s("medium"), s("-global_quality"), s(c + 3 + bump)]),
        (Enc::Qsv, Quality::Bitrate(b)) => a.extend([s("-preset"), s("medium")].into_iter().chain(rate(b))),
        (Enc::Amf, Quality::Crf(c)) => {
            a.extend(["-quality", "quality", "-rc", "cqp"].iter().map(|x| s(x))
                .chain([s("-qp_i"), s(c + 1 + bump), s("-qp_p"), s(c + 3 + bump)]));
            if vc == Vc::H264 {
                a.extend([s("-qp_b"), s(c + 5)]);
            }
        }
        (Enc::Amf, Quality::Bitrate(b)) => a.extend(["-quality", "quality", "-rc", "vbr_peak"].iter().map(|x| s(x)).chain(rate(b))),
        (Enc::Cpu, Quality::Crf(c)) => match vc {
            Vc::H264 => a.extend([s("-preset"), s("faster"), s("-crf"), s(c)]),
            Vc::Hevc => a.extend([s("-preset"), s("fast"), s("-crf"), s(c + 5)]),
            Vc::Av1 => a.extend([s("-crf"), s(c + 10), s("-b:v"), s(0)]),
        },
        (Enc::Cpu, Quality::Bitrate(b)) => match vc {
            Vc::H264 => a.extend([s("-preset"), s("medium"), s("-b:v"), s(b)]),
            Vc::Hevc => a.extend([s("-preset"), s("fast")].into_iter().chain(rate(b))),
            Vc::Av1 => a.extend(rate(b)),
        },
        (Enc::Mf, Quality::Bitrate(b)) => a.extend([s("-b:v"), s(b)]),
        (Enc::Mf, Quality::Crf(_)) => {}
        (Enc::Mpeg4, Quality::Bitrate(b)) => a.extend(rate(b)),
        (Enc::Mpeg4, Quality::Crf(_)) => a.extend([s("-q:v"), s(3)]),
    }
    if enc == Enc::Cpu {
        match vc {
            Vc::Hevc => a.extend([s("-x265-params"), s("log-level=error")]),
            Vc::Av1 => a.extend(["-cpu-used", "6", "-row-mt", "1", "-tiles", "2x2"].iter().map(|x| s(x))),
            Vc::H264 => {}
        }
    }
    // quality mode on easy-to-compress footage can come out bigger than the original: cap the rate
    // near the source's (going from a newer codec to H.264 needs more room)
    if let (Quality::Crf(_), Enc::Nvenc | Enc::Cpu, Some(src)) = (q, enc, source_video_bps(info)) {
        let newer = info.video.first().is_some_and(|v| matches!(v.codec.as_str(), "hevc" | "vp9" | "av1"));
        let k = match (vc, newer) {
            (Vc::H264, true) => 2.2,
            (Vc::H264, false) | (_, true) => 1.25,
            (_, false) => 1.0,
        };
        let cap = (src as f64 * k).max(400_000.0) as u64;
        a.extend([s("-maxrate"), s(cap), s("-bufsize"), s(cap * 2)]);
    }
    a.extend([s("-pix_fmt"), s(if enc == Enc::Qsv { "nv12" } else { "yuv420p" })]);
    a
}

/// The source's video bitrate in bits/s, when ffmpeg reports one.
fn source_video_bps(info: &Info) -> Option<u64> {
    let v = info.video.first()?;
    let kbps = if v.kbps > 0 {
        v.kbps
    } else {
        info.kbps.saturating_sub(info.audio.iter().map(|a| a.kbps.max(128)).sum())
    };
    (kbps > 0).then_some(kbps as u64 * 1000)
}

/// Bitrate for encoders without a quality mode: ~0.1 bits per pixel at CRF 23.
fn estimate_bitrate(info: &Info, crf: u32) -> u64 {
    let (w, h) = info.size().unwrap_or((1280, 720));
    let fps = info.video.first().map(|v| v.fps).filter(|f| *f > 0.0).unwrap_or(30.0) as f64;
    let rate = w as f64 * h as f64 * fps * 0.1 * 2f64.powf((23.0 - crf as f64) / 6.0);
    rate.clamp(200_000.0, 40_000_000.0) as u64
}

// ------------------------------------------------------------------ containers

struct Container {
    muxer: &'static str,
    ext: &'static str,
    video: &'static [&'static str],
    audio: &'static [&'static str],
}

fn container(target: &str) -> Container {
    match target {
        "MP4" => Container { muxer: "mp4", ext: "mp4", video: &["h264", "hevc", "av1", "mpeg4"], audio: &["aac", "mp3", "alac", "ac3", "eac3", "opus", "flac"] },
        "MOV" => Container { muxer: "mov", ext: "mov", video: &["h264", "hevc", "prores", "mjpeg", "mpeg4"], audio: &["aac", "alac", "mp3", "pcm_s16le", "pcm_s24le", "ac3"] },
        "MKV" => Container { muxer: "matroska", ext: "mkv", video: &["h264", "hevc", "av1", "vp8", "vp9", "mpeg4", "mpeg2video", "prores", "mjpeg", "theora", "ffv1"],
                             audio: &["aac", "mp3", "opus", "vorbis", "flac", "alac", "ac3", "eac3", "dts", "truehd", "pcm_s16le", "pcm_s24le"] },
        "WEBM" => Container { muxer: "webm", ext: "webm", video: &["vp8", "vp9", "av1"], audio: &["opus", "vorbis"] },
        "AVI" => Container { muxer: "avi", ext: "avi", video: &["mpeg4", "msmpeg4v3", "mjpeg"], audio: &["ac3", "pcm_s16le"] },
        "GIF" => Container { muxer: "gif", ext: "gif", video: &[], audio: &[] },
        "MP3" => Container { muxer: "mp3", ext: "mp3", video: &[], audio: &["mp3"] },
        "M4A" => Container { muxer: "ipod", ext: "m4a", video: &[], audio: &["aac"] },
        "ALAC" => Container { muxer: "ipod", ext: "m4a", video: &[], audio: &["alac"] },
        "WAV" => Container { muxer: "wav", ext: "wav", video: &[], audio: &["pcm_s16le", "pcm_s24le", "pcm_f32le"] },
        "FLAC" => Container { muxer: "flac", ext: "flac", video: &[], audio: &["flac"] },
        "OGG" => Container { muxer: "ogg", ext: "ogg", video: &[], audio: &["vorbis", "opus", "flac"] },
        "OPUS" => Container { muxer: "opus", ext: "opus", video: &[], audio: &["opus"] },
        "AIFF" => Container { muxer: "aiff", ext: "aiff", video: &[], audio: &["pcm_s16be", "pcm_s24be"] },
        "WMA" => Container { muxer: "asf", ext: "wma", video: &[], audio: &["wmav2", "wmav1"] },
        _ => Container { muxer: "mp4", ext: "mp4", video: &["h264"], audio: &["aac"] },
    }
}

const AUDIO_ONLY: &[&str] = &["MP3", "M4A", "ALAC", "WAV", "FLAC", "OGG", "OPUS", "AIFF", "WMA"];
const LOSSY: &[&str] = &["mp3", "aac", "vorbis", "opus", "wmav1", "wmav2", "ac3", "eac3"];

/// (target, ext) for tools that keep the source's format where they can.
fn keep_target(src: &Path, info: &Info) -> (&'static str, String) {
    let e = ext_lower(src);
    let t = match e.as_str() {
        "mp4" | "m4v" => "MP4",
        "mov" => "MOV",
        "mkv" => "MKV",
        "webm" => "WEBM",
        "avi" => "AVI",
        "mp3" => "MP3",
        "m4a" | "aac" => if info.audio.first().map(|a| a.codec == "alac").unwrap_or(false) { "ALAC" } else { "M4A" },
        "wav" => "WAV",
        "flac" => "FLAC",
        "ogg" | "oga" => "OGG",
        "opus" => "OPUS",
        "aiff" | "aif" => "AIFF",
        "wma" => "WMA",
        _ => "",
    };
    let is_audio = info.video.is_empty();
    let fits = !t.is_empty() && AUDIO_ONLY.contains(&t) == is_audio;
    let t = if fits { t } else if is_audio { "M4A" } else { "MP4" };
    let ext = if fits && e != "aac" { e } else { container(t).ext.to_string() };
    (t, ext)
}

fn pcm(bits: u32, endian: &str) -> String {
    format!("pcm_s{}{endian}", if bits > 16 { 24 } else { 16 })
}

fn audio_codec(target: &str, a: &Stream, kbps: Option<u32>) -> Res<Vec<String>> {
    let need = |enc: &str, what: &str| -> Res<String> {
        if has(enc) { Ok(enc.into()) } else { Err(tf("This copy of ffmpeg can't make {} (it has no {} encoder).", &[what, enc])) }
    };
    Ok(match target {
        "MP4" | "MOV" | "MKV" | "M4A" => vec![s("-c:a"), s("aac"), s("-b:a"), format!("{}k", kbps.unwrap_or(192))],
        "WEBM" | "OPUS" => vec![s("-c:a"), need("libopus", "Opus")?, s("-b:a"), format!("{}k", kbps.unwrap_or(128))],
        "MP3" | "AVI" => {
            let mut v = vec![s("-c:a"), need("libmp3lame", "MP3")?];
            match kbps {
                Some(k) => v.extend([s("-b:a"), format!("{k}k")]),
                None if target == "AVI" => v.extend([s("-b:a"), s("192k")]), // AVI players choke on VBR
                None => v.extend([s("-q:a"), s(2)]), // LAME's ~190 kbps VBR
            }
            if a.channels > 2 {
                v.extend([s("-ac"), s(2)]);
            }
            v
        }
        "OGG" => {
            if has("libvorbis") {
                match kbps {
                    Some(k) => vec![s("-c:a"), s("libvorbis"), s("-b:a"), format!("{k}k")],
                    None => vec![s("-c:a"), s("libvorbis"), s("-q:a"), s(5)],
                }
            } else {
                vec![s("-c:a"), need("libopus", "OGG")?, s("-b:a"), format!("{}k", kbps.unwrap_or(128))]
            }
        }
        "WMA" => vec![s("-c:a"), s("wmav2"), s("-b:a"), format!("{}k", kbps.unwrap_or(192))],
        "ALAC" => vec![s("-c:a"), s("alac")],
        "WAV" => vec![s("-c:a"), pcm(a.bits, "le")],
        "AIFF" => vec![s("-c:a"), pcm(a.bits, "be")],
        "FLAC" => vec![s("-c:a"), s("flac")],
        _ => return Err(format!("no audio codec for {target}")),
    })
}

fn audio_args(target: &str, streams: &[Stream], af: &[String], kbps: Option<u32>, copy: bool) -> Res<Vec<String>> {
    let allowed = container(target).audio;
    if copy && af.is_empty() && kbps.is_none() && streams.iter().all(|s| allowed.contains(&s.codec.as_str())) {
        return Ok(vec![s("-c:a"), s("copy")]);
    }
    let mut args = audio_codec(target, &streams[0], kbps)?;
    if !af.is_empty() {
        args.extend([s("-af"), af.join(",")]);
        let opus = args.iter().any(|a| a == "libopus");
        let rate = if opus { 48000 } else { streams[0].sample_rate };
        if rate > 0 {
            args.extend([s("-ar"), s(rate)]);
        }
    } else if args.iter().any(|a| a == "libopus") {
        args.extend([s("-ar"), s(48000)]);
    }
    Ok(args)
}

// ------------------------------------------------------------------ running

pub struct Ctl {
    pub cancel: Arc<AtomicBool>,
    pub progress: Box<dyn Fn(f32) + Send + Sync>,
}

fn explain(tail: &VecDeque<String>) -> String {
    let text = tail.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>().join(" ");
    if text.contains("no space left") {
        t("The disk is full.").into()
    } else if text.contains("permission denied") {
        t("No permission to write next to this file. Try a folder you own, or set Settings → Save results → subfolder.").into()
    } else if text.contains("error writing trailer") || text.contains("could not write header") {
        t("ffmpeg couldn't process this file.").into()
    } else if text.contains("invalid data found") || text.contains("moov atom not found") || text.contains("does not contain any stream") {
        unreadable()
    } else {
        t("ffmpeg couldn't process this file.").into()
    }
}

const CANCELLED: &str = "\u{1}cancelled";

/// Not a failure: the file is already what was asked for. Shown as a calm info card.
const NOTE: char = '\u{1}';

pub fn note(msg: String) -> String {
    format!("{NOTE}{msg}")
}

pub fn note_text(e: &str) -> Option<&str> {
    e.strip_prefix(NOTE)
}

pub fn is_cancel(e: &str) -> bool {
    e == CANCELLED
}

pub fn cancelled() -> String {
    CANCELLED.into()
}

/// Run ffmpeg with progress: `span` maps this run onto a part of the job's progress bar.
fn exec(args: Vec<String>, duration: f64, ctl: &Ctl, span: (f32, f32)) -> Res<()> {
    let mut full = vec![s("-nostdin"), s("-y"), s("-hide_banner"), s("-loglevel"), s("error"), s("-nostats"),
                        s("-stats_period"), s("0.25"), s("-progress"), s("pipe:1")];
    full.extend(args);
    let mut child = command(&full)?.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| {
        crate::logf!("ffmpeg could not start: {e}");
        missing()
    })?;
    let tail = Arc::new(Mutex::new(VecDeque::with_capacity(12)));
    let mut err = child.stderr.take().unwrap();
    let tail2 = tail.clone();
    let er = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        let mut g = tail2.lock().unwrap();
        for l in String::from_utf8_lossy(&buf).lines().filter(|l| !l.trim().is_empty()) {
            if g.len() == 12 {
                g.pop_front();
            }
            g.push_back(l.trim().to_string());
        }
    });
    let out = child.stdout.take().unwrap();
    let progress = &ctl.progress;
    let cancel = ctl.cancel.clone();
    let pid = child.id();
    // the watcher kills ffmpeg promptly on Cancel, even if it is quiet
    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();
    let watcher = std::thread::spawn(move || {
        while !done2.load(Ordering::Relaxed) {
            if cancel.load(Ordering::Relaxed) {
                kill_pid(pid);
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let mut last = -1.0f32;
    for line in BufReader::new(out).lines().map_while(Result::ok) {
        let (k, v) = line.split_once('=').unwrap_or(("", ""));
        let frac = match k {
            "out_time_us" | "out_time_ms" if duration > 0.0 => v.trim().parse::<f64>().ok().map(|us| (us / 1e6 / duration).clamp(0.0, 1.0) as f32),
            "progress" if v == "end" => Some(1.0),
            _ => None,
        };
        if let Some(f) = frac {
            if f - last >= 0.005 || f == 1.0 {
                last = f;
                progress(span.0 + (span.1 - span.0) * f);
            }
        }
    }
    let status = child.wait();
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let _ = er.join();
    if ctl.cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    match status {
        Ok(st) if st.success() => Ok(()),
        _ => {
            let g = tail.lock().unwrap();
            crate::logf!("ffmpeg failed: {}\n  args: {:?}", g.back().cloned().unwrap_or_default(), full);
            Err(explain(&g))
        }
    }
}

fn kill_pid(pid: u32) {
    use windows::Win32::System::Threading::*;
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(h, 1);
            let _ = windows::Win32::Foundation::CloseHandle(h);
        }
    }
}

/// Write through a temp file beside the target, so a failed or cancelled job leaves nothing.
fn write_to(out: &Path, muxer: &str, f: impl FnOnce(&Path, &[String]) -> Res<()>) -> Res<PathBuf> {
    let dir = out.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".{}.dragshift-{}.part", out.file_stem().unwrap_or_default().to_string_lossy(), std::process::id()));
    let tail = vec![s("-f"), s(muxer), file_arg(&tmp)];
    let r = f(&tmp, &tail);
    match r {
        Ok(()) if file_size(&tmp) > 0 => {
            let dest = util::unique_path(out.to_path_buf());
            std::fs::rename(&tmp, &dest).map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                e.to_string()
            })?;
            Ok(dest)
        }
        Ok(()) => {
            let _ = std::fs::remove_file(&tmp);
            Err(t("ffmpeg finished but didn't write anything.").into())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// ------------------------------------------------------------------ operations

#[derive(Clone, Debug)]
pub enum Op {
    Convert(String),
    SizeMb(u64),
    SizePercent(u32),
    Bitrate(u32),
    Rotate,
    Mute,
    ExtractAudio,
    Frame(Option<f64>),
    Trim { start: f64, end: f64, fade_in: f64, fade_out: f64, fast: bool },
    Crop { x: u32, y: u32, w: u32, h: u32 },
    Speed { factor: f64, keep_pitch: bool },
    Encode(VideoPreset),
}

impl Op {
    pub fn from_key(key: &str) -> Option<Op> {
        let (kind, name) = key.split_once(':')?;
        Some(match (kind, name) {
            ("convert", f) => Op::Convert(f.to_string()),
            ("size", p) if p.ends_with('%') => Op::SizePercent(p.trim_end_matches('%').parse().ok()?),
            ("size", mb) => Op::SizeMb(mb.parse().ok()?),
            ("kbps", k) => Op::Bitrate(k.parse().ok()?),
            ("enc", spec) => Op::Encode(VideoPreset::from_spec(spec)?),
            ("tool", "Rotate") => Op::Rotate,
            ("tool", "Mute") => Op::Mute,
            ("tool", "Audio") => Op::ExtractAudio,
            ("tool", "Frame") => Op::Frame(None),
            _ => return None,
        })
    }

    pub fn working_text(&self) -> String {
        match self {
            Op::Convert(f) => tf("Converting to {}…", &[f]),
            Op::SizeMb(mb) => tf("Compressing to {} MB…", &[&mb.to_string()]),
            Op::SizePercent(_) | Op::Bitrate(_) | Op::Encode(_) => t("Compressing…").into(),
            Op::Rotate => t("Rotating…").into(),
            Op::Mute => t("Removing the sound…").into(),
            Op::ExtractAudio => t("Saving the soundtrack…").into(),
            Op::Frame(_) => t("Saving a frame…").into(),
            Op::Trim { .. } => t("Trimming…").into(),
            Op::Crop { .. } => t("Cropping…").into(),
            Op::Speed { .. } => t("Changing the speed…").into(),
        }
    }

    /// Ops that finish in a blink (stream copies) don't need a Cancel button.
    pub fn cancellable(&self) -> bool {
        !matches!(self, Op::Frame(_) | Op::Mute | Op::ExtractAudio)
    }
}

struct Transcode<'a> {
    vf: Vec<String>,
    af: Vec<String>,
    q: Quality,
    kbps: Option<u32>,
    copy: bool,
    mute: bool,
    start: Option<f64>,
    length: Option<f64>,
    duration: Option<f64>,
    gpu: bool,
    vc: Vc,
    subs: bool,
    /// a custom preset's sound: codec (aac | opus | mp3 | copy) and kbps
    audio: Option<(String, u32)>,
    extra: Vec<String>,
    input_opts: Vec<String>,
    _p: std::marker::PhantomData<&'a ()>,
}

impl Default for Transcode<'_> {
    fn default() -> Self {
        Transcode { vf: vec![], af: vec![], q: Quality::Crf(CRF_CONVERT), kbps: None, copy: true, mute: false,
                    start: None, length: None, duration: None, gpu: true, vc: Vc::H264, subs: true, audio: None,
                    extra: vec![], input_opts: vec![], _p: std::marker::PhantomData }
    }
}

fn require_video(info: &Info) -> Res<&Stream> {
    info.video.first().ok_or_else(|| t("This file has no video, only sound.").into())
}

fn require_audio(info: &Info) -> Res<&Stream> {
    info.audio.first().ok_or_else(|| t("This video has no sound.").into())
}

fn video_codec_args(target: &str, info: &Info, q: Quality, gpu: bool, vc: Vc) -> Res<(Vec<String>, (Vc, Enc))> {
    Ok(match target {
        "WEBM" => {
            if !has("libvpx-vp9") {
                return Err(tf("This copy of ffmpeg can't make {} (it has no {} encoder).", &["WebM", "libvpx-vp9"]));
            }
            let quality = match q {
                Quality::Crf(c) => vec![s("-crf"), s(c + 13), s("-b:v"), s(0)],
                Quality::Bitrate(b) => vec![s("-b:v"), s(b), s("-maxrate"), s(b * 3 / 2), s("-bufsize"), s(b * 2)],
            };
            let mut v = vec![s("-c:v"), s("libvpx-vp9")];
            v.extend(quality);
            v.extend(["-row-mt", "1", "-deadline", "good", "-cpu-used", "5", "-pix_fmt", "yuv420p"].iter().map(|x| s(x)));
            (v, (Vc::H264, Enc::Mpeg4))
        }
        "AVI" => {
            let mut v = vec![s("-c:v"), s("mpeg4"), s("-vtag"), s("XVID")];
            match q {
                Quality::Crf(_) => v.extend([s("-q:v"), s(4)]),
                Quality::Bitrate(b) => v.extend([s("-b:v"), s(b)]),
            }
            v.extend([s("-pix_fmt"), s("yuv420p")]);
            (v, (Vc::H264, Enc::Mpeg4))
        }
        _ => {
            let (vc, enc) = pick_encoder(vc, gpu);
            let mut v = video_args(vc, enc, q, info);
            if vc == Vc::Hevc && (target == "MP4" || target == "MOV") {
                v.extend([s("-tag:v"), s("hvc1")]); // Apple players
            }
            (v, (vc, enc))
        }
    })
}

const TEXT_SUBS: &[&str] = &["subrip", "srt", "ass", "ssa", "mov_text", "webvtt", "text"];

/// Subtitle (and MKV font) streams the target can carry; picture subtitles only fit in MKV.
fn sub_args(target: &str, info: &Info) -> Vec<String> {
    let mut a = vec![];
    let to = match target {
        "MKV" => None,
        "MP4" | "MOV" => Some("mov_text"),
        "WEBM" => Some("webvtt"),
        _ => return a,
    };
    let mut n = 0;
    for st in &info.subs {
        let text = TEXT_SUBS.contains(&st.codec.as_str());
        if to.is_some() && !text {
            continue;
        }
        let codec = match to {
            Some(c) => c,
            None if st.codec == "mov_text" => "srt", // MKV has no mov_text
            None => "copy",
        };
        a.extend([s("-map"), format!("0:{}", st.index), format!("-c:s:{n}"), s(codec)]);
        n += 1;
    }
    if target == "MKV" && info.attachments {
        a.extend([s("-map"), s("0:t"), s("-c:t"), s("copy")]);
    }
    a
}

/// Sound for a custom preset: the asked codec where the container takes it.
fn preset_audio(target: &str, streams: &[Stream], codec: &str, kbps: u32) -> Res<Vec<String>> {
    let allowed = container(target).audio;
    let k = format!("{}k", kbps.clamp(32, 512));
    let codec = if target == "WEBM" && !(codec == "copy" && streams.iter().all(|a| allowed.contains(&a.codec.as_str()))) {
        "opus"
    } else {
        codec
    };
    let mut v = match codec {
        "copy" if streams.iter().all(|a| allowed.contains(&a.codec.as_str())) => return Ok(vec![s("-c:a"), s("copy")]),
        "opus" if has("libopus") => vec![s("-c:a"), s("libopus"), s("-b:a"), k, s("-ar"), s(48000)],
        "mp3" if has("libmp3lame") => vec![s("-c:a"), s("libmp3lame"), s("-b:a"), k],
        _ if target == "WEBM" => return Err(tf("This copy of ffmpeg can't make {} (it has no {} encoder).", &["WebM", "libopus"])),
        "copy" => vec![s("-c:a"), s("aac"), s("-b:a"), format!("{}k", streams[0].kbps.clamp(128, 320))],
        _ => vec![s("-c:a"), s("aac"), s("-b:a"), k],
    };
    if codec == "mp3" && streams[0].channels > 2 {
        v.extend([s("-ac"), s(2)]);
    }
    Ok(v)
}

fn transcode(src: &Path, info: &Info, target: &str, out: &Path, o: Transcode, ctl: &Ctl) -> Res<PathBuf> {
    let c = container(target);
    let mut args: Vec<String> = o.input_opts.clone();
    if let Some(st) = o.start {
        args.extend([s("-ss"), format!("{st:.3}")]);
    }
    if let Some(l) = o.length {
        args.extend([s("-t"), format!("{l:.3}")]);
    }
    args.extend([s("-i"), file_arg(src)]);
    let mut two_pass = false;
    let mut enc_used = Enc::Mpeg4;
    let mut with_subs = false;
    if AUDIO_ONLY.contains(&target) {
        let a = require_audio(info)?;
        args.extend([s("-map"), format!("0:{}", a.index), s("-vn"), s("-sn"), s("-dn")]);
        args.extend(audio_args(target, std::slice::from_ref(a), &o.af, o.kbps, o.copy)?);
    } else {
        let v = require_video(info)?;
        args.extend([s("-map"), format!("0:{}", v.index), s("-dn")]);
        if o.copy && o.vf.is_empty() && c.video.contains(&v.codec.as_str()) {
            args.extend([s("-c:v"), s("copy")]);
            if v.codec == "hevc" && (target == "MP4" || target == "MOV") {
                args.extend([s("-tag:v"), s("hvc1")]); // Apple players
            }
        } else {
            let mut vf = o.vf.clone();
            vf.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".into()); // 4:2:0 needs even sizes
            args.extend([s("-vf"), vf.join(",")]);
            let (va, (vc, enc)) = video_codec_args(target, info, o.q, o.gpu, o.vc)?;
            enc_used = enc;
            two_pass = vc == Vc::H264 && enc == Enc::Cpu && matches!(o.q, Quality::Bitrate(_));
            args.extend(va);
        }
        if o.mute || info.audio.is_empty() {
            args.push(s("-an"));
        } else {
            args.extend([s("-map"), s("0:a")]);
            match &o.audio {
                Some((codec, kbps)) => args.extend(preset_audio(target, &info.audio, codec, *kbps)?),
                None => args.extend(audio_args(target, &info.audio, &o.af, o.kbps, o.copy)?),
            }
        }
        let subs = if o.subs { sub_args(target, info) } else { vec![] };
        with_subs = !subs.is_empty();
        if with_subs {
            args.extend(subs);
        } else {
            args.push(s("-sn"));
        }
        if target == "MP4" || target == "MOV" {
            args.extend([s("-movflags"), s("+faststart")]);
        }
    }
    args.extend(o.extra.clone());
    let duration = o.duration.or(o.length).unwrap_or(info.duration);
    let run = |args: &Vec<String>| -> Res<PathBuf> {
        write_to(out, c.muxer, |_tmp, tail| {
            if two_pass {
                let log = std::env::temp_dir().join(format!("dragshift-2pass-{}", std::process::id()));
                let mut p1: Vec<String> = args.iter().cloned().filter(|a| a != "+faststart").collect();
                // pass 1 needs no audio and writes nowhere
                if let Some(i) = p1.iter().position(|a| a == "-movflags") {
                    p1.remove(i);
                }
                let mut p1b = p1.clone();
                p1b.extend([s("-pass"), s(1), s("-passlogfile"), log.display().to_string(), s("-an"), s("-f"), s("null"), s("NUL")]);
                exec(p1b, duration, ctl, (0.0, 0.5))?;
                let mut p2 = args.clone();
                p2.extend([s("-pass"), s(2), s("-passlogfile"), log.display().to_string()]);
                p2.extend(tail.iter().cloned());
                let r = exec(p2, duration, ctl, (0.5, 1.0));
                for ext in ["-0.log", "-0.log.mbtree"] {
                    let _ = std::fs::remove_file(format!("{}{ext}", log.display()));
                }
                r
            } else {
                let mut a = args.clone();
                a.extend(tail.iter().cloned());
                exec(a, duration, ctl, (0.0, 1.0))
            }
        })
    };
    match run(&args) {
        Err(e) if !is_cancel(&e) && matches!(enc_used, Enc::Nvenc | Enc::Qsv | Enc::Amf) => {
            // a GPU encoder that passed the test can still fail on a real file: fall back to the CPU
            crate::logf!("GPU encode failed ({e}); retrying on the CPU");
            let o2 = Transcode { gpu: false, ..o };
            transcode(src, info, target, out, o2, ctl)
        }
        Err(e) if !is_cancel(&e) && with_subs => {
            // an odd subtitle stream the target won't take shouldn't cost the whole video
            crate::logf!("encode with subtitles failed ({e}); retrying without them");
            let o2 = Transcode { subs: false, ..o };
            transcode(src, info, target, out, o2, ctl)
        }
        r => r,
    }
}

fn gif(src: &Path, info: &Info, out: &Path, ctl: &Ctl) -> Res<PathBuf> {
    let v = require_video(info)?;
    if info.duration > GIF_MAX_SECONDS {
        return Err(tf("This clip is {} long. GIFs work best under {} seconds, so trim it first.",
                      &[&fmt_duration(info.duration), &(GIF_MAX_SECONDS as u32).to_string()]));
    }
    let fps = if v.fps > 0.0 { v.fps.min(GIF_FPS).round() } else { GIF_FPS };
    let graph = format!("[0:{}]fps={fps},scale='min({GIF_WIDTH},iw)':-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4[out]", v.index);
    let args = vec![s("-i"), file_arg(src), s("-filter_complex"), graph, s("-map"), s("[out]"), s("-an"), s("-loop"), s(0)];
    write_to(out, "gif", |_, tail| {
        let mut a = args.clone();
        a.extend(tail.iter().cloned());
        exec(a, info.duration, ctl, (0.0, 1.0))
    })
}

fn convert(src: &Path, info: &Info, target: &str, ctl: &Ctl) -> Res<PathBuf> {
    let c = container(target);
    let out = sibling(src, "", c.ext);
    if target == "GIF" {
        return gif(src, info, &out, ctl);
    }
    // keep album art and tags when going between audio formats that can hold a picture
    let mut o = Transcode::default();
    if info.video.is_empty() && matches!(target, "MP3" | "M4A" | "ALAC" | "FLAC") && has_cover(src) {
        o.extra = vec![s("-map"), s("0:v?"), s("-c:v"), s("copy"), s("-disposition:v"), s("attached_pic")];
        if target == "MP3" {
            o.extra.extend([s("-id3v2_version"), s(3)]);
        }
        match transcode(src, info, target, &out, o, ctl) {
            Ok(p) => return Ok(p),
            Err(e) if is_cancel(&e) => return Err(e),
            Err(_) => o = Transcode::default(),
        }
    }
    transcode(src, info, target, &out, o, ctl)
}

fn has_cover(src: &Path) -> bool {
    command(&[s("-hide_banner"), s("-nostdin"), s("-i"), file_arg(src)]).ok()
        .and_then(|mut c| c.stdout(Stdio::null()).stderr(Stdio::piped()).output().ok())
        .map(|o| String::from_utf8_lossy(&o.stderr).contains("(attached pic)"))
        .unwrap_or(false)
}

/// The biggest resolution a bitrate can carry cleanly (≥ 0.05 bits per pixel per frame in H.264;
/// newer codecs get by on less).
fn plan_scale(info: &Info, video_bps: f64, vc: Vc) -> (Option<u32>, Option<f32>) {
    let Some((w, h)) = info.size() else { return (None, None) };
    let video_bps = video_bps / rate_share(vc);
    let fps = info.video[0].fps.max(1.0);
    let short = w.min(h);
    let mut fps_cap = None;
    let mut f = fps as f64;
    if video_bps / (w as f64 * h as f64 * f) < 0.05 && fps > 31.0 {
        fps_cap = Some(30.0);
        f = 30.0;
    }
    for target in [short, 1080, 720, 540, 480, 360, 240] {
        if target > short {
            continue;
        }
        let k = target as f64 / short as f64;
        let px = w as f64 * k * h as f64 * k;
        if video_bps / (px * f) >= 0.05 || target == 240 {
            return (if target < short { Some(target) } else { None }, fps_cap);
        }
    }
    (None, fps_cap)
}

fn compress_to(src: &Path, info: &Info, target_bytes: u64, label: &str, ctl: &Ctl) -> Res<PathBuf> {
    if info.duration <= 0.0 {
        return Err(t("DragShift can't tell how long this file is, so it can't aim for a size.").into());
    }
    let size = file_size(src);
    if size <= target_bytes {
        return Err(note(tf("It's {}, already under {}.", &[&human_size(size), label])));
    }
    // MKV stays MKV so its subtitle tracks and fonts survive
    let target = if ext_lower(src) == "mkv" { "MKV" } else { "MP4" };
    let out = sibling(src, label, container(target).ext);
    let vc = Vc::from_name(&crate::settings::get().video_codec);
    // subtitles and fonts ride along as they are, so they come out of the budget
    let extra = if target == "MKV" { side_streams_size(src, info, ctl) } else { 0 };
    let total = target_bytes.saturating_sub(extra) as f64 * 8.0 / info.duration;
    let akbps: u32 = if info.audio.is_empty() { 0 } else if total > 2.0e6 { 128 } else if total > 800e3 { 96 } else if total > 300e3 { 64 } else { 48 };
    let mut video = total * 0.96 - akbps as f64 * 1000.0;
    if video < MIN_VIDEO_BPS {
        return Err(tf("This video is too long to fit in {}. Trim it first or pick a bigger size.", &[&human_size(target_bytes)]));
    }
    let mut attempt = 0;
    loop {
        let (scale, fps) = plan_scale(info, video, vc);
        let mut vf = vec![];
        if let Some(sh) = scale {
            vf.push(scale_short(info, sh));
        }
        if let Some(f) = fps {
            vf.push(format!("fps={f}"));
        }
        let o = Transcode { vf, q: Quality::Bitrate(video as u64), kbps: (akbps > 0).then_some(akbps), copy: false, vc,
                            ..Default::default() };
        let p = transcode(src, info, target, &out, o, ctl)?;
        let got = file_size(&p);
        if got <= target_bytes || attempt >= 1 {
            return Ok(p);
        }
        // over the limit: aim lower and try once more
        crate::logf!("compress: {} > {}, retrying", got, target_bytes);
        let _ = std::fs::remove_file(&p);
        video *= target_bytes as f64 / got as f64 * 0.93;
        attempt += 1;
    }
}

/// Bytes the subtitle and font streams take in MKV (copied out once to find out).
fn side_streams_size(src: &Path, info: &Info, ctl: &Ctl) -> u64 {
    if info.subs.is_empty() && !info.attachments {
        return 0;
    }
    let mut args = vec![s("-i"), file_arg(src), s("-vn"), s("-an")];
    args.extend(sub_args("MKV", info));
    let tmp = std::env::temp_dir().join(format!("dragshift-side-{}.mkv", std::process::id()));
    args.extend([s("-f"), s("matroska"), file_arg(&tmp)]);
    let ok = exec(args, 0.0, ctl, (0.0, 0.0)).is_ok();
    let n = if ok { file_size(&tmp) } else { 0 };
    let _ = std::fs::remove_file(&tmp);
    n
}

/// A scale filter that brings the short side down to `short`, keeping the aspect.
fn scale_short(info: &Info, short: u32) -> String {
    let (w, h) = info.size().unwrap_or((0, 0));
    if w >= h { format!("scale=-2:{short}") } else { format!("scale={short}:-2") }
}

const CRF_PRESET: u32 = 23;

/// A custom preset from the Compress ring: the codecs, bitrates and size the user picked.
fn encode(src: &Path, info: &Info, p: &VideoPreset, ctl: &Ctl) -> Res<PathBuf> {
    require_video(info)?;
    let target = if p.vcodec == "vp9" { "WEBM" } else if ext_lower(src) == "mkv" { "MKV" } else { "MP4" };
    let mut vf = vec![];
    if let Some((w, h)) = info.size() {
        if p.height > 0 && p.height < w.min(h) {
            vf.push(scale_short(info, p.height));
        }
    }
    let mut label = vec![p.codec_name().to_string()];
    if p.height > 0 {
        label.push(format!("{}p", p.height));
    }
    let o = Transcode {
        vf,
        q: if p.vkbps > 0 { Quality::Bitrate(p.vkbps as u64 * 1000) } else { Quality::Crf(CRF_PRESET) },
        copy: false,
        vc: Vc::from_name(&p.vcodec),
        audio: Some((p.acodec.clone(), p.akbps)),
        ..Default::default()
    };
    transcode(src, info, target, &sibling(src, &label.join(" "), container(target).ext), o, ctl)
}

fn bitrate(src: &Path, info: &Info, kbps: u32, ctl: &Ctl) -> Res<PathBuf> {
    let a = require_audio(info)?;
    let (mut target, mut ext) = keep_target(src, info);
    let lossy_home = matches!(target, "MP3" | "M4A" | "OGG" | "OPUS" | "WMA") && LOSSY.contains(&a.codec.as_str());
    if !lossy_home || !info.video.is_empty() {
        target = "MP3";
        ext = "mp3".into();
    }
    // re-encoding lossy audio at a higher rate only makes it bigger, never better
    let src_kbps = if a.kbps > 0 { a.kbps } else if info.video.is_empty() { info.kbps } else { 0 };
    if LOSSY.contains(&a.codec.as_str()) && src_kbps > 0 && kbps * 100 >= src_kbps * 95 {
        return Err(note(tf("It's already {} kbps. A higher bitrate would only make the file bigger, not better.",
                           &[&src_kbps.to_string()])));
    }
    let o = Transcode { kbps: Some(kbps), copy: false, ..Default::default() };
    transcode(src, info, target, &sibling(src, &format!("{kbps}k"), &ext), o, ctl)
}

const EXTRACT: &[(&str, &str)] = &[("aac", "M4A"), ("alac", "ALAC"), ("mp3", "MP3"), ("opus", "OPUS"), ("vorbis", "OGG"),
                                    ("flac", "FLAC"), ("pcm_s16le", "WAV"), ("pcm_s24le", "WAV")];

fn rotate(src: &Path, info: &Info, ctl: &Ctl) -> Res<PathBuf> {
    let v = require_video(info)?;
    let (target, ext) = keep_target(src, info);
    let out = sibling(src, t("Rotated"), &ext);
    if matches!(target, "MP4" | "MOV") {
        // lossless and instant: only the display matrix changes
        let mut rot = v.rotation - 90;
        if rot <= -180 {
            rot += 360;
        }
        let o = Transcode { input_opts: vec![s("-display_rotation:v:0"), s(rot)], ..Default::default() };
        match transcode(src, info, target, &out, o, ctl) {
            Ok(p) => return Ok(p),
            Err(e) if is_cancel(&e) => return Err(e),
            Err(e) => crate::logf!("lossless rotate failed ({e}), re-encoding"),
        }
    }
    let o = Transcode { vf: vec![s("transpose=1")], copy: false, ..Default::default() };
    transcode(src, info, target, &out, o, ctl)
}

fn atempo(mut f: f64) -> Vec<String> {
    let mut steps = vec![];
    while f > 2.0 {
        steps.push(2.0);
        f /= 2.0;
    }
    while f < 0.5 {
        steps.push(0.5);
        f /= 0.5;
    }
    steps.push(f);
    steps.iter().map(|x| format!("atempo={x:.6}")).collect()
}

pub fn run(op: &Op, src: &Path, ctl: &Ctl) -> Res<Vec<PathBuf>> {
    let info = probe(src)?;
    let out = match op {
        Op::Convert(f) => convert(src, &info, f, ctl)?,
        Op::SizeMb(mb) => compress_to(src, &info, mb * 1_000_000 * 97 / 100, &format!("{mb} {}", t("MB")), ctl)?,
        Op::SizePercent(p) => {
            let target = file_size(src) * *p as u64 / 100;
            if info.video.is_empty() {
                let k = ((info.audio[0].kbps.max(info.kbps).max(128)) * p / 100).clamp(32, 320);
                bitrate(src, &info, k, ctl)?
            } else {
                compress_to(src, &info, target, &format!("{p}%"), ctl)?
            }
        }
        Op::Bitrate(k) => bitrate(src, &info, *k, ctl)?,
        Op::Encode(p) => encode(src, &info, p, ctl)?,
        Op::Rotate => rotate(src, &info, ctl)?,
        Op::Mute => {
            require_video(&info)?;
            let (target, ext) = keep_target(src, &info);
            transcode(src, &info, target, &sibling(src, t("Muted"), &ext), Transcode { mute: true, ..Default::default() }, ctl)?
        }
        Op::ExtractAudio => {
            let a = require_audio(&info)?;
            let target = EXTRACT.iter().find(|(c, _)| *c == a.codec).map(|(_, t)| *t).unwrap_or("M4A");
            transcode(src, &info, target, &sibling(src, "", container(target).ext), Transcode::default(), ctl)?
        }
        Op::Frame(at) => {
            let v = require_video(&info)?;
            let at = at.unwrap_or(info.duration / 2.0).clamp(0.0, (info.duration - 0.05).max(0.0));
            let args = vec![s("-ss"), format!("{at:.3}"), s("-i"), file_arg(src), s("-map"), format!("0:{}", v.index),
                            s("-frames:v"), s(1), s("-update"), s(1), s("-c:v"), s("png")];
            write_to(&sibling(src, t("Frame"), "png"), "image2", |_, tail| {
                let mut a = args.clone();
                a.extend(tail.iter().cloned());
                exec(a, 0.0, ctl, (0.0, 1.0))
            })?
        }
        Op::Trim { start, end, fade_in, fade_out, fast } => {
            let end = if info.duration > 0.0 { end.min(info.duration) } else { *end };
            if *start < 0.0 || end - start < 0.05 {
                return Err(t("Pick a start point before the end point.").into());
            }
            let len = end - start;
            let (target, ext) = keep_target(src, &info);
            let mut af = vec![];
            if *fade_in > 0.0 {
                af.push(format!("afade=t=in:st=0:d={fade_in:.3}"));
            }
            if *fade_out > 0.0 {
                af.push(format!("afade=t=out:st={:.3}:d={fade_out:.3}", (len - fade_out).max(0.0)));
            }
            let o = Transcode { start: Some(*start), length: Some(len), copy: *fast && af.is_empty(), af, ..Default::default() };
            transcode(src, &info, target, &sibling(src, t("Trimmed"), &ext), o, ctl)?
        }
        Op::Crop { x, y, w, h } => {
            require_video(&info)?;
            let (w, h) = (w - w % 2, h - h % 2);
            let (sw, sh) = info.size().unwrap_or((0, 0));
            if w < 2 || h < 2 || (sw > 0 && x + w > sw) || (sh > 0 && y + h > sh) {
                return Err(t("The crop area is outside the video.").into());
            }
            let (target, ext) = keep_target(src, &info);
            let o = Transcode { vf: vec![format!("crop={w}:{h}:{x}:{y}")], copy: false, ..Default::default() };
            transcode(src, &info, target, &sibling(src, t("Cropped"), &ext), o, ctl)?
        }
        Op::Speed { factor, keep_pitch } => {
            if !(0.25..=4.0).contains(factor) {
                return Err(t("Speed must be between 0.25× and 4×.").into());
            }
            let (target, ext) = keep_target(src, &info);
            let vf = if info.video.is_empty() { vec![] } else { vec![format!("setpts=PTS/{factor:.6}")] };
            let rate = info.audio.first().map(|a| a.sample_rate).unwrap_or(0);
            let af = if info.audio.is_empty() { vec![] } else if *keep_pitch || rate == 0 { atempo(*factor) } else {
                vec![format!("asetrate={}", (rate as f64 * factor).round()), format!("aresample={rate}")]
            };
            // subtitles would keep the old timing
            let o = Transcode { vf, af, copy: false, subs: false, duration: Some(info.duration / factor), ..Default::default() };
            transcode(src, &info, target, &sibling(src, &format!("{factor}x"), &ext), o, ctl)?
        }
    };
    Ok(vec![out])
}

// ------------------------------------------------------------------ summaries

pub fn summarize(op: &Op, srcs: &[PathBuf], outs: &[PathBuf]) -> Outcome {
    let before: u64 = srcs.iter().map(|p| file_size(p)).sum();
    let after: u64 = outs.iter().map(|p| file_size(p)).sum();
    let Some(o0) = outs.first() else {
        return Outcome { title: t("Nothing was written").into(), ..Default::default() };
    };
    let name = o0.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let oinfo = probe(o0).ok();
    let mut bits: Vec<String> = vec![];
    if outs.len() == 1 {
        bits.push(name.clone());
    } else {
        bits.push(tf("{} files", &[&outs.len().to_string()]));
    }
    if let Some(i) = &oinfo {
        if let Some((w, h)) = i.size() {
            bits.push(format!("{w}×{h}"));
        }
        if i.duration > 0.0 && !matches!(op, Op::Frame(_)) {
            bits.push(fmt_duration(i.duration));
        }
    }
    let pct = if before > 0 { (1.0 - after as f64 / before as f64) * 100.0 } else { 0.0 };
    let smaller = |p: f64| if p >= 1.0 { tf("{}% smaller", &[&format!("{p:.0}")]) } else { String::new() };
    let src_ext = srcs.first().map(|p| ext_lower(p).to_uppercase()).unwrap_or_default();
    let n = srcs.len();
    match op {
        Op::Convert(f) => {
            bits.push(human_size(after));
            let title = if n > 1 { tf("{} files → {}", &[&n.to_string(), f]) } else { format!("{src_ext} → {f}") };
            Outcome { title, detail: bits.join(" · "), good: String::new(), outs: outs.to_vec() }
        }
        Op::SizeMb(_) | Op::SizePercent(_) | Op::Bitrate(_) | Op::Encode(_) => {
            let sizes = format!("{} → {}", human_size(before), human_size(after));
            let mut d = vec![sizes];
            d.extend(bits.into_iter().skip(1));
            let good = if pct >= 1.0 { smaller(pct) } else { t("already well compressed").into() };
            Outcome { title: if n > 1 { tf("Compressed {} files", &[&n.to_string()]) } else { t("Compressed").into() },
                      detail: d.join(" · "), good, outs: outs.to_vec() }
        }
        _ => {
            let title = match op {
                Op::Rotate => t("Rotated 90°"),
                Op::Mute => t("Sound removed"),
                Op::ExtractAudio => t("Soundtrack saved"),
                Op::Frame(_) => t("Frame saved"),
                Op::Trim { .. } => t("Trimmed"),
                Op::Crop { .. } => t("Cropped"),
                Op::Speed { .. } => t("Speed changed"),
                _ => t("Done"),
            };
            bits.push(human_size(after));
            Outcome { title: title.into(), detail: bits.join(" · "), good: String::new(), outs: outs.to_vec() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffmpeg_banner() {
        let text = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'file:C:\\a.mp4':\n  Duration: 00:01:02.50, start: 0.000000, bitrate: 1269 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 1129 kb/s, 29.97 fps, 30 tbr, 15360 tbn (default)\n    Side data:\n      displaymatrix: rotation of -90.00 degrees\n  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 128 kb/s (default)\n";
        let i = parse_probe(text);
        assert!((i.duration - 62.5).abs() < 1e-6);
        assert_eq!(i.kbps, 1269);
        assert_eq!(i.video.len(), 1);
        assert_eq!((i.video[0].width, i.video[0].height), (1920, 1080));
        assert_eq!(i.video[0].rotation, -90);
        assert_eq!(i.size(), Some((1080, 1920)));
        assert!((i.video[0].fps - 29.97).abs() < 0.01);
        assert_eq!(i.audio[0].codec, "aac");
        assert_eq!(i.audio[0].sample_rate, 44100);
        assert_eq!(i.audio[0].channels, 2);
        assert_eq!(i.audio[0].index, 1);
    }

    #[test]
    fn skips_cover_art_and_reads_bit_depth() {
        let text = "Input #0, flac, from 'x.flac':\n  Duration: 00:03:00.00, start: 0.000000, bitrate: 900 kb/s\n  Stream #0:0: Audio: flac, 96000 Hz, stereo, s32 (24 bit)\n  Stream #0:1: Video: mjpeg (Baseline), yuvj420p(pc, bt470bg/unknown/unknown), 500x500 [SAR 1:1 DAR 1:1], 90k tbr, 90k tbn (attached pic)\n";
        let i = parse_probe(text);
        assert!(i.video.is_empty());
        assert_eq!(i.audio[0].bits, 24);
    }

    #[test]
    fn keeps_subtitles_the_target_can_hold() {
        let text = "Input #0, matroska,webm, from 'x.mkv':\n  Duration: 00:00:10.00, start: 0.000000, bitrate: 900 kb/s\n  Stream #0:0: Video: h264 (High), yuv420p(progressive), 1920x1080, 23.98 fps\n  Stream #0:1(rus): Audio: aac (LC), 48000 Hz, stereo, fltp\n  Stream #0:2(rus): Subtitle: ass (ssa) (default)\n  Stream #0:3(eng): Subtitle: hdmv_pgs_subtitle (pgssub), 1920x1080\n  Stream #0:4: Attachment: ttf\n";
        let i = parse_probe(text);
        assert_eq!(i.subs.iter().map(|s| s.codec.as_str()).collect::<Vec<_>>(), ["ass", "hdmv_pgs_subtitle"]);
        assert!(i.attachments);
        let mkv = sub_args("MKV", &i).join(" ");
        assert_eq!(mkv, "-map 0:2 -c:s:0 copy -map 0:3 -c:s:1 copy -map 0:t -c:t copy");
        assert_eq!(sub_args("MP4", &i).join(" "), "-map 0:2 -c:s:0 mov_text");
        assert!(sub_args("AVI", &i).is_empty());
    }

    #[test]
    fn preset_round_trips_through_the_menu_key() {
        let p = VideoPreset { name: "x".into(), vcodec: "av1".into(), vkbps: 2500, height: 720, acodec: "opus".into(), akbps: 96 };
        let back = VideoPreset::from_spec(p.key().strip_prefix("enc:").unwrap()).unwrap();
        assert_eq!(VideoPreset { name: "x".into(), ..back }, p);
        assert!(VideoPreset::from_spec("hevc,1,2").is_none());
    }

    /// Runs the bundled ffmpeg: `cargo test -- --ignored` with DRAGSHIFT_FFMPEG set.
    #[test]
    #[ignore]
    fn encodes_with_real_ffmpeg() {
        let dir = std::env::temp_dir().join(format!("dragshift-enc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("APPDATA", &dir);
        std::env::set_var("LOCALAPPDATA", &dir);
        let srt = dir.join("subs.srt");
        std::fs::write(&srt, "1\n00:00:00,500 --> 00:00:03,000\nПривет\n").unwrap();
        let src = dir.join("clip.mkv");
        let font = std::path::Path::new(r"C:\Windows\Fonts\arial.ttf");
        let mk = [
            "-f", "lavfi", "-i", "testsrc2=s=1280x720:r=30:d=4", "-f", "lavfi", "-i", "sine=f=440:d=4",
        ].iter().map(|x| s(x)).chain([s("-i"), srt.display().to_string(), s("-attach"), font.display().to_string()])
            .chain(["-metadata:s:t", "mimetype=application/x-truetype-font", "-map", "0", "-map", "1", "-map", "2",
                    "-c:v", "libx264", "-c:a", "aac", "-c:s", "ass", "-y"].iter().map(|x| s(x)))
            .chain([src.display().to_string()]).collect::<Vec<_>>();
        assert!(command(&mk).unwrap().stderr(Stdio::null()).status().unwrap().success());
        util::init_log();
        let ctl = Ctl { cancel: Arc::new(AtomicBool::new(false)), progress: Box::new(|_| {}) };
        let info = probe(&src).unwrap();
        assert_eq!(info.subs.len(), 1);
        assert!(info.attachments);

        for gpu in [false, true] {
            std::fs::create_dir_all(dir.join("DragShift")).unwrap();
            std::fs::write(dir.join("DragShift").join("settings.json"), format!("{{\"gpu\": {gpu}}}")).unwrap();
            crate::settings::load();
            for (spec, codec, ext) in [("hevc,0,480,opus,96", "hevc", "mkv"), ("h264,800,0,mp3,128", "h264", "mkv"),
                                       ("av1,500,360,copy,0", "av1", "mkv"), ("vp9,600,360,aac,96", "vp9", "webm")] {
                let p = VideoPreset::from_spec(spec).unwrap();
                let out = run(&Op::Encode(p), &src, &ctl).unwrap().remove(0);
                assert_eq!(ext_lower(&out), ext);
                let o = probe(&out).unwrap();
                assert_eq!(o.video[0].codec, codec, "{spec}");
                assert_eq!(o.subs.len(), 1, "{spec}: subtitles lost");
            }
            let half = run(&Op::SizePercent(50), &src, &ctl).unwrap().remove(0);
            let o = probe(&half).unwrap();
            assert_eq!((o.video[0].codec.as_str(), o.subs.len(), o.attachments), ("hevc", 1, true));
            assert!(file_size(&half) <= file_size(&src) / 2 * 102 / 100, "gpu {gpu}: {} -> {}", file_size(&src), file_size(&half));
        }
        // MP4 gets the text track as mov_text
        let mp4 = run(&Op::Convert("MP4".into()), &src, &ctl).unwrap().remove(0);
        assert_eq!(probe(&mp4).unwrap().subs[0].codec, "mov_text");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atempo_chains() {
        assert_eq!(atempo(4.0).len(), 2);
        assert_eq!(atempo(0.25).len(), 2);
        assert_eq!(atempo(1.5).len(), 1);
    }
}
