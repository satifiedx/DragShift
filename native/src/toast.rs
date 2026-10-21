//! Matte notifications, bottom-right, stacked.
//!
//! A card appears once work has taken more than 150 ms (spinner, progress and Cancel for media
//! jobs), then turns into the result: thumbnail, what changed ("2.4 MB → 610 KB · 75% smaller")
//! and Open · Show in folder · Undo (moves the new files to the Recycle Bin). Fades, slides, the
//! spinner and the progress bar move in step with the monitor's refresh (see vsync.rs); a card
//! that isn't moving draws nothing.

use crate::gfx::{self, sd_rrect, text_in, Align, Canvas, Text, Weight};
use crate::i18n::{t, tf};
use crate::layered::{self, Surface};
use crate::theme::{self, Palette};
use crate::util;
use crate::vsync;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TOAST_THUMB: u32 = WM_APP + 30;
const TIMER_TICK: usize = 1;

const W: f32 = 380.0;
const H: f32 = 104.0;
const GAP: f32 = 10.0;
const MARGIN: f32 = 18.0;
const RADIUS: f32 = 16.0;
const SHOW_AFTER: Duration = Duration::from_millis(150);
const FADE_IN: f32 = 0.20;
const FADE_OUT: f32 = 0.16;
const MAX_CARDS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Working,
    Done,
    Error,
    Info,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Btn {
    Open,
    Show,
    Undo,
    Cancel,
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub title: String,
    pub detail: String,
    pub good: String,
    pub outs: Vec<PathBuf>,
}

pub struct Thumb {
    pub bgra: Vec<u8>,
    pub w: usize,
    pub h: usize,
}

pub struct Toast {
    pub hwnd: HWND,
    pub job: Option<u64>,
    pub cancellable: bool,
    pub state: State,
    title: String,
    detail: String,
    good: String,
    outs: Vec<PathBuf>,
    thumb: Option<Thumb>,
    progress: Option<f32>,
    shown_progress: f32,
    scale: f32,
    rect: (i32, i32, i32, i32),
    pos: (f32, f32),
    mon: HMONITOR,
    pal: Palette,
    shown: bool,
    born: Instant,
    shown_at: Instant,
    last_frame: Instant,
    life: Option<Instant>,
    leaving: Option<Instant>,
    hovered: bool,
    hover_btn: Option<Btn>,
    hover_close: bool,
    buttons: Vec<(Btn, (f32, f32, f32, f32))>,
    surface: Option<Surface>,
    cache: Option<Canvas>,
    dirty: bool,
    tracking: bool,
    window: bool,
    pub dead: bool,
}

/// What the app has to do after a click on a card.
pub enum Request {
    None,
    Cancel(u64),
}

impl Toast {
    fn new(job: Option<u64>) -> Toast {
        let hwnd = layered::create_window(Default::default());
        let now = Instant::now();
        Toast {
            hwnd, job, cancellable: false, state: State::Working, title: String::new(), detail: String::new(),
            good: String::new(), outs: vec![], thumb: None, progress: None, shown_progress: 0.0, scale: 1.0,
            rect: (0, 0, 0, 0), pos: (0.0, 0.0), mon: HMONITOR::default(), pal: theme::DARK, shown: false,
            born: now, shown_at: now, last_frame: now, life: None, leaving: None, hovered: false, hover_btn: None,
            hover_close: false, buttons: vec![], surface: None, cache: None, dirty: true, tracking: false,
            window: true, dead: false,
        }
    }

    fn appear(&mut self, rect: (i32, i32, i32, i32), scale: f32) {
        self.scale = scale;
        self.rect = rect;
        self.pos = (rect.0 as f32, rect.1 as f32);
        self.mon = vsync::monitor_of(rect.0 + rect.2 / 2, rect.1 + rect.3 / 2);
        self.pal = theme::current();
        self.surface = Surface::new(rect.2 as usize, rect.3 as usize);
        self.shown = true;
        self.shown_at = Instant::now();
        self.last_frame = self.shown_at;
        self.dirty = true;
        self.present();
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, rect.0, rect.1, rect.2, rect.3,
                                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        vsync::kick(self.mon);
    }

    /// Slide to a new slot (a card above went away).
    fn relocate(&mut self, rect: (i32, i32, i32, i32)) {
        if rect != self.rect {
            self.rect = rect;
            vsync::kick(self.mon);
        }
    }

