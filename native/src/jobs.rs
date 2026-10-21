//! Media jobs on their own threads. They report back to the UI thread with posted messages,
//! so the menu and the notifications never wait for ffmpeg.

use crate::i18n::{t, tf};
use crate::media::{self, Ctl, Op};
use crate::toast::Outcome;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_JOB_PROGRESS: u32 = WM_APP + 40; // wparam = job, lparam = file index << 16 | frac × 10000
pub const WM_JOB_DONE: u32 = WM_APP + 41;     // lparam = Box<JobDone>

pub struct JobDone {
    pub id: u64,
    pub result: Result<Outcome, String>,
}

pub fn unpack_progress(lp: isize) -> (usize, f32) {
    ((lp >> 16) as usize, (lp & 0xFFFF) as f32 / 10000.0)
}

fn post(hwnd: isize, msg: u32, wp: usize, lp: isize) -> bool {
    unsafe { PostMessageW(HWND(hwnd as *mut _), msg, WPARAM(wp), LPARAM(lp)).is_ok() }
}

pub fn spawn_media(id: u64, op: Op, paths: Vec<PathBuf>, cancel: Arc<AtomicBool>, hwnd: HWND) {
    let hwnd = hwnd.0 as isize;
    std::thread::Builder::new().name(format!("job-{id}")).spawn(move || {
        let n = paths.len().max(1);
        let mut outs = Vec::new();
        let mut done_srcs = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        for (i, p) in paths.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let ctl = Ctl {
                cancel: cancel.clone(),
                progress: Box::new(move |f: f32| {
                    let total = ((i as f32 + f.clamp(0.0, 1.0)) / n as f32 * 10000.0) as isize;
                    post(hwnd, WM_JOB_PROGRESS, id as usize, ((i as isize) << 16) | total);
                }),
            };
            match media::run(&op, p, &ctl) {
                Ok(o) => {
                    outs.extend(o);
                    done_srcs.push(p.clone());
                }
                Err(e) if media::is_cancel(&e) => break,
                Err(e) if media::note_text(&e).is_some() => notes.push(e),
                Err(e) => {
                    crate::logf!("job {id}: {} failed: {e}", p.display());
                    errors.push(e);
                }
            }
        }
        let result = if cancel.load(Ordering::Relaxed) {
            // a cancelled batch keeps the files that were already finished
            if outs.is_empty() { Err(media::cancelled()) } else { Ok(media::summarize(&op, &done_srcs, &outs)) }
        } else if outs.is_empty() {
            Err(errors.into_iter().chain(notes).next().unwrap_or_else(|| t("Nothing was written").into()))
        } else {
            let mut o = media::summarize(&op, &done_srcs, &outs);
            if !errors.is_empty() {
                o.good = tf("{} didn't work", &[&errors.len().to_string()]);
            }
            Ok(o)
        };
        let b = Box::into_raw(Box::new(JobDone { id, result }));
        if !post(hwnd, WM_JOB_DONE, 0, b as isize) {
            unsafe { drop(Box::from_raw(b)) };
        }
    }).ok();
}
