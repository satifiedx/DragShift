//! `DragShift.exe --selftest [--keep]`: makes sample clips with ffmpeg and runs every media
//! operation on them. Prints one line per check; the exit code is the number of failures.
//! `DragShift.exe --run <key> <files…>` runs one menu item without the UI.

use crate::media::{self, Ctl, Info, Op};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

fn out(line: &str) {
    let mut so = std::io::stdout();
    let _ = writeln!(so, "{line}");
    let _ = so.flush();
    crate::util::log(line);
}

pub fn attach_console() {
    unsafe {
        let _ = windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
}

fn ctl() -> Ctl {
    Ctl { cancel: Arc::new(AtomicBool::new(false)), progress: Box::new(|_| {}) }
}

fn ffmpeg(args: &[&str]) -> bool {
    let Some(f) = media::ffmpeg() else { return false };
    Command::new(f).args(["-hide_banner", "-loglevel", "error", "-y"]).args(args)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit())
        .creation_flags(0x0800_0000).status().map(|s| s.success()).unwrap_or(false)
}

fn make_samples(dir: &Path) -> Option<Vec<PathBuf>> {
    let p = |n: &str| dir.join(n);
    let v = p("clip.mp4");
    let ok = ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=1280x720:r=30:d=6", "-f", "lavfi", "-i", "sine=f=440:d=6:sample_rate=48000",
                      "-c:v", "libx264", "-preset", "ultrafast", "-g", "30", "-pix_fmt", "yuv420p", "-ac", "2", "-c:a", "aac", "-b:a", "128k",
                      "-shortest", v.to_str()?])
        && ffmpeg(&["-i", v.to_str()?, "-c", "copy", p("clip.mkv").to_str()?])
        && ffmpeg(&["-display_rotation:v:0", "90", "-i", v.to_str()?, "-c", "copy", p("phone.mp4").to_str()?])
        && ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=640x360:r=25:d=3", "-c:v", "libx264", "-preset", "ultrafast",
                    "-pix_fmt", "yuv420p", p("silent.mp4").to_str()?])
        && ffmpeg(&["-f", "lavfi", "-i", "sine=f=330:d=5:sample_rate=44100", "-ac", "2", "-c:a", "pcm_s16le", p("tone.wav").to_str()?])
        && ffmpeg(&["-i", p("tone.wav").to_str()?, "-c:a", "flac", p("tone.flac").to_str()?])
        && ffmpeg(&["-i", p("tone.wav").to_str()?, "-c:a", "libmp3lame", "-b:a", "192k", p("tone.mp3").to_str()?]);
    ok.then(|| vec![v])
}

/// `expect_ext` for checks that should end in a note instead of a file.
const NOTE: &str = "note";

struct Check {
    op: Op,
    file: &'static str,
    expect_ext: &'static str,
    expect: fn(&Info, &Info) -> Result<(), String>,
}

fn dur_near(want: f64) -> impl Fn(&Info) -> Result<(), String> {
    move |o: &Info| {
        if (o.duration - want).abs() <= 0.35 { Ok(()) } else { Err(format!("duration {:.2}, wanted {want:.2}", o.duration)) }
    }
}

fn same_len(i: &Info, o: &Info) -> Result<(), String> {
    dur_near(i.duration)(o)
}

fn has_audio_only(i: &Info, o: &Info) -> Result<(), String> {
    if !o.video.is_empty() {
        return Err("output still has video".into());
    }
    if o.audio.is_empty() {
        return Err("no audio in output".into());
    }
    same_len(i, o)
}

fn video_no_audio(i: &Info, o: &Info) -> Result<(), String> {
    if o.video.is_empty() || !o.audio.is_empty() {
        return Err(format!("video {} audio {}", o.video.len(), o.audio.len()));
    }
    same_len(i, o)
}

fn mono(i: &Info, o: &Info) -> Result<(), String> {
    if o.video.len() != i.video.len() {
        return Err(format!("video {} → {}", i.video.len(), o.video.len()));
    }
    match o.audio.first() {
        Some(a) if a.channels == 1 => same_len(i, o),
        Some(a) => Err(format!("{} channels", a.channels)),
        None => Err("no audio in output".into()),
    }
}

fn normalized(i: &Info, o: &Info) -> Result<(), String> {
    match (i.audio.first(), o.audio.first()) {
        (Some(a), Some(b)) if a.channels != b.channels => Err(format!("channels {} → {}", a.channels, b.channels)),
        (Some(a), Some(b)) if a.sample_rate != b.sample_rate => Err(format!("rate {} → {}", a.sample_rate, b.sample_rate)),
        (Some(_), Some(_)) => same_len(i, o),
        _ => Err("no audio in output".into()),
    }
}

