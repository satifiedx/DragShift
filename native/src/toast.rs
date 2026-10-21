//! Liquid-glass notifications, bottom-right, stacked.
//!
//! A card appears once work has taken more than 150 ms (spinner, progress and Cancel for media
//! jobs), then turns into the result: thumbnail, what changed ("2.4 MB → 610 KB · 75% smaller")
//! and Open · Show in folder · Undo (moves the new files to the Recycle Bin).

use crate::gfx::{self, iridescent, sd_rrect, text_in, Align, Canvas, Color, Text, Weight};
use crate::i18n::{t, tf};
use crate::layered::{self, Surface};
use crate::liquid::{self, Field, Style};
use crate::util;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TOAST_THUMB: u32 = WM_APP + 30;
const TIMER_TICK: usize = 1;

const W: f32 = 380.0;
const H: f32 = 104.0;
const GAP: f32 = 10.0;
const MARGIN: f32 = 18.0;
const RADIUS: f32 = 20.0;
const SHOW_AFTER: Duration = Duration::from_millis(150);
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
    scale: f32,
    rect: (i32, i32, i32, i32),
    shown: bool,
    born: Instant,
    shown_at: Instant,
    life: Option<Instant>,
    hovered: bool,
    hover_btn: Option<Btn>,
    hover_close: bool,
    buttons: Vec<(Btn, (f32, f32, f32, f32))>,
    glass: Option<Canvas>,
    excluded: bool,
    surface: Option<Surface>,
    tracking: bool,
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
        Toast {
            hwnd, job, cancellable: false, state: State::Working, title: String::new(), detail: String::new(),
            good: String::new(), outs: vec![], thumb: None, progress: None, scale: 1.0, rect: (0, 0, 0, 0),
            shown: false, born: Instant::now(), shown_at: Instant::now(), life: None, hovered: false,
            hover_btn: None, hover_close: false, buttons: vec![], glass: None, excluded: false, surface: None,
            tracking: false, dead: false,
        }
    }

    fn appear(&mut self, rect: (i32, i32, i32, i32), scale: f32) {
        self.scale = scale;
        self.rect = rect;
        self.capture();
        self.surface = Surface::new(rect.2 as usize, rect.3 as usize);
        self.shown = true;
        self.shown_at = Instant::now();
        self.paint(0);
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, rect.0, rect.1, rect.2, rect.3,
                                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        self.excluded = layered::exclude_from_capture(self.hwnd);
    }

    /// Snapshot what's behind the card and refract it (a couple of ms).
    fn capture(&mut self) {
        let (x, y, w, h) = self.rect;
        let s = self.scale;
        self.glass = crate::capture::grab(x, y, w, h).map(|bg| {
            let (fw, fh) = (w as f32, h as f32);
            let field = Field::from_fn(w as usize, h as usize, |px, py| sd_rrect(px, py, 0.5, 0.5, fw - 1.0, fh - 1.0, RADIUS * s));
            let style = Style { blur: 8.0, strength: 12.0, bezel: 16.0, ..Style::default() }.scaled(s);
            liquid::render(&bg, &field, &style, None).canvas
        });
    }

    fn relocate(&mut self, rect: (i32, i32, i32, i32)) {
        if rect == self.rect {
            return;
        }
        self.rect = rect;
        if self.excluded {
            self.capture();
        } else {
            self.glass = None;
        }
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, rect.0, rect.1, rect.2, rect.3, SWP_NOACTIVATE);
        }
        self.paint(self.alpha());
    }

    fn alpha(&self) -> u8 {
        let p = (self.shown_at.elapsed().as_secs_f32() / 0.2).min(1.0);
        ((1.0 - (1.0 - p).powi(3)) * 255.0) as u8
    }

    fn set_life(&mut self, ms: u64) {
        self.life = Some(Instant::now() + Duration::from_millis(ms));
    }

    pub fn working(&mut self, title: &str, detail: &str) {
        self.state = State::Working;
        self.title = title.into();
        self.detail = detail.into();
        self.life = None;
    }

    pub fn progress(&mut self, frac: f32, detail: Option<String>) {
        self.progress = Some(frac.clamp(0.0, 1.0));
        if let Some(d) = detail {
            self.detail = d;
        }
        if self.shown {
            self.paint(self.alpha());
        }
    }

    pub fn done(&mut self, o: Outcome) {
        self.state = State::Done;
        self.title = o.title;
        self.detail = o.detail;
        self.good = o.good;
        self.outs = o.outs;
        self.progress = None;
        self.cancellable = false;
        self.set_life(6500);
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
        self.set_life(9000);
    }

    pub fn info(&mut self, title: &str, detail: &str, ms: u64) {
        self.state = State::Info;
        self.title = title.into();
        self.detail = detail.into();
        self.good.clear();
        self.thumb = None;
        self.outs.clear();
        self.progress = None;
        self.set_life(ms);
    }

    pub fn dismiss(&mut self) {
        self.dead = true;
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_TICK);
            let _ = DestroyWindow(self.hwnd);
        }
        self.surface = None;
        self.glass = None;
    }

    pub fn on_thumb(&mut self, t: Thumb) {
        if self.state == State::Done {
            self.thumb = Some(t);
            self.paint(self.alpha());
        }
    }

    /// Called every 33 ms. Returns false once the card wants to go.
    pub fn tick(&mut self) -> bool {
        if self.dead {
            return false;
        }
        if !self.shown {
            return true;
        }
        if let Some(l) = self.life {
            if !self.hovered && Instant::now() >= l {
                return false;
            }
        }
        if self.state == State::Working || self.shown_at.elapsed().as_millis() < 240 {
            self.paint(self.alpha());
        }
        true
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
            self.paint(self.alpha());
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
        self.paint(self.alpha());
    }

    pub fn click(&mut self, x: f32, y: f32) -> Request {
        if inside(self.close_rect(), x, y) {
            if self.state == State::Working {
                if let (true, Some(j)) = (self.cancellable, self.job) {
                    return Request::Cancel(j);
                }
            }
            self.dead = true;
            return Request::None;
        }
        let btn = self.buttons.iter().find(|(_, r)| inside(*r, x, y)).map(|(b, _)| *b);
        match btn {
            Some(Btn::Open) => {
                if let Some(o) = self.outs.first() {
                    util::shell_open(o);
                }
                self.dead = true;
            }
            Some(Btn::Show) => {
                if let Some(o) = self.outs.first() {
                    util::reveal(o);
                }
                self.dead = true;
            }
            Some(Btn::Undo) => {
                let n = util::recycle(&self.outs);
                let msg = if n == 1 { t("Moved 1 file to the Recycle Bin").to_string() } else {
                    tf("Moved {} files to the Recycle Bin", &[&n.to_string()])
                };
                self.info(t("Undone"), &msg, 2500);
                self.paint(self.alpha());
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
                    self.dead = true;
                }
            }
        }
        Request::None
    }

    fn close_rect(&self) -> (f32, f32, f32, f32) {
        let s = self.scale;
        ((W - 30.0) * s, 6.0 * s, 24.0 * s, 24.0 * s)
    }

    // ------------------------------------------------------------ paint
    fn paint(&mut self, alpha: u8) {
        let Some(mut surf) = self.surface.take() else { return };
        let s = self.scale;
        let (w, h) = (self.rect.2 as usize, self.rect.3 as usize);
        let mut cv = Canvas::new(w, h);
        let err = self.state == State::Error;
        let (fw, fh) = (w as f32, h as f32);
        let tint = match (&self.glass, err) {
            (Some(g), _) => {
                cv.draw(g, 0, 0, 1.0);
                if err { Color::rgba(140, 30, 50, 150) } else { Color::rgba(20, 16, 32, 120) }
            }
            (None, true) => Color::rgba(120, 30, 45, 230),
            (None, false) => Color::rgba(34, 27, 52, 232),
        };
        glass_overlay(&mut cv, 0.5, 0.5, fw - 1.0, fh - 1.0, RADIUS * s, tint, s, 170.0);

        // leading visual
        let lead = (14.0 * s, ((H - 56.0) / 2.0 - 8.0) * s, 56.0 * s);
        let (lcx, lcy) = (lead.0 + lead.2 / 2.0, lead.1 + lead.2 / 2.0);
        match (self.state, &self.thumb) {
            (State::Working, _) => {
                cv.stroke_circle(lcx, lcy, 16.0 * s, 4.0 * s, |_, _| Color::rgba(255, 255, 255, 50));
                let spin = self.born.elapsed().as_secs_f32() * 6.5;
                let sweep = if let Some(p) = self.progress { (p * std::f32::consts::TAU).max(0.35) } else { 1.75 };
                let a0 = if self.progress.is_some() { -std::f32::consts::FRAC_PI_2 } else { spin };
                cv.stroke_arc(lcx, lcy, 16.0 * s, 4.0 * s, a0, sweep, |x, y| {
                    iridescent((y - lcy).atan2(x - lcx) / std::f32::consts::TAU - spin * 0.05)
                });
                if let Some(p) = self.progress {
                    let pct = format!("{}", (p * 100.0).round() as u32);
                    text_in(&mut cv, &Text::new(&pct, 11.0 * s, Weight::Bold), (lcx - 16.0 * s, lcy - 8.0 * s, 32.0 * s, 16.0 * s),
                            Align::Center, gfx::WHITE, None);
                }
            }
            (_, Some(th)) => {
                cv.draw_thumb(&th.bgra, th.w, th.h, lead.0, lead.1, lead.2, 12.0 * s);
                cv.stroke_rrect(lead.0, lead.1, lead.2, lead.2, 12.0 * s, 1.0 * s, |_, _| Color::rgba(255, 255, 255, 110));
            }
            _ => {
                cv.fill_circle(lcx, lcy, 22.0 * s, Color::rgba(255, 255, 255, 34));
                let (icon, col) = match self.state {
                    State::Done => ("✓", Color::rgb(0x8d, 0xff, 0xb8)),
                    State::Error => ("!", Color::rgb(0xff, 0xd3, 0x6b)),
                    _ => ("↺", gfx::WHITE),
                };
                text_in(&mut cv, &Text::new(icon, 22.0 * s, Weight::Bold), (lcx - 22.0 * s, lcy - 22.0 * s, 44.0 * s, 44.0 * s),
                        Align::Center, col, None);
            }
        }

        let x = 82.0 * s;
        let tw = fw - x - 34.0 * s;
        let title = gfx::elide(&self.title, 15.0 * s, Weight::Bold, tw);
        text_in(&mut cv, &Text::new(&title, 15.0 * s, Weight::Bold), (x, 13.0 * s, tw, 20.0 * s), Align::Left, gfx::WHITE, None);
        if !self.good.is_empty() {
            let gx = x + gfx::measure(&Text::new(&title, 15.0 * s, Weight::Bold)) + 8.0 * s;
            let gw = gfx::measure(&Text::new(&self.good, 11.0 * s, Weight::Bold)) + 14.0 * s;
            if gx + gw < fw - 34.0 * s {
                cv.fill_rrect(gx, 14.0 * s, gw, 18.0 * s, 9.0 * s, Color::rgba(80, 220, 140, 70));
                text_in(&mut cv, &Text::new(&self.good, 11.0 * s, Weight::Bold), (gx, 14.0 * s, gw, 18.0 * s), Align::Center,
                        Color::rgb(0xa8, 0xff, 0xc9), None);
            }
        }
        let working_bar = self.state == State::Working && self.progress.is_some();
        let lines = gfx::wrap(&self.detail, 12.0 * s, Weight::Regular, tw + 18.0 * s, if working_bar { 1 } else { 2 });
        for (i, l) in lines.iter().enumerate() {
            text_in(&mut cv, &Text::new(l, 12.0 * s, Weight::Regular), (x, (35.0 + 16.0 * i as f32) * s, tw + 18.0 * s, 16.0 * s),
                    Align::Left, Color::rgba(255, 255, 255, 200), None);
        }

        // close × (or stop, while a cancellable job runs)
        let cr = self.close_rect();
        if self.hover_close {
            cv.fill_rrect(cr.0, cr.1, cr.2, cr.3, 6.0 * s, Color::rgba(255, 80, 100, 150));
        }
        text_in(&mut cv, &Text::new("✕", 13.0 * s, Weight::Regular), cr, Align::Center, Color::rgba(255, 255, 255, 170), None);

        // progress bar
        if let (State::Working, Some(p)) = (self.state, self.progress) {
            let (bx, by, bw, bh) = (x, 58.0 * s, tw, 4.0 * s);
            cv.fill_rrect(bx, by, bw, bh, bh / 2.0, Color::rgba(255, 255, 255, 40));
            if p > 0.0 {
                let fwid = (bw * p).max(bh);
                cv.fill_sdf((bx, by, bx + fwid, by + bh), |px, py| sd_rrect(px, py, bx, by, fwid, bh, bh / 2.0),
                            |px, _| iridescent(0.55 + (px - bx) / bw * 0.35));
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
            cv.fill_rrect(r.0, r.1, r.2, r.3, 11.0 * s, Color::rgba(255, 255, 255, if hov { 60 } else { 26 }));
            cv.stroke_rrect(r.0, r.1, r.2, r.3, 11.0 * s, 1.0 * s, |_, _| Color::rgba(255, 255, 255, if hov { 120 } else { 60 }));
            text_in(&mut cv, &Text::new(label, 12.0 * s, Weight::Bold), r, Align::Center, gfx::WHITE, None);
            self.buttons.push((b, r));
            bx += bw + 6.0 * s;
        }
        surf.present(self.hwnd, &cv, self.rect.0, self.rect.1, alpha);
        self.surface = Some(surf);
    }
}

