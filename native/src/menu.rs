//! The radial drop menu that pops up under the cursor while you Shift+drag a file.
//!
//! The moment it opens we snapshot the screen behind it and refract that snapshot through the
//! menu's shape on a worker thread (a few ms in Rust). Everything it allocates (snapshot, glass,
//! frame buffer, DIB) is dropped again when it closes, so it costs nothing while idle.

use crate::actions::{self, Item, Ring};
use crate::app::with_app;
use crate::gfx::{self, iridescent, sd_sector, text_in, Align, Canvas, Color, Text, Weight};
use crate::hooks;
use crate::i18n::{t, tf};
use crate::layered::{self, Surface};
use crate::liquid::{self, Backdrop, Field, Style};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use windows::core::implement;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_MENU_RENDERED: u32 = WM_APP + 20;
pub const WM_MENU_ACTIVATE: u32 = WM_APP + 21;
pub const TIMER_FRAME: usize = 1;
pub const TIMER_CLOSE: usize = 2;

const SIZE: f32 = 400.0;
const R_OUT: f32 = 158.0;
const R_IN: f32 = 60.0;
const R_HUB: f32 = 50.0;
const GAP_DEG: f32 = 3.2;
const CORNER: f32 = 10.0;
const SPECULATIVE: usize = 8; // petals shown while we wait to learn what is being dragged
const NO_DRAG_TICKS: u32 = 50;

pub struct Activation {
    pub key: String,
    pub paths: Vec<PathBuf>,
}

pub struct Rendered {
    serial: u32,
    n: usize,
    canvas: Canvas,
    light: bool,
}

pub fn angles(n: usize, i: usize) -> (f32, f32) {
    let span = 360.0 / n as f32;
    let start = 90.0 + span / 2.0 - (i as f32 + 1.0) * span;
    (start + GAP_DEG / 2.0, start + span - GAP_DEG / 2.0)
}