    /// Something on the card changed: draw it again on the next frame.
    fn refresh(&mut self) {
        self.dirty = true;
        if self.shown {
            vsync::kick(self.mon);
        }
    }

    fn alpha(&self) -> u8 {
        let p = (self.shown_at.elapsed().as_secs_f32() / FADE_IN).min(1.0);
        let mut a = 1.0 - (1.0 - p).powi(3);
        if let Some(l) = self.leaving {
            let q = (l.elapsed().as_secs_f32() / FADE_OUT).min(1.0);
            a *= 1.0 - q * q;
        }
        (a * 255.0) as u8
    }

    fn set_life(&mut self, ms: u64) {
        self.life = Some(Instant::now() + Duration::from_millis(ms));
    }

    /// Fade out, then go.
    fn leave(&mut self) {
        if !self.shown {
            self.dead = true;
        } else if self.leaving.is_none() {
            self.leaving = Some(Instant::now());
            vsync::kick(self.mon);
        }
    }

    pub fn working(&mut self, title: &str, detail: &str) {
        self.state = State::Working;
        self.title = title.into();
        self.detail = detail.into();
        self.life = None;
        self.refresh();
    }

    pub fn progress(&mut self, frac: f32, detail: Option<String>) {
        self.progress = Some(frac.clamp(0.0, 1.0));
        if let Some(d) = detail {
            self.detail = d;
        }
        self.refresh();
    }

    pub fn done(&mut self, o: Outcome) {
        self.state = State::Done;
        self.title = o.title;
        self.detail = o.detail;
        self.good = o.good;
        self.outs = o.outs;
        self.progress = None;
        self.cancellable = false;
        self.leaving = None;
        self.set_life(6500);
        self.refresh();
        if let Some(first) = self.outs.first().cloned() {
            let (hwnd, size) = (self.hwnd.0 as isize, (64.0 * self.scale.max(1.0)) as i32);
            std::thread::spawn(move || {
                if let Some(t) = shell_thumbnail(&first, size) {
                    let p = Box::into_raw(Box::new(t));
                    unsafe {
                        if PostMessageW(HWND(hwnd as *mut _), WM_TOAST_THUMB, WPARAM(0), LPARAM(p as isize)).is_err() {
                            drop(Box::from_raw(p));
                        }
                    }
                }
            });
        }
    }

    pub fn error(&mut self, title: &str, detail: &str) {
        self.state = State::Error;
        self.title = title.into();
        self.detail = detail.into();
        self.progress = None;
        self.cancellable = false;
        self.leaving = None;
        self.set_life(9000);
        self.refresh();
    }

    pub fn info(&mut self, title: &str, detail: &str, ms: u64) {
        self.state = State::Info;
        self.title = title.into();
        self.detail = detail.into();
        self.good.clear();
        self.thumb = None;
        self.outs.clear();
        self.progress = None;
        self.leaving = None;
        self.set_life(ms);
        self.refresh();
    }

    pub fn dismiss(&mut self) {
        self.dead = true;
        if std::mem::take(&mut self.window) {
            unsafe {
                let _ = KillTimer(self.hwnd, TIMER_TICK);
                let _ = DestroyWindow(self.hwnd);
            }
        }
        self.surface = None;
        self.cache = None;
    }

    pub fn on_thumb(&mut self, t: Thumb) {
        if self.state == State::Done {
            self.thumb = Some(t);
            self.refresh();
        }
    }

    /// Called every 100 ms: the card's lifetime. Returns false once it is gone.
    pub fn tick(&mut self) -> bool {
        if self.dead {
            return false;
        }
        if let Some(l) = self.life {
            if self.shown && !self.hovered && Instant::now() >= l {
                self.leave();
            }
        }
        true
    }

    /// One refresh of the monitor. Returns true while the card is still moving.
    pub fn on_frame(&mut self) -> bool {
        if !self.shown || self.dead {
            return false;
        }
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        let mut moving = false;
        let mut changed = self.dirty;

        if let Some(l) = self.leaving {
            if l.elapsed().as_secs_f32() >= FADE_OUT {
                self.dead = true;
                return false;
            }
            moving = true;
            changed = true;
        }
        if self.shown_at.elapsed().as_secs_f32() < FADE_IN + 0.05 {
            moving = true;
            changed = true;
        }
        let target = (self.rect.0 as f32, self.rect.1 as f32);
        if self.pos != target {
            self.pos = (theme::approach(self.pos.0, target.0, 0.22, dt), theme::approach(self.pos.1, target.1, 0.22, dt));
            if (self.pos.0 - target.0).abs() < 0.5 && (self.pos.1 - target.1).abs() < 0.5 {
                self.pos = target;
            }
            moving = true;
            changed = true;
        }
        if self.state == State::Working {
            match self.progress {
                None => {
                    moving = true; // the spinner
                    self.dirty = true;
                }
                Some(p) => {
                    let v = theme::approach(self.shown_progress, p, 0.20, dt);
                    if v != self.shown_progress {
                        self.shown_progress = v;
                        self.dirty = true;
                        moving = true;
                    }
                }
            }
            changed |= self.dirty;
        }
        if changed {
            self.present();
        }
        moving
    }