fn inside(r: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= r.0 && y >= r.1 && x < r.0 + r.2 && y < r.1 + r.3
}

/// Tint, rainbow wash, sheen, inner bezel glow and the specular + iridescent rim.
#[allow(clippy::too_many_arguments)]
pub fn glass_overlay(cv: &mut Canvas, x: f32, y: f32, w: f32, h: f32, r: f32, tint: Color, s: f32, rainbow: f32) {
    let diag = w + h;
    cv.fill_sdf((x, y, x + w, y + h), |px, py| sd_rrect(px, py, x, y, w, h, r), |px, py| {
        let depth = -sd_rrect(px, py, x, y, w, h, r);
        let u = ((px - x) + (py - y)) / diag;
        let wash = iridescent(u);
        let sheen = (1.0 - (py - y) / (h * 0.55)).clamp(0.0, 1.0) * 70.0 / 255.0;
        let bezel = ((1.0 - depth / (7.0 * s)).clamp(0.0, 1.0) * 26.0 + (1.0 - depth / (14.0 * s)).clamp(0.0, 1.0) * 12.0) / 255.0;
        // blend: tint, then wash (alpha 26/255), then white sheen + bezel glow
        let mut c = tint;
        let wa = 26.0 / 255.0;
        let out_a = c.a + wa * (1.0 - c.a);
        let mix = |a: f32, b: f32| (a * c.a + b * wa * (1.0 - c.a)) / out_a.max(1e-3);
        c = Color { r: mix(c.r, wash.r), g: mix(c.g, wash.g), b: mix(c.b, wash.b), a: out_a };
        let white = (sheen + bezel).min(0.6);
        let out_a2 = c.a + white * (1.0 - c.a);
        let k = white / out_a2.max(1e-3);
        Color { r: c.r * (1.0 - k) + k, g: c.g * (1.0 - k) + k, b: c.b * (1.0 - k) + k, a: out_a2 }
    });
    // specular rim, bright towards the top-left light
    cv.stroke_rrect(x, y, w, h, r, 1.3 * s, |px, py| {
        let u = ((px - x) + (py - y)) / diag;
        let a = if u < 0.35 { 235.0 + (60.0 - 235.0) * u / 0.35 } else if u < 0.65 { 60.0 - 25.0 * (u - 0.35) / 0.3 } else { 35.0 + 115.0 * (u - 0.65) / 0.35 };
        gfx::WHITE.with_a(a / 255.0)
    });
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    cv.stroke_rrect(x, y, w, h, r, 1.4 * s, |px, py| {
        iridescent((py - cy).atan2(px - cx) / std::f32::consts::TAU + 30.0 / 360.0).with_a(rainbow / 255.0)
    });
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
            SetTimer(c.hwnd, TIMER_TICK, 33, None);
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
        let a = self.cards[i].alpha();
        self.cards[i].paint(a);
    }

    pub fn show_now(&mut self, i: usize) {
        if !self.cards[i].shown {
            let idx = self.cards[..i].iter().filter(|c| c.shown && !c.dead).count();
            let (rect, s) = Self::slot(idx);
            self.cards[i].appear(rect, s);
        } else {
            let a = self.cards[i].alpha();
            self.cards[i].paint(a);
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
                let a = self.cards[i].alpha();
                self.cards[i].paint(a);
            }
            None => self.done(o),
        }
    }

    pub fn job_error(&mut self, job: u64, title: &str, detail: &str) {
        match self.by_job(job) {
            Some(i) => {
                self.show_now(i);
                self.cards[i].error(title, detail);
                let a = self.cards[i].alpha();
                self.cards[i].paint(a);
            }
            None => self.error(title, detail),
        }
    }

    pub fn job_info(&mut self, job: u64, title: &str, detail: &str, ms: u64) {
        match self.by_job(job) {
            Some(i) => {
                self.show_now(i);
                self.cards[i].info(title, detail, ms);
                let a = self.cards[i].alpha();
                self.cards[i].paint(a);
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
            self.cards[i].dismiss();
            self.reap();
        }
    }

    /// Drop dead cards and slide the rest into place.
    pub fn reap(&mut self) {
        let had = self.cards.len();
        for c in self.cards.iter_mut().filter(|c| c.dead && c.surface.is_some()) {
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