fn ease_out_back(x: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

fn ease_out_cubic(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

/// Signed distance field of the whole menu (hub + n petals) at scale `s`.
fn menu_field(n: usize, s: f32) -> Field {
    let size = (SIZE * s).round() as usize;
    let c = size as f32 / 2.0;
    let secs: Vec<(f32, f32)> = (0..n).map(|i| {
        let (a0, a1) = angles(n, i);
        (((a0 + a1) / 2.0).to_radians(), ((a1 - a0) / 2.0).to_radians())
    }).collect();
    Field::from_fn(size, size, |x, y| {
        let (dx, dy) = (x - c, c - y);
        let rad = dx.hypot(dy);
        let mut d = rad - R_HUB * s;
        if rad > (R_IN - CORNER - 4.0) * s {
            let ang = dy.atan2(dx);
            for &(mid, half) in &secs {
                d = d.min(sd_sector(rad, ang, R_IN * s, R_OUT * s, mid, half, CORNER * s));
            }
        }
        d
    })
}

pub struct Menu {
    pub hwnd: HWND,
    pub visible: bool,
    pub click_mode: bool,
    pub got_drag: bool,
    pub paths: Vec<PathBuf>,
    rings: Vec<Ring>,
    ring_idx: usize,
    cat: Option<actions::Cat>,
    hover: i32,
    hub_text: String,
    scale: f32,
    size: usize,
    pos: (i32, i32),
    shown_at: (i32, i32),
    idle_ticks: u32,
    hub_armed: bool,
    hub_since: Option<Instant>,
    backdrop: Option<Arc<Backdrop>>,
    glass: HashMap<usize, Arc<Canvas>>,
    rendering: Vec<usize>,
    light: bool,
    serial: u32,
    opened: Instant,
    swap_started: Option<Instant>,
    spring: Vec<f32>,
    hub_spring: f32,
    surface: Option<Surface>,
    frame: Option<Canvas>,
    tracking: bool,
    dirty: bool,
}

impl Menu {
    pub fn new() -> Menu {
        let hwnd = layered::create_window(Default::default());
        unsafe {
            let target: IDropTarget = DropTarget { accepted: std::cell::Cell::new(false) }.into();
            if let Err(e) = RegisterDragDrop(hwnd, &target) {
                crate::logf!("menu: RegisterDragDrop failed: {e}");
            }
        }
        Menu {
            hwnd, visible: false, click_mode: false, got_drag: false, paths: vec![], rings: vec![], ring_idx: 0,
            cat: None, hover: -1, hub_text: String::new(), scale: 1.0, size: SIZE as usize, pos: (0, 0),
            shown_at: (0, 0), idle_ticks: 0, hub_armed: false, hub_since: None, backdrop: None,
            glass: HashMap::new(), rendering: vec![], light: false, serial: 0, opened: Instant::now(),
            swap_started: None, spring: vec![], hub_spring: 0.0, surface: None, frame: None, tracking: false,
            dirty: true,
        }
    }

    fn items(&self) -> &[Item] {
        self.rings.get(self.ring_idx).map(|r| r.items.as_slice()).unwrap_or(&[])
    }

    // ------------------------------------------------------------ show / hide
    pub fn popup(&mut self, x: i32, y: i32, paths: Option<Vec<PathBuf>>) {
        let (s, mon, _) = layered::monitor_at(x, y);
        self.scale = s;
        self.size = (SIZE * s).round() as usize;
        let sz = self.size as i32;
        let tlx = (x - sz / 2).clamp(mon.left, mon.right - sz);
        let tly = (y - sz / 2).clamp(mon.top, mon.bottom - sz);
        self.pos = (tlx, tly);
        self.shown_at = (x, y);
        self.serial += 1;
        self.glass.clear();
        self.rendering.clear();
        let t0 = Instant::now();
        self.backdrop = crate::capture::grab_without_drag_image(tlx, tly, sz, sz).map(Arc::new);
        crate::logf!("menu: capture {:.1} ms", t0.elapsed().as_secs_f32() * 1000.0);
        self.hover = -1;
        self.got_drag = false;
        self.hub_armed = false;
        self.hub_since = None;
        self.idle_ticks = 0;
        self.click_mode = paths.is_some();
        self.swap_started = None;
        match paths {
            Some(p) => self.load(p),
            None => {
                self.paths.clear();
                self.rings.clear();
                self.cat = None;
                self.ring_idx = 0;
                self.spring.clear();
                self.hub_text = t("Drop here").into();
                self.render(SPECULATIVE);
            }
        }
        self.surface = Surface::new(self.size, self.size);
        self.frame = Some(Canvas::new(self.size, self.size));
        self.opened = Instant::now();
        self.visible = true;
        hooks::MENU_UP.store(true, std::sync::atomic::Ordering::Relaxed);
        self.paint();
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, tlx, tly, sz, sz,
                                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
            SetTimer(self.hwnd, TIMER_FRAME, 16, None);
        }
    }

    pub fn close(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        hooks::MENU_UP.store(false, std::sync::atomic::Ordering::Relaxed);
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_FRAME);
            let _ = KillTimer(self.hwnd, TIMER_CLOSE);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        // give the memory back: the next popup rebuilds it in a few ms
        self.surface = None;
        self.frame = None;
        self.backdrop = None;
        self.glass = HashMap::new();
        self.rings = Vec::new();
        self.paths = Vec::new();
        self.spring = Vec::new();
        self.tracking = false;
        crate::app::schedule_trim();
    }

    pub fn load(&mut self, paths: Vec<PathBuf>) {
        let paths: Vec<PathBuf> = paths.iter().map(|p| crate::util::resolve_shortcut(p)).collect();
        let (rings, cat) = actions::rings_for(&paths);
        self.rings = rings.into_iter().filter(|r| !r.items.is_empty()).collect();
        self.cat = cat;
        self.ring_idx = 0;
        self.hub_text = actions::describe(&paths);
        self.paths = paths;
        let counts: Vec<usize> = self.rings.iter().map(|r| r.items.len()).collect();
        for n in counts {
            self.render(n);
        }
        self.spring = vec![0.0; self.items().len()];
        self.dirty = true;
    }

    fn render(&mut self, n: usize) {
        if n == 0 || self.glass.contains_key(&n) || self.rendering.contains(&n) {
            return;
        }
        let Some(bg) = self.backdrop.clone() else { return };
        self.rendering.push(n);
        let (serial, s, hwnd) = (self.serial, self.scale, self.hwnd.0 as isize);
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let field = menu_field(n, s);
            if field.w != bg.w || field.h != bg.h {
                return;
            }
            let g = liquid::render(&bg, &field, &Style::default().scaled(s), None);
            let canvas = liquid::with_shadow(&g.canvas, &field, 26.0 * s,
                                             if g.light { 0.42 } else { 0.30 }, (0, (8.0 * s) as i32));
            crate::logf!("menu: glass for {n} petals in {:.1} ms", t0.elapsed().as_secs_f32() * 1000.0);
            let r = Box::new(Rendered { serial, n, canvas, light: g.light });
            unsafe {
                let p = Box::into_raw(r);
                if PostMessageW(HWND(hwnd as *mut _), WM_MENU_RENDERED, WPARAM(0), LPARAM(p as isize)).is_err() {
                    drop(Box::from_raw(p));
                }
            }
        });
    }

    pub fn on_rendered(&mut self, lp: LPARAM) {
        let r = unsafe { Box::from_raw(lp.0 as *mut Rendered) };
        if r.serial == self.serial && self.visible {
            self.rendering.retain(|&n| n != r.n);
            self.light = r.light;
            self.glass.insert(r.n, Arc::new(r.canvas));
            self.dirty = true;
            self.paint();
        }
    }

    pub fn next_ring(&mut self) {
        if self.rings.len() > 1 {
            self.ring_idx = (self.ring_idx + 1) % self.rings.len();
            self.hover = -1;
            self.spring = vec![0.0; self.items().len()];
            self.swap_started = Some(Instant::now());
            self.dirty = true;
        }
    }

    // ------------------------------------------------------------ geometry
    pub fn hit(&self, lx: f32, ly: f32) -> i32 {
        let s = self.scale;
        let c = self.size as f32 / 2.0;
        let (dx, dy) = (lx - c, c - ly);
        let d = dx.hypot(dy);
        if d <= (R_HUB + 4.0) * s {
            return -2;
        }
        let n = self.items().len();
        if d > (R_OUT + 30.0) * s || n == 0 {
            return -1;
        }
        let ang = dy.atan2(dx).to_degrees().rem_euclid(360.0);
        let rel = (90.0 + 180.0 / n as f32 - ang).rem_euclid(360.0);
        ((rel / (360.0 / n as f32)) as usize % n) as i32
    }

    fn local(&self, x: i32, y: i32) -> (f32, f32) {
        ((x - self.pos.0) as f32, (y - self.pos.1) as f32)
    }

    fn set_hover(&mut self, h: i32) {
        if h != self.hover {
            self.hover = h;
            if h >= 0 {
                self.hub_armed = true;
            }
            self.hub_since = if h == -2 && self.hub_armed && self.rings.len() > 1 { Some(Instant::now()) } else { None };
            self.dirty = true;
        }
    }

    fn take(&mut self, idx: i32) -> Option<Activation> {
        let it = self.items().get(idx as usize)?.clone();
        let a = Activation { key: it.key, paths: self.paths.clone() };
        self.close();
        Some(a)
    }

    // ------------------------------------------------------------ drag & drop (OLE)
    pub fn drag_enter(&mut self, paths: Vec<PathBuf>) -> bool {
        if paths.is_empty() || !self.visible {
            return false;
        }
        self.got_drag = true;
        let resolved: Vec<PathBuf> = paths.iter().map(|p| crate::util::resolve_shortcut(p)).collect();
        if resolved != self.paths {
            self.load(paths);
        }
        true
    }

    pub fn drag_over(&mut self, x: i32, y: i32) {
        let (lx, ly) = self.local(x, y);
        let h = self.hit(lx, ly);
        self.set_hover(h);
    }

    pub fn drag_leave(&mut self) {
        self.set_hover(-1);
    }

    pub fn drop_at(&mut self, x: i32, y: i32) -> Option<Activation> {
        let (lx, ly) = self.local(x, y);
        let idx = self.hit(lx, ly);
        if idx >= 0 {
            self.take(idx)
        } else {
            self.close();
            None
        }
    }

    // ------------------------------------------------------------ click mode
    pub fn mouse_move(&mut self, lx: f32, ly: f32) {
        if !self.click_mode {
            return;
        }
        if !self.tracking {
            let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE,
                                            hwndTrack: self.hwnd, dwHoverTime: 0 };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            self.tracking = true;
        }
        let h = self.hit(lx, ly);
        self.set_hover(h);
    }

    pub fn mouse_leave(&mut self) {
        self.tracking = false;
        if self.click_mode {
            self.set_hover(-1);
        }
    }

    pub fn mouse_up(&mut self, right: bool, lx: f32, ly: f32) -> Option<Activation> {
        if !self.click_mode {
            return None;
        }
        if right {
            self.close();
            return None;
        }
        let idx = self.hit(lx, ly);
        if idx == -2 {
            self.next_ring();
            None
        } else if idx >= 0 {
            self.take(idx)
        } else {
            self.close();
            None
        }
    }

    /// A click anywhere on screen (from the mouse hook): outside the menu closes it.
    pub fn outside_click(&mut self, x: i32, y: i32) {
        if !self.visible || !self.click_mode {
            return;
        }
        let (lx, ly) = self.local(x, y);
        let inside = lx >= 0.0 && ly >= 0.0 && lx < self.size as f32 && ly < self.size as f32;
        if !inside || self.hit(lx, ly) == -1 {
            self.close();
        }
    }

    // ------------------------------------------------------------ trigger events
    pub fn trigger_up(&mut self) {
        if self.visible && !self.click_mode {
            // the drop may still be landing: give it a moment
            unsafe {
                SetTimer(self.hwnd, TIMER_CLOSE, 120, None);
            }
        }
    }

    pub fn trigger_toggle(&mut self) {
        if self.visible && (self.got_drag || self.click_mode) {
            self.next_ring();
        }
    }

    // ------------------------------------------------------------ animation
    pub fn on_timer(&mut self, id: usize) {
        if id == TIMER_CLOSE {
            unsafe {
                let _ = KillTimer(self.hwnd, TIMER_CLOSE);
            }
            if !hooks::left_down() {
                self.close();
            }
            return;
        }
        if !self.visible {
            return;
        }
        let mut moved = self.dirty;
        for i in 0..self.spring.len() {
            let target = if i as i32 == self.hover { 1.0 } else { 0.0 };
            let v = self.spring[i];
            let mut nv = v + (target - v) * 0.32;
            if (nv - target).abs() < 0.003 {
                nv = target;
            }
            if nv != v {
                self.spring[i] = nv;
                moved = true;
            }
        }
        let ht = if self.hover == -2 { 1.0 } else { 0.0 };
        let mut nh = self.hub_spring + (ht - self.hub_spring) * 0.3;
        if (nh - ht).abs() < 0.003 {
            nh = ht;
        }
        if nh != self.hub_spring {
            self.hub_spring = nh;
            moved = true;
        }
        if self.opened.elapsed().as_millis() < 260 {
            moved = true;
        }
        if let Some(s) = self.swap_started {
            if s.elapsed().as_millis() < 260 {
                moved = true;
            }
        }
        if let Some(since) = self.hub_since {
            if since.elapsed().as_millis() >= 420 {
                self.hub_since = None;
                self.next_ring();
                moved = true;
            }
        }
        // no file drag arrived but the cursor is moving: a window move or text selection
        if !self.got_drag && !self.click_mode {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            }
            if (pt.x - self.shown_at.0).abs() + (pt.y - self.shown_at.1).abs() > (6.0 * self.scale) as i32 {
                self.idle_ticks += 1;
                if self.idle_ticks > NO_DRAG_TICKS {
                    self.close();
                    return;
                }
            }
            if !hooks::left_down() && self.opened.elapsed().as_millis() > 200 {
                self.close();
                return;
            }
        }
        if moved {
            self.paint();
        }
    }

    // ------------------------------------------------------------ painting
    fn paint(&mut self) {
        self.dirty = false;
        let Some(mut frame) = self.frame.take() else { return };
        frame.clear();
        self.paint_into(&mut frame);
        if let Some(surf) = self.surface.as_mut() {
            surf.present(self.hwnd, &frame, self.pos.0, self.pos.1, 255);
        }
        self.frame = Some(frame);
    }

    fn paint_into(&self, cv: &mut Canvas) {
        let s = self.scale;
        let sz = self.size as f32;
        let c = sz / 2.0;
        let p = (self.opened.elapsed().as_secs_f32() / 0.19).min(1.0);
        let t = ease_out_back(p);
        let opacity = (t * 1.5).clamp(0.0, 1.0);
        let sg = 0.80 + 0.20 * t;
        let swap = self.swap_started.map(|st| ease_out_cubic((st.elapsed().as_secs_f32() / 0.22).min(1.0))).unwrap_or(1.0);
        let g = |v: f32| (v - c) * sg + c; // global pop-in transform

        let items = self.items();
        let n = items.len();
        let base = if n > 0 { self.glass.get(&n) } else { self.glass.get(&SPECULATIVE) }
            .or_else(|| self.glass.values().next());
        if let Some(b) = base {
            cv.draw_affine(b, sg, c * (1.0 - sg), c * (1.0 - sg), opacity, (0.0, 0.0, sz, sz), |_, _| 1.0);
        }

        let (fg, fg_dim, shadow) = if self.light {
            (Color::rgb(28, 20, 44), Color::rgba(28, 20, 44, 170), Color::rgba(255, 255, 255, 120))
        } else {
            (Color::rgb(255, 255, 255), Color::rgba(255, 255, 255, 190), Color::rgba(0, 0, 0, 110))
        };
        let rot = (1.0 - swap) * -22.0;
        for (i, it) in items.iter().enumerate() {
            let (a0, a1) = angles(n, i);
            let h = self.spring.get(i).copied().unwrap_or(0.0);
            if h > 0.001 {
                if let Some(b) = base {
                    self.paint_hover(cv, b, c, a0, a1, h, sg, opacity);
                }
            }
            let ang = ((a0 + a1) / 2.0 + rot).to_radians();
            let r = ((R_IN + R_OUT) / 2.0 + h * 4.0 + 6.0 * h) * s;
            let (px, py) = (g(c + r * ang.cos()), g(c - r * ang.sin()));
            let k = s * sg;
            let a = opacity * (0.25 + 0.75 * swap);
            let col = if h > 0.5 { Color::rgb(255, 255, 255) } else { fg }.mul_a(a);
            let sh = shadow.mul_a(a);
            if !it.glyph.is_empty() {
                text_in(cv, &Text::new(it.glyph, (18.0 + 3.0 * h) * k, Weight::Regular),
                        (px - 30.0 * k, py - 24.0 * k, 60.0 * k, 24.0 * k), Align::Center, col, Some(sh));
                let label = it.label.to_uppercase();
                let lpx = 10.0 * k;
                let label = gfx::elide(&label, lpx, Weight::Bold, 92.0 * k);
                text_in(cv, &Text::new(&label, lpx, Weight::Bold).spaced(0.7 * k),
                        (px - 46.0 * k, py + 1.0 * k, 92.0 * k, 16.0 * k), Align::Center, col, Some(sh));
            } else {
                text_in(cv, &Text::new(&it.label, (14.0 + 2.0 * h) * k, Weight::Bold).spaced(0.9 * k),
                        (px - 42.0 * k, py - 12.0 * k, 84.0 * k, 24.0 * k), Align::Center, col, Some(sh));
            }
        }
        if n == 0 && !self.paths.is_empty() {
            self.paint_unsupported(cv, c, sg, opacity, fg, fg_dim, shadow);
        }
        self.paint_hub(cv, c, sg, opacity, fg, fg_dim, shadow);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_hover(&self, cv: &mut Canvas, base: &Canvas, c: f32, a0: f32, a1: f32, h: f32, sg: f32, opacity: f32) {
        let s = self.scale;
        let mid = ((a0 + a1) / 2.0).to_radians();
        let r_mid = (R_IN + R_OUT) / 2.0 * s;
        let (pcx, pcy) = (c + r_mid * mid.cos(), c - r_mid * mid.sin());
        let k = 1.0 + 0.07 * h;
        let push = 6.0 * s * h;
        let (ox, oy) = (push * mid.cos(), -push * mid.sin());
        let kk = k * sg;
        let tx = (pcx * (1.0 - k) + ox - c) * sg + c;
        let ty = (pcy * (1.0 - k) + oy - c) * sg + c;
        let (mid_s, half_s) = (mid, ((a1 - a0) / 2.0).to_radians());
        let (r0, r1, cr) = ((R_IN - 1.0) * s, (R_OUT + 1.0) * s, CORNER * s);
        // screen-space bounding box of the petal: sample its outline
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for j in 0..=8 {
            let a = (a0 + (a1 - a0) * j as f32 / 8.0).to_radians();
            for r in [r0, r1] {
                let (sx, sy) = (c + r * a.cos(), c - r * a.sin());
                let (qx, qy) = (sx * kk + tx, sy * kk + ty);
                x0 = x0.min(qx);
                y0 = y0.min(qy);
                x1 = x1.max(qx);
                y1 = y1.max(qy);
            }
        }
        let region = (x0 - 2.0, y0 - 2.0, x1 + 2.0, y1 + 2.0);
        cv.draw_affine(base, kk, tx, ty, opacity, region, |sx, sy| {
            let (dx, dy) = (sx - c, c - sy);
            let d = sd_sector(dx.hypot(dy), dy.atan2(dx), r0, r1, mid_s, half_s, cr);
            (0.5 - d * kk).clamp(0.0, 1.0)
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_unsupported(&self, cv: &mut Canvas, c: f32, sg: f32, opacity: f32, fg: Color, fg_dim: Color, sh: Color) {
        let k = self.scale * sg;
        let ext = self.paths[0].extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_else(|| t("this file").into());
        let r_mid = (R_IN + R_OUT) / 2.0 * k;
        let msg = tf("Can't convert {}", &[&ext]);
        text_in(cv, &Text::new(&msg, 13.0 * k, Weight::Bold), (c - R_OUT * k, c - r_mid - 18.0 * k, 2.0 * R_OUT * k, 18.0 * k),
                Align::Center, fg.mul_a(opacity), Some(sh.mul_a(opacity)));
        text_in(cv, &Text::new(t("Images · PDF · Word · Text · Video · Audio"), 10.0 * k, Weight::Regular),
                (c - R_OUT * k, c - r_mid + 1.0 * k, 2.0 * R_OUT * k, 16.0 * k), Align::Center, fg_dim.mul_a(opacity), None);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_hub(&self, cv: &mut Canvas, c: f32, sg: f32, opacity: f32, fg: Color, fg_dim: Color, sh: Color) {
        let k = self.scale * sg;
        let rh = (R_HUB - 0.5) * k;
        cv.stroke_circle(c, c, rh, 1.2 * k, |x, y| {
            let a = (y - c).atan2(x - c) / std::f32::consts::TAU + 60.0 / 360.0;
            iridescent(a).with_a(150.0 / 255.0 * opacity)
        });
        let items = self.items();
        let hovered = if self.hover >= 0 { items.get(self.hover as usize) } else { None };
        let (top, sub) = match hovered {
            Some(it) => (it.label.clone(), it.hint.clone()),
            None => {
                let sub = if self.rings.len() > 1 {
                    let other = &self.rings[(self.ring_idx + 1) % self.rings.len()].name;
                    format!("⇧ {}", other.to_uppercase())
                } else {
                    self.rings.get(self.ring_idx).map(|r| r.name.to_uppercase()).unwrap_or_default()
                };
                (self.hub_text.clone(), sub)
            }
        };
        let w = (2.0 * R_HUB - 8.0) * k;
        let top = gfx::elide(&top, 14.0 * k, Weight::Bold, w);
        text_in(cv, &Text::new(&top, 14.0 * k, Weight::Bold), (c - w / 2.0, c - 21.0 * k, w, 20.0 * k),
                Align::Center, fg.mul_a(opacity), Some(sh.mul_a(opacity)));
        let sw = (2.0 * R_HUB - 12.0) * k;
        let lines = gfx::wrap(&sub, 10.0 * k, Weight::Regular, sw, 2);
        for (i, line) in lines.iter().enumerate() {
            text_in(cv, &Text::new(line, 10.0 * k, Weight::Regular),
                    (c - sw / 2.0, c + (2.0 + 13.0 * i as f32) * k, sw, 13.0 * k), Align::Center,
                    fg_dim.mul_a(opacity), None);
        }
    }
}

// ------------------------------------------------------------------ OLE drop target

fn files_from(data: &IDataObject) -> Vec<PathBuf> {
    let mut out = Vec::new();
    unsafe {
        let fmt = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let Ok(mut med) = data.GetData(&fmt) else { return out };
        let hdrop = HDROP(med.u.hGlobal.0);
        let n = DragQueryFileW(hdrop, u32::MAX, None);
        for i in 0..n {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            out.push(PathBuf::from(crate::util::from_wide(&buf)));
        }
        ReleaseStgMedium(&mut med);
    }
    out
}

#[implement(IDropTarget)]
struct DropTarget {
    accepted: std::cell::Cell<bool>,
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, data: Option<&IDataObject>, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL,
                 effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let files = data.map(files_from).unwrap_or_default();
        let ok = with_app(|a| {
            let ok = a.menu.drag_enter(files);
            if ok {
                a.menu.drag_over(pt.x, pt.y);
            }
            ok
        }).unwrap_or(false);
        self.accepted.set(ok);
        unsafe {
            // copy, never move: Shift would otherwise make Explorer delete the source
            *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        if self.accepted.get() {
            with_app(|a| a.menu.drag_over(pt.x, pt.y));
        }
        unsafe {
            *effect = if self.accepted.get() { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        with_app(|a| a.menu.drag_leave());
        Ok(())
    }

    fn Drop(&self, _data: Option<&IDataObject>, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL,
            effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let act = with_app(|a| a.menu.drop_at(pt.x, pt.y)).flatten();
        unsafe {
            *effect = if act.is_some() { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
            if let Some(a) = act {
                // return to Explorer's drag loop at once; the work starts on the next message
                let hwnd = with_app(|app| app.main_hwnd).unwrap_or_default();
                let p = Box::into_raw(Box::new(a));
                if PostMessageW(hwnd, WM_MENU_ACTIVATE, WPARAM(0), LPARAM(p as isize)).is_err() {
                    drop(Box::from_raw(p));
                }
            }
        }
        Ok(())
    }
}