fn video_ok(i: &Info, o: &Info) -> Result<(), String> {
    if o.video.is_empty() {
        return Err("no video".into());
    }
    if !i.audio.is_empty() && o.audio.is_empty() {
        return Err("audio lost".into());
    }
    same_len(i, o)
}

fn checks() -> Vec<Check> {
    use Op::*;
    let c = |op: Op, file, expect_ext, expect| Check { op, file, expect_ext, expect };
    let mut v = vec![];
    for (f, ext) in [("MP4", "mp4"), ("MOV", "mov"), ("WEBM", "webm"), ("AVI", "avi")] {
        v.push(c(Convert(f.into()), "clip.mkv", ext, video_ok));
    }
    v.push(c(Convert("MKV".into()), "clip.mp4", "mkv", video_ok));
    v.push(c(Convert("GIF".into()), "silent.mp4", "gif", |_, o| if o.video.is_empty() { Err("no frames".into()) } else { Ok(()) }));
    for (f, ext) in [("MP3", "mp3"), ("M4A", "m4a"), ("WAV", "wav")] {
        v.push(c(Convert(f.into()), "clip.mp4", ext, has_audio_only));
    }
    for (f, ext) in [("MP3", "mp3"), ("M4A", "m4a"), ("FLAC", "flac"), ("OGG", "ogg"), ("OPUS", "opus"), ("AIFF", "aiff"),
                     ("WMA", "wma"), ("ALAC", "m4a"), ("WAV", "wav")] {
        v.push(c(Convert(f.into()), if f == "WAV" { "tone.flac" } else { "tone.wav" }, ext, has_audio_only));
    }
    v.push(c(Bitrate(128), "tone.mp3", "mp3", |_, o| {
        let k = o.audio.first().map(|a| a.kbps).unwrap_or(0);
        if (100..=150).contains(&k) { Ok(()) } else { Err(format!("{k} kb/s")) }
    }));
    v.push(c(Bitrate(96), "tone.flac", "mp3", has_audio_only));
    v.push(c(SizePercent(50), "clip.mp4", "mp4", video_ok));
    v.push(c(SizeMb(1), "clip.mp4", "mp4", video_ok));
    // already small enough or already low bitrate: a calm note, nothing written
    v.push(c(SizeMb(10), "clip.mp4", NOTE, video_ok));
    v.push(c(Bitrate(256), "tone.mp3", NOTE, has_audio_only));
    v.push(c(Rotate, "clip.mp4", "mp4", |i, o| {
        let (w, h) = o.size().unwrap_or((0, 0));
        if (w, h) == (i.size().unwrap().1, i.size().unwrap().0) { Ok(()) } else { Err(format!("size {w}x{h}")) }
    }));
    v.push(c(Rotate, "phone.mp4", "mp4", |_, o| {
        // stored 1280x720 with a 90° tag shows as 720x1280; one more turn makes it landscape
        if o.size() == Some((1280, 720)) { Ok(()) } else { Err(format!("size {:?}", o.size())) }
    }));
    v.push(c(Rotate, "clip.mkv", "mkv", |i, o| if o.size().map(|s| s.0) == i.size().map(|s| s.1) { Ok(()) } else { Err(format!("size {:?}", o.size())) }));
    v.push(c(Mute, "clip.mp4", "mp4", video_no_audio));
    v.push(c(Mono, "clip.mp4", "mp4", mono));
    v.push(c(Mono, "clip.mkv", "mkv", mono));
    v.push(c(Mono, "tone.mp3", "mp3", mono));
    v.push(c(Mono, "tone.flac", "flac", mono));
    v.push(c(Normalize, "tone.mp3", "mp3", normalized));
    v.push(c(Normalize, "tone.flac", "flac", normalized));
    v.push(c(Normalize, "clip.mp4", "mp4", normalized));
    v.push(c(ExtractAudio, "clip.mp4", "m4a", has_audio_only));
    v.push(c(Frame(None), "clip.mp4", "png", |_, o| if o.size() == Some((1280, 720)) { Ok(()) } else { Err(format!("{:?}", o.size())) }));
    v.push(c(Trim { start: 1.0, end: 4.0, fade_in: 0.0, fade_out: 0.0, fast: false }, "clip.mp4", "mp4", |_, o| dur_near(3.0)(o)));
    v.push(c(Trim { start: 2.0, end: 6.0, fade_in: 0.0, fade_out: 0.0, fast: true }, "clip.mkv", "mkv", |_, o| {
        if (o.duration - 4.0).abs() < 1.05 { Ok(()) } else { Err(format!("duration {:.2}", o.duration)) }
    }));
    v.push(c(Trim { start: 0.5, end: 3.5, fade_in: 0.5, fade_out: 1.0, fast: false }, "tone.wav", "wav", |_, o| dur_near(3.0)(o)));
    v.push(c(Trim { start: 1.0, end: 2.0, fade_in: 0.2, fade_out: 0.2, fast: false }, "tone.mp3", "mp3", |_, o| dur_near(1.0)(o)));
    v.push(c(Crop { x: 100, y: 50, w: 641, h: 361 }, "clip.mp4", "mp4", |_, o| if o.size() == Some((640, 360)) { Ok(()) } else { Err(format!("{:?}", o.size())) }));
    v.push(c(Speed { factor: 2.0, keep_pitch: true }, "clip.mp4", "mp4", |i, o| dur_near(i.duration / 2.0)(o)));
    v.push(c(Speed { factor: 0.5, keep_pitch: false }, "tone.wav", "wav", |i, o| dur_near(i.duration * 2.0)(o)));
    v.push(c(Speed { factor: 1.5, keep_pitch: true }, "silent.mp4", "mp4", |i, o| dur_near(i.duration / 1.5)(o)));
    v
}