    // ------------------------------------------------------------ input
    pub fn mouse_move(&mut self, x: f32, y: f32) {
        if !self.tracking {
            let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE,
                                            hwndTrack: self.hwnd, dwHoverTime: 0 };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            self.tracking = true;
        }
        self.hovered = true;
        let hb = self.buttons.iter().find(|(_, r)| inside(*r, x, y)).map(|(b, _)| *b);
        let hc = inside(self.close_rect(), x, y);
        if hb != self.hover_btn || hc != self.hover_close {
            self.hover_btn = hb;
            self.hover_close = hc;
            self.refresh();
        }
    }

    pub fn wants_hand(&self) -> bool {
        self.hover_btn.is_some() || self.hover_close || (self.state == State::Done && !self.outs.is_empty())
    }

    pub fn mouse_leave(&mut self) {
        self.tracking = false;
        self.hovered = false;
        self.hover_btn = None;
        self.hover_close = false;
        match self.state {
            State::Done | State::Info => self.set_life(2500),
            State::Error => self.set_life(4000),
            State::Working => {}
        }
        self.refresh();
    }

    pub fn click(&mut self, x: f32, y: f32) -> Request {
        if inside(self.close_rect(), x, y) {
            if self.state == State::Working {
                if let (true, Some(j)) = (self.cancellable, self.job) {
                    return Request::Cancel(j);
                }
            }
            self.leave();
            return Request::None;
        }
        let btn = self.buttons.iter().find(|(_, r)| inside(*r, x, y)).map(|(b, _)| *b);
        match btn {
            Some(Btn::Open) => {
                if let Some(o) = self.outs.first() {
                    util::shell_open(o);
                }
                self.leave();
            }
            Some(Btn::Show) => {
                if let Some(o) = self.outs.first() {
                    util::reveal(o);
                }
                self.leave();
            }
            Some(Btn::Undo) => {
                let n = util::recycle(&self.outs);
                let msg = if n == 1 { t("Moved 1 file to the Recycle Bin").to_string() } else {
                    tf("Moved {} files to the Recycle Bin", &[&n.to_string()])
                };
                self.info(t("Undone"), &msg, 2500);
            }
            Some(Btn::Cancel) => {
                if let Some(j) = self.job {
                    return Request::Cancel(j);
                }
            }
            None => {
                if self.state == State::Done {
                    if let Some(o) = self.outs.first() {
                        util::reveal(o);
                    }
                    self.leave();
                }
            }
        }
        Request::None
    }

    fn close_rect(&self) -> (f32, f32, f32, f32) {
        let s = self.scale;
        ((W - 32.0) * s, 8.0 * s, 24.0 * s, 24.0 * s)
    }

    // ------------------------------------------------------------ paint
    /// Redraw the card if it changed, then put it on screen at its current place and opacity.
    fn present(&mut self) {
        let Some(mut surf) = self.surface.take() else { return };
        let (w, h) = (self.rect.2 as usize, self.rect.3 as usize);
        let mut cv = match self.cache.take() {
            Some(c) if c.w == w && c.h == h => c,
            _ => {
                self.dirty = true;
                Canvas::new(w, h)
            }
        };
        if self.dirty {
            cv.clear();
            self.draw(&mut cv);
            self.dirty = false;
        }
        surf.present(self.hwnd, &cv, self.pos.0.round() as i32, self.pos.1.round() as i32, self.alpha());
        self.cache = Some(cv);
        self.surface = Some(surf);
    }

    fn draw(&mut self, cv: &mut Canvas) {
        let s = self.scale;
        let pal = self.pal;
        let (fw, fh) = (cv.w as f32, cv.h as f32);
        let err = self.state == State::Error;
        let (bg, rim) = if err {
            (theme::mix(pal.panel, pal.bad, 0.12), theme::mix(pal.line, pal.bad, 0.55))
        } else {
            (pal.panel, pal.line)
        };
        cv.fill_rrect(0.0, 0.0, fw, fh, RADIUS * s, rim);
        cv.fill_rrect(1.0 * s, 1.0 * s, fw - 2.0 * s, fh - 2.0 * s, RADIUS * s - 1.0 * s, bg);

        // leading visual
        let lead = (16.0 * s, ((H - 56.0) / 2.0 - 8.0) * s, 56.0 * s);
        let (lcx, lcy) = (lead.0 + lead.2 / 2.0, lead.1 + lead.2 / 2.0);
        match (self.state, &self.thumb) {
            (State::Working, _) => {
                cv.stroke_circle(lcx, lcy, 16.0 * s, 4.0 * s, |_, _| pal.line);
                let tau = std::f32::consts::TAU;
                let (a0, sweep) = match self.progress {
                    Some(_) => (-std::f32::consts::FRAC_PI_2, (self.shown_progress * tau).max(0.3)),
                    None => {
                        // a comet that breathes as it turns
                        let t = self.born.elapsed().as_secs_f32();
                        (t * 5.5, 1.2 + 0.8 * (t * 2.6).sin())
                    }
                };
                cv.stroke_arc(lcx, lcy, 16.0 * s, 4.0 * s, a0, sweep, |_, _| pal.accent);
                if self.progress.is_some() {
                    let pct = format!("{}", (self.shown_progress * 100.0).round() as u32);
                    text_in(cv, &Text::new(&pct, 11.0 * s, Weight::Bold), (lcx - 16.0 * s, lcy - 8.0 * s, 32.0 * s, 16.0 * s),
                            Align::Center, pal.text, None);
                }
            }
            (_, Some(th)) => {
                cv.draw_thumb(&th.bgra, th.w, th.h, lead.0, lead.1, lead.2, 12.0 * s);
                cv.stroke_rrect(lead.0, lead.1, lead.2, lead.2, 12.0 * s, 1.0 * s, |_, _| pal.line);
            }
            _ => {
                let (icon, col) = match self.state {
                    State::Done => ("✓", pal.good),
                    State::Error => ("!", pal.bad),
                    _ => ("i", pal.accent),
                };
                cv.fill_circle(lcx, lcy, 22.0 * s, theme::mix(bg, col, 0.18));
                text_in(cv, &Text::new(icon, 22.0 * s, Weight::Bold), (lcx - 22.0 * s, lcy - 22.0 * s, 44.0 * s, 44.0 * s),
                        Align::Center, col, None);
            }
        }

        let x = 86.0 * s;
        let tw = fw - x - 36.0 * s;
        let title = gfx::elide(&self.title, 15.0 * s, Weight::Bold, tw);
        text_in(cv, &Text::new(&title, 15.0 * s, Weight::Bold), (x, 13.0 * s, tw, 20.0 * s), Align::Left, pal.text, None);
        if !self.good.is_empty() {
            let gx = x + gfx::measure(&Text::new(&title, 15.0 * s, Weight::Bold)) + 8.0 * s;
            let gw = gfx::measure(&Text::new(&self.good, 11.0 * s, Weight::Bold)) + 14.0 * s;
            if gx + gw < fw - 36.0 * s {
                cv.fill_rrect(gx, 14.0 * s, gw, 18.0 * s, 9.0 * s, theme::mix(bg, pal.good, 0.2));
                text_in(cv, &Text::new(&self.good, 11.0 * s, Weight::Bold), (gx, 14.0 * s, gw, 18.0 * s), Align::Center,
                        pal.good, None);
            }
        }
        let working_bar = self.state == State::Working && self.progress.is_some();
        let lines = gfx::wrap(&self.detail, 12.0 * s, Weight::Regular, tw + 18.0 * s, if working_bar { 1 } else { 2 });
        for (i, l) in lines.iter().enumerate() {
            text_in(cv, &Text::new(l, 12.0 * s, Weight::Regular), (x, (35.0 + 16.0 * i as f32) * s, tw + 18.0 * s, 16.0 * s),
                    Align::Left, pal.dim, None);
        }

        // close × (or stop, while a cancellable job runs)
        let cr = self.close_rect();
        if self.hover_close {
            cv.fill_rrect(cr.0, cr.1, cr.2, cr.3, 6.0 * s, pal.raised);
        }
        text_in(cv, &Text::new("✕", 13.0 * s, Weight::Regular), cr, Align::Center,
                if self.hover_close { pal.text } else { pal.dim }, None);

        // progress bar
        if working_bar {
            let p = self.shown_progress;
            let (bx, by, bw, bh) = (x, 58.0 * s, tw, 4.0 * s);
            cv.fill_rrect(bx, by, bw, bh, bh / 2.0, pal.line);
            if p > 0.0 {
                let fwid = (bw * p).max(bh);
                cv.fill_sdf((bx, by, bx + fwid, by + bh), |px, py| sd_rrect(px, py, bx, by, fwid, bh, bh / 2.0),
                            |_, _| pal.accent);
            }
        }

        // buttons
        self.buttons.clear();
        let names: Vec<(Btn, &str)> = match self.state {
            State::Done if !self.outs.is_empty() => vec![(Btn::Open, t("Open")), (Btn::Show, t("Show in folder")), (Btn::Undo, t("Undo"))],
            State::Working if self.cancellable => vec![(Btn::Cancel, t("Cancel"))],
            _ => vec![],
        };
        let mut bx = x;
        for (b, label) in names {
            let bw = gfx::measure(&Text::new(label, 12.0 * s, Weight::Bold)) + 22.0 * s;
            let r = (bx, fh - 32.0 * s, bw, 22.0 * s);
            let hov = self.hover_btn == Some(b);
            cv.fill_rrect(r.0, r.1, r.2, r.3, 7.0 * s, if hov { pal.raised } else { pal.sector });
            text_in(cv, &Text::new(label, 12.0 * s, Weight::Bold), r, Align::Center, pal.text, None);
            self.buttons.push((b, r));
            bx += bw + 6.0 * s;
        }
    }
}