pub fn selftest(keep: bool) -> i32 {
    attach_console();
    let Some(ff) = media::ffmpeg() else {
        out("FAIL ffmpeg not found");
        return 1;
    };
    out(&format!("ffmpeg: {}", ff.display()));
    let dir = std::env::temp_dir().join(format!("dragshift-selftest-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let t0 = Instant::now();
    if make_samples(&dir).is_none() {
        out("FAIL could not make sample clips");
        return 1;
    }
    out(&format!("samples ready in {:.1}s", t0.elapsed().as_secs_f32()));
    let mut fails = 0;
    for ch in checks() {
        let src = dir.join(ch.file);
        let t = Instant::now();
        let label = format!("{:?} on {}", ch.op, ch.file);
        let res = media::probe(&src).and_then(|i| media::run(&ch.op, &src, &ctl()).map(|o| (i, o)));
        let verdict = match res {
            Err(e) if ch.expect_ext == NOTE => media::note_text(&e).map(|n| format!("note: {n}")).ok_or(e),
            Err(e) => Err(e),
            Ok(_) if ch.expect_ext == NOTE => Err("wrote a file, wanted a note".into()),
            Ok((i, outs)) => {
                let o = &outs[0];
                let ext = crate::util::ext_lower(o);
                if ext != ch.expect_ext {
                    Err(format!("wrote .{ext}, wanted .{}", ch.expect_ext))
                } else {
                    let r = media::probe(o).and_then(|oi| (ch.expect)(&i, &oi));
                    let summary = media::summarize(&ch.op, &[src.clone()], &outs);
                    r.map(|_| format!("{} | {} {}", summary.title, summary.detail, summary.good))
                }
            }
        };
        match verdict {
            Ok(s) => out(&format!("ok   {label:<70} {:>5.1}s  {s}", t.elapsed().as_secs_f32())),
            Err(e) => {
                fails += 1;
                out(&format!("FAIL {label:<70} {e}"));
            }
        }
    }
    out(&format!("{} failed, {:.1}s total", fails, t0.elapsed().as_secs_f32()));
    if keep {
        out(&format!("files kept in {}", dir.display()));
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
    fails
}

pub fn run_one(key: &str, files: &[PathBuf]) -> i32 {
    attach_console();
    let Some(op) = Op::from_key(key) else {
        out(&format!("unknown key {key}"));
        return 2;
    };
    let mut fails = 0;
    for f in files {
        let c = Ctl { cancel: Arc::new(AtomicBool::new(false)), progress: Box::new(|p| eprint!("\r{:5.1}%", p * 100.0)) };
        match media::run(&op, f, &c) {
            Ok(o) => {
                let s = media::summarize(&op, &[f.clone()], &o);
                out(&format!("\r{} | {} | {}", s.title, s.detail, s.good));
            }
            Err(e) => match media::note_text(&e) {
                Some(n) => out(&format!("\rSKIP {}: {n}", f.display())),
                None => {
                    fails += 1;
                    out(&format!("\rFAIL {}: {e}", f.display()));
                }
            },
        }
    }
    fails
}