fn inside(r: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= r.0 && y >= r.1 && x < r.0 + r.2 && y < r.1 + r.3
}

// ------------------------------------------------------------------ the stack

pub struct Toasts {
    pub cards: Vec<Toast>,
}

impl Toasts {
    pub fn new() -> Toasts {
        Toasts { cards: Vec::new() }
    }

    fn slot(i: usize) -> ((i32, i32, i32, i32), f32) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let (s, _, work) = layered::monitor_at(pt.x, pt.y);
        let (w, h) = ((W * s).round() as i32, (H * s).round() as i32);
        let x = work.right - w - (MARGIN * s) as i32;
        let y = work.bottom - (MARGIN * s) as i32 - h - i as i32 * (h + (GAP * s) as i32);
        ((x, y, w, h), s)
    }

    /// A new card. It shows itself after 150 ms of work, or at once for a result.
    pub fn new_card(&mut self, job: Option<u64>) -> usize {
        while self.cards.iter().filter(|c| !c.dead).count() >= MAX_CARDS {
            if let Some(c) = self.cards.iter_mut().find(|c| !c.dead) {
                c.dismiss();
            }
            self.reap();
        }
        let c = Toast::new(job);
        unsafe {
            SetTimer(c.hwnd, TIMER_TICK, 100, None);
        }
        self.cards.push(c);
        self.cards.len() - 1
    }

    pub fn info(&mut self, title: &str, detail: &str, ms: u64) {
        let i = self.new_card(None);
        self.cards[i].info(title, detail, ms);
        self.show_now(i);
    }

    pub fn error(&mut self, title: &str, detail: &str) {
        let i = self.new_card(None);
        self.cards[i].error(title, detail);
        self.show_now(i);
    }

    pub fn done(&mut self, o: Outcome) {
        let i = self.new_card(None);
        self.show_now(i);
        self.cards[i].done(o);
    }

    pub fn show_now(&mut self, i: usize) {
        if !self.cards[i].shown {
            let idx = self.cards[..i].iter().filter(|c| c.shown && !c.dead).count();
            let (rect, s) = Self::slot(idx);
            self.cards[i].appear(rect, s);
        } else {
            self.cards[i].refresh();
        }
    }

    /// A card for a running job: it shows itself after 150 ms unless the job is done by then.
    pub fn start(&mut self, job: u64, title: &str, detail: &str, cancellable: bool) {
        let i = self.new_card(Some(job));
        self.cards[i].working(title, detail);
        self.cards[i].cancellable = cancellable;
    }

    pub fn job_progress(&mut self, job: u64, frac: f32, detail: Option<String>) {
        if let Some(i) = self.by_job(job) {
            self.cards[i].progress(frac, detail);
        }
    }

    pub fn job_done(&mut self, job: u64, o: Outcome) {
        match self.by_job(job) {
            Some(i) => {
                self.show_now(i);
                self.cards[i].done(o);
            }
            None => self.done(o),
        }
    }

    pub fn job_error(&mut self, job: u64, title: &str, detail: &str) {
        match self.by_job(job) {
            Some(i) => {
                self.show_now(i);
                self.cards[i].error(title, detail);
            }
            None => self.error(title, detail),
        }
    }

    pub fn job_info(&mut self, job: u64, title: &str, detail: &str, ms: u64) {
        match self.by_job(job) {
            Some(i) => {
                self.show_now(i);
                self.cards[i].info(title, detail, ms);
            }
            None => self.info(title, detail, ms),
        }
    }

    /// Drop a job's card without a result (an editor window opened instead).
    pub fn job_forget(&mut self, job: u64) {
        if let Some(i) = self.by_job(job) {
            self.cards[i].dismiss();
            self.reap();
        }
    }

    pub fn by_job(&mut self, job: u64) -> Option<usize> {
        self.cards.iter().position(|c| c.job == Some(job) && !c.dead)
    }

    pub fn by_hwnd(&mut self, hwnd: HWND) -> Option<&mut Toast> {
        self.cards.iter_mut().find(|c| c.hwnd == hwnd && !c.dead)
    }

    pub fn tick(&mut self, hwnd: HWND) {
        let Some(i) = self.cards.iter().position(|c| c.hwnd == hwnd) else { return };
        let c = &self.cards[i];
        if !c.shown && !c.dead && c.born.elapsed() >= SHOW_AFTER {
            self.show_now(i);
            return;
        }
        if !self.cards[i].tick() {
            self.reap();
        }
    }

    /// One refresh of the monitor for every card. Returns true while any of them moves.
    pub fn on_frame(&mut self) -> bool {
        let mut moving = false;
        for c in self.cards.iter_mut() {
            moving |= c.on_frame();
        }
        if self.cards.iter().any(|c| c.dead) {
            self.reap();
            moving = true;
        }
        moving
    }

    /// The monitor the cards are on, for the frame clock.
    pub fn monitor(&self) -> HMONITOR {
        self.cards.iter().find(|c| c.shown && !c.dead).map(|c| c.mon).unwrap_or_default()
    }

    /// Drop dead cards and slide the rest into place.
    pub fn reap(&mut self) {
        let had = self.cards.len();
        for c in self.cards.iter_mut().filter(|c| c.dead) {
            c.dismiss();
        }
        self.cards.retain(|c| !c.dead);
        if self.cards.len() != had {
            let mut idx = 0;
            for c in self.cards.iter_mut() {
                if c.shown {
                    let (rect, _) = Self::slot(idx);
                    c.relocate(rect);
                    idx += 1;
                }
            }
        }
        if self.cards.is_empty() {
            crate::app::schedule_trim();
        }
    }
}

// ------------------------------------------------------------------ thumbnails

/// The Explorer thumbnail of a file (photos, videos, album art, PDFs), as straight BGRA.
fn shell_thumbnail(path: &std::path::Path, size: i32) -> Option<Thumb> {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::*;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED | COINIT_DISABLE_OLE1DDE);
        let w = util::wide(path);
        let item: IShellItemImageFactory = SHCreateItemFromParsingName(windows::core::PCWSTR(w.as_ptr()), None).ok()?;
        let hb = item.GetImage(SIZE { cx: size, cy: size }, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let mut bm = BITMAP::default();
        GetObjectW(hb, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
        let (bw, bh) = (bm.bmWidth as usize, bm.bmHeight.unsigned_abs() as usize);
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bw as i32,
                biHeight: -(bh as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; bw * bh * 4];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(dc, hb, 0, bh as u32, Some(buf.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(hb);
        if lines == 0 || bw == 0 || bh == 0 {
            return None;
        }
        if buf.chunks_exact(4).all(|p| p[3] == 0) {
            buf.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        } else {
            // shell bitmaps with alpha are premultiplied: undo for our straight-alpha blit
            for p in buf.chunks_exact_mut(4) {
                let a = p[3] as f32 / 255.0;
                if a > 0.0 {
                    for c in p.iter_mut().take(3) {
                        *c = ((*c as f32 / a).min(255.0)) as u8;
                    }
                }
            }
        }
        Some(Thumb { bgra: buf, w: bw, h: bh })
    }
}
