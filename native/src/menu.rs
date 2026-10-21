//! The radial drop menu that pops up under the cursor while you Shift+drag a file.
//!
//! Flat matte petals around a hub. The shape (with its soft shadow) is drawn once per petal
//! count on a worker thread; every frame only composites it and paints the hovered petal and the
//! labels, in step with the monitor's refresh (see vsync.rs). Everything it allocates is dropped
//! again when it closes, so it costs nothing while idle.

use crate::actions::{self, Item, Ring};
use crate::app::with_app;
use crate::gfx::{self, sd_sector, text_in, Align, Canvas, Text, Weight};
use crate::hooks;
use crate::i18n::{t, tf};
use crate::layered::{self, Surface};
use crate::shape::{self, Field};
use crate::theme::{self, Palette};
use crate::vsync;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use windows::core::implement;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_MENU_RENDERED: u32 = WM_APP + 20;
pub const WM_MENU_ACTIVATE: u32 = WM_APP + 21;
pub const TIMER_CLOSE: usize = 2;

const SIZE: f32 = 400.0;
const R_OUT: f32 = 158.0;
const R_IN: f32 = 60.0;
const R_HUB: f32 = 50.0;
const GAP_DEG: f32 = 3.2;
const CORNER: f32 = 10.0;
const SPECULATIVE: usize = 8; // petals shown while we wait to learn what is being dragged
const NO_DRAG_SECS: f32 = 0.8;
const POP_SECS: f32 = 0.19;
const SWAP_SECS: f32 = 0.22;

pub struct Activation {
    pub key: String,
    pub paths: Vec<PathBuf>,
}

pub struct Rendered {
    serial: u32,
    n: usize,
    canvas: Canvas,
    tiles: Vec<Tile>,
}

/// Coverage of one petal at rest, cut to its bounding box: the hovered petal is drawn from this
/// (scaled and pushed out) instead of evaluating its shape for every pixel of every frame.
pub struct Tile {
    x0: f32,
    y0: f32,
    w: usize,
    h: usize,
    cov: Vec<u8>,
}

impl Tile {
    /// Bilinear coverage at canvas position (x, y), 0..1.
    #[inline]
    fn at(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = (x - self.x0 - 0.5, y - self.y0 - 0.5);
        if fx < -1.0 || fy < -1.0 || fx >= self.w as f32 || fy >= self.h as f32 {
            return 0.0;
        }
        let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let get = |x: i32, y: i32| {
            if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 { 0.0 } else { self.cov[y as usize * self.w + x as usize] as f32 }
        };
        let top = get(ix, iy) * (1.0 - tx) + get(ix + 1, iy) * tx;
        let bot = get(ix, iy + 1) * (1.0 - tx) + get(ix + 1, iy + 1) * tx;
        (top * (1.0 - ty) + bot * ty) / 255.0
    }
}

/// Cut each petal out of the menu's field (the hub and the neighbours left out), on all cores.
fn petal_tiles(n: usize, s: f32, field: &Field) -> Vec<Tile> {
    let c = field.w as f32 / 2.0;
    let (r0, r1) = (R_IN * s, R_OUT * s);
    let inner = (R_HUB + R_IN) / 2.0 * s;
    std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..n).map(|i| scope.spawn(move || {
            let (a0, a1) = angles(n, i);
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for j in 0..=16 {
                let a = (a0 + (a1 - a0) * j as f32 / 16.0).to_radians();
                for r in [r0, r1] {
                    let (x, y) = (c + r * a.cos(), c - r * a.sin());
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
            let (x0, y0) = ((x0 - 2.0).floor().max(0.0), (y0 - 2.0).floor().max(0.0));
            let w = (((x1 + 2.0).ceil() - x0) as usize).min(field.w - x0 as usize);
            let h = (((y1 + 2.0).ceil() - y0) as usize).min(field.h - y0 as usize);
            let mut cov = vec![0u8; w * h];
            for (ty, row) in cov.chunks_exact_mut(w).enumerate() {
                let fy = y0 as usize + ty;
                for (tx, v) in row.iter_mut().enumerate() {
                    let fx = x0 as usize + tx;
                    let (dx, dy) = (fx as f32 + 0.5 - c, c - (fy as f32 + 0.5));
                    if dx.hypot(dy) < inner || petal_at(n, dy.atan2(dx)).0 != i {
                        continue;
                    }
                    let d = field.d[fy * field.w + fx];
                    *v = ((0.5 - d).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
            Tile { x0, y0, w, h, cov }
        })).collect();
        jobs.into_iter().map(|j| j.join().unwrap()).collect()
    })
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

/// Index of the petal whose middle is nearest to the math angle `ang` (radians), and the
/// fractional position between petals (petal i sits at 90° − i·span).
fn petal_at(n: usize, ang: f32) -> (usize, f32) {
    let span = std::f32::consts::TAU / n as f32;
    let f = (std::f32::consts::FRAC_PI_2 - ang).rem_euclid(std::f32::consts::TAU) / span;
    ((f.round() as usize) % n, f)
}

/// Signed distance field of the whole menu (hub + n petals) at scale `s`. Each pixel only
/// looks at the two petals it lies between.
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
            let (_, f) = petal_at(n, ang);
            let lo = f.floor() as usize;
            for i in [lo % n, (lo + 1) % n] {
                let (mid, half) = secs[i];
                d = d.min(sd_sector(rad, ang, R_IN * s, R_OUT * s, mid, half, CORNER * s));
            }
        }
        d
    })
}

/// The menu at rest: matte petals, a darker hub, hairline rims and a soft shadow.
fn render_shape(n: usize, s: f32, pal: &Palette) -> (Canvas, Vec<Tile>) {
    let field = menu_field(n, s);
    let c = field.w as f32 / 2.0;
    let split = (R_HUB + R_IN) / 2.0 * s;
    let flat = shape::fill(&field, |x, y| if (x - c).hypot(y - c) < split { pal.bg } else { pal.sector },
                           pal.line.with_a(0.9), 1.0 * s);
    (shape::with_shadow(&flat, &field, 22.0 * s, pal.shadow, (0, (6.0 * s) as i32)), petal_tiles(n, s, &field))
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
    pub mon: HMONITOR,
    pal: Palette,
    stray: f32,
    shapes: HashMap<usize, Arc<Canvas>>,
    tiles: HashMap<usize, Vec<Tile>>,
    still: Option<(usize, usize, Canvas)>, // (ring, petals, shape + labels at rest)
    rendering: Vec<usize>,
    serial: u32,
    opened: Instant,
    pop_start: Option<Instant>, // the pop-in starts once there is a shape to show
    cache_key: (u32, u32),      // scale and palette the cached shapes were drawn for
    last_frame: Instant,
    swap_started: Option<Instant>,
    spring: Vec<f32>,
    hub_spring: f32,
    surface: Option<Surface>,
    frame: Option<Canvas>,
    tracking: bool,
    dirty: bool,
    stats: (u32, u32, f32), // frames, paints, paint seconds: logged on close
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
            shown_at: (0, 0), mon: HMONITOR::default(), pal: theme::DARK, stray: 0.0, shapes: HashMap::new(),
            tiles: HashMap::new(), still: None, rendering: vec![], serial: 0, opened: Instant::now(), pop_start: None, cache_key: (0, 0), last_frame: Instant::now(), swap_started: None,
            spring: vec![], hub_spring: 0.0, surface: None, frame: None, tracking: false, dirty: true,
            stats: (0, 0, 0.0),
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
        self.mon = vsync::monitor_of(x, y);
        self.pal = theme::current();
        self.serial += 1;
        let key = (s.to_bits(), self.pal.bg.r.to_bits());
        if key != self.cache_key {
            self.shapes.clear();
            self.tiles.clear();
            self.cache_key = key;
        }
        self.still = None;
        self.rendering.clear();
        self.hover = -1;
        self.hub_spring = 0.0;
        self.got_drag = false;
        self.stray = 0.0;
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
        self.pop_start = (!self.shapes.is_empty()).then_some(self.opened);
        self.last_frame = self.opened;
        self.stats = (0, 0, 0.0);
        self.visible = true;
        hooks::MENU_UP.store(true, std::sync::atomic::Ordering::Relaxed);
        self.paint();
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, tlx, tly, sz, sz,
                                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        vsync::kick(self.mon);
    }

    pub fn close(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        hooks::MENU_UP.store(false, std::sync::atomic::Ordering::Relaxed);
        let (frames, paints, secs) = self.stats;
        let up = self.opened.elapsed().as_secs_f32();
        crate::logf!("menu: {:.2}s up, {:.0} frames/s, {} paints, {:.2} ms each", up, frames as f32 / up.max(1e-3),
                     paints, secs * 1000.0 / paints.max(1) as f32);
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_CLOSE);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        // give the memory back; the shapes stay until the idle trim, for a quick second drag
        self.surface = None;
        self.frame = None;
        self.still = None;
        self.rings = Vec::new();
        self.paths = Vec::new();
        self.spring = Vec::new();
        self.tracking = false;
        crate::app::schedule_trim();
    }

    /// The UI went idle: drop the cached shapes too.
    pub fn forget(&mut self) {
        if !self.visible {
            self.shapes = HashMap::new();
            self.tiles = HashMap::new();
            self.cache_key = (0, 0);
        }
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
        self.still = None;
        self.dirty = true;
    }

    fn render(&mut self, n: usize) {
        if n == 0 || self.shapes.contains_key(&n) || self.rendering.contains(&n) {
            return;
        }
        self.rendering.push(n);
        let (serial, s, hwnd, pal) = (self.serial, self.scale, self.hwnd.0 as isize, self.pal);
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let (canvas, tiles) = render_shape(n, s, &pal);
            crate::logf!("menu: shape for {n} petals in {:.1} ms", t0.elapsed().as_secs_f32() * 1000.0);
            let r = Box::new(Rendered { serial, n, canvas, tiles });
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
            let r = *r;
            self.shapes.insert(r.n, Arc::new(r.canvas));
            self.tiles.insert(r.n, r.tiles);
            self.still = None;
            if self.pop_start.is_none() {
                self.pop_start = Some(Instant::now());
            }
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

    /// One press of the trigger key flips to the next ring, exactly once.
    pub fn trigger_toggle(&mut self) {
        if self.visible && (self.got_drag || self.click_mode) {
            self.next_ring();
        }
    }

    pub fn on_timer(&mut self, id: usize) {
        if id == TIMER_CLOSE {
            unsafe {
                let _ = KillTimer(self.hwnd, TIMER_CLOSE);
            }
            if !hooks::left_down() {
                self.close();
            }
        }
    }

    // ------------------------------------------------------------ animation
    /// One refresh of the monitor. Frames keep coming while the menu is up.
    pub fn on_frame(&mut self) {
        if !self.visible {
            return;
        }
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.stats.0 += 1;
        let mut moved = self.dirty;
        for i in 0..self.spring.len() {
            let target = if i as i32 == self.hover { 1.0 } else { 0.0 };
            let v = self.spring[i];
            let nv = theme::approach(v, target, 0.30, dt);
            if nv != v {
                self.spring[i] = nv;
                moved = true;
            }
        }
        let ht = if self.hover == -2 && self.click_mode && self.rings.len() > 1 { 1.0 } else { 0.0 };
        let nh = theme::approach(self.hub_spring, ht, 0.30, dt);
        if nh != self.hub_spring {
            self.hub_spring = nh;
            moved = true;
        }
        // keep painting one frame past the end so the last step lands exactly on 1.0
        if self.pop_start.is_none_or(|p| p.elapsed().as_secs_f32() < POP_SECS + 0.05) {
            moved = true;
        }
        if let Some(s) = self.swap_started {
            if s.elapsed().as_secs_f32() < SWAP_SECS + 0.05 {
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
                self.stray += dt;
                if self.stray > NO_DRAG_SECS {
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
    fn swapping(&self) -> bool {
        self.swap_started.map(|s| s.elapsed().as_secs_f32() < SWAP_SECS).unwrap_or(false)
    }

    /// Bake the current ring's labels into a copy of its shape: frames then only paint what moves.
    fn ensure_still(&mut self) {
        let n = self.items().len();
        if n == 0 || self.swapping() || self.still.as_ref().is_some_and(|s| s.0 == self.ring_idx && s.1 == n) {
            return;
        }
        let Some(base) = self.shapes.get(&n) else { return };
        let mut cv = (**base).clone();
        for i in 0..n {
            self.paint_label(&mut cv, i, 0.0, 1.0, 0.0, 1.0);
        }
        self.still = Some((self.ring_idx, n, cv));
    }

    fn paint(&mut self) {
        let t0 = Instant::now();
        self.dirty = false;
        self.ensure_still();
        let Some(mut frame) = self.frame.take() else { return };
        frame.clear();
        self.paint_into(&mut frame);
        if let Some(surf) = self.surface.as_mut() {
            surf.present(self.hwnd, &frame, self.pos.0, self.pos.1, 255);
        }
        self.frame = Some(frame);
        self.stats.1 += 1;
        self.stats.2 += t0.elapsed().as_secs_f32();
    }

    fn paint_into(&self, cv: &mut Canvas) {
        let sz = self.size as f32;
        let c = sz / 2.0;
        let Some(pop) = self.pop_start else { return };
        let p = (pop.elapsed().as_secs_f32() / POP_SECS).min(1.0);
        let t = ease_out_back(p);
        let opacity = (t * 1.5).clamp(0.0, 1.0);
        let sg = 0.86 + 0.14 * t;
        let swap = self.swap_started.map(|st| ease_out_cubic((st.elapsed().as_secs_f32() / SWAP_SECS).min(1.0))).unwrap_or(1.0);
        let n = self.items().len();
        let still = self.still.as_ref().filter(|st| st.0 == self.ring_idx && st.1 == n && !self.swapping());
        let base = if n > 0 { self.shapes.get(&n) } else { self.shapes.get(&SPECULATIVE) }
            .or_else(|| self.shapes.values().next());
        if let Some(b) = still.map(|st| &st.2).or(base.map(|b| &**b)) {
            cv.draw_scaled(b, sg, c * (1.0 - sg), c * (1.0 - sg), opacity);
        }
        let rot = (1.0 - swap) * -18.0;
        let a = opacity * (0.2 + 0.8 * swap);
        for i in 0..n {
            let h = self.spring.get(i).copied().unwrap_or(0.0);
            if h > 0.001 {
                self.paint_hover(cv, c, i, h, sg, opacity);
            }
            if still.is_none() || h > 0.001 {
                self.paint_label(cv, i, h, sg, rot, a);
            }
        }
        if n == 0 && !self.paths.is_empty() {
            self.paint_unsupported(cv, c, sg, opacity);
        }
        self.paint_hub(cv, c, sg, opacity);
    }

    /// Glyph and name of petal `i`, `h` = how hovered, `sg` = pop-in scale, `rot` = swap turn.
    fn paint_label(&self, cv: &mut Canvas, i: usize, h: f32, sg: f32, rot: f32, a: f32) {
        let items = self.items();
        let Some(it) = items.get(i) else { return };
        let (pal, s) = (&self.pal, self.scale);
        let c = self.size as f32 / 2.0;
        let g = |v: f32| (v - c) * sg + c;
        let (a0, a1) = angles(items.len(), i);
        let ang = ((a0 + a1) / 2.0 + rot).to_radians();
        let r = ((R_IN + R_OUT) / 2.0 + 6.0 * h) * s;
        let (px, py) = (g(c + r * ang.cos()), g(c - r * ang.sin()));
        let k = s * sg;
        let col = theme::mix(pal.text, pal.on_accent, h).mul_a(a);
        let dim = theme::mix(pal.dim, pal.on_accent, h).mul_a(a);
        if !it.glyph.is_empty() {
            text_in(cv, &Text::new(it.glyph, 18.0 * k, Weight::Regular),
                    (px - 30.0 * k, py - 24.0 * k, 60.0 * k, 24.0 * k), Align::Center, col, None);
            let label = it.label.to_uppercase();
            let lpx = 10.0 * k;
            let label = gfx::elide(&label, lpx, Weight::Bold, 92.0 * k);
            text_in(cv, &Text::new(&label, lpx, Weight::Bold).spaced(0.7 * k),
                    (px - 46.0 * k, py + 1.0 * k, 92.0 * k, 16.0 * k), Align::Center, dim, None);
        } else {
            text_in(cv, &Text::new(&it.label, 14.0 * k, Weight::Bold).spaced(0.9 * k),
                    (px - 42.0 * k, py - 12.0 * k, 84.0 * k, 24.0 * k), Align::Center, col, None);
        }
    }

    /// The hovered petal: it grows a little, moves out and fills with the accent colour.
    fn paint_hover(&self, cv: &mut Canvas, c: f32, i: usize, h: f32, sg: f32, opacity: f32) {
        let n = self.items().len();
        let Some(tile) = self.tiles.get(&n).and_then(|t| t.get(i)) else { return };
        let s = self.scale;
        let (a0, a1) = angles(n, i);
        let mid = ((a0 + a1) / 2.0).to_radians();
        let r_mid = (R_IN + R_OUT) / 2.0 * s;
        let (pcx, pcy) = (c + r_mid * mid.cos(), c - r_mid * mid.sin());
        let k = 1.0 + 0.05 * h;
        let push = 6.0 * s * h;
        let (ox, oy) = (push * mid.cos(), -push * mid.sin());
        // screen = source·kk + (tx, ty): grow around the petal's middle, push out, then pop-in
        let kk = k * sg;
        let tx = (pcx * (1.0 - k) + ox - c) * sg + c;
        let ty = (pcy * (1.0 - k) + oy - c) * sg + c;
        let col = theme::mix(self.pal.sector, self.pal.accent, h).mul_a(opacity);
        let (bx0, by0) = (tile.x0 * kk + tx - 1.0, tile.y0 * kk + ty - 1.0);
        let (bx1, by1) = ((tile.x0 + tile.w as f32) * kk + tx + 1.0, (tile.y0 + tile.h as f32) * kk + ty + 1.0);
        let (x0, y0) = (bx0.floor().max(0.0) as usize, by0.floor().max(0.0) as usize);
        let (x1, y1) = ((bx1.ceil().max(0.0) as usize).min(cv.w), (by1.ceil().max(0.0) as usize).min(cv.h));
        for y in y0..y1 {
            let sy = (y as f32 + 0.5 - ty) / kk;
            for x in x0..x1 {
                let cov = tile.at((x as f32 + 0.5 - tx) / kk, sy);
                if cov > 0.0 {
                    cv.blend(x, y, col, cov);
                }
            }
        }
    }

    fn paint_unsupported(&self, cv: &mut Canvas, c: f32, sg: f32, opacity: f32) {
        let k = self.scale * sg;
        let ext = self.paths[0].extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_else(|| t("this file").into());
        let r_mid = (R_IN + R_OUT) / 2.0 * k;
        let msg = tf("Can't convert {}", &[&ext]);
        text_in(cv, &Text::new(&msg, 13.0 * k, Weight::Bold), (c - R_OUT * k, c - r_mid - 18.0 * k, 2.0 * R_OUT * k, 18.0 * k),
                Align::Center, self.pal.text.mul_a(opacity), None);
        text_in(cv, &Text::new(t("Images · PDF · Word · Text · Video · Audio"), 10.0 * k, Weight::Regular),
                (c - R_OUT * k, c - r_mid + 1.0 * k, 2.0 * R_OUT * k, 16.0 * k), Align::Center, self.pal.dim.mul_a(opacity), None);
    }

    fn paint_hub(&self, cv: &mut Canvas, c: f32, sg: f32, opacity: f32) {
        let pal = &self.pal;
        let k = self.scale * sg;
        if self.hub_spring > 0.001 {
            let rh = (R_HUB - 1.0) * k;
            cv.stroke_circle(c, c, rh, 1.6 * k, |_, _| pal.accent.mul_a(self.hub_spring * opacity));
        }
        let items = self.items();
        let hovered = if self.hover >= 0 { items.get(self.hover as usize) } else { None };
        let (top, sub, sub_col) = match hovered {
            Some(it) => (it.label.clone(), it.hint.clone(), pal.dim),
            None if self.rings.len() > 1 => {
                let other = &self.rings[(self.ring_idx + 1) % self.rings.len()].name;
                (self.hub_text.clone(), format!("⇧ {}", other.to_uppercase()), pal.accent)
            }
            None => {
                let name = self.rings.get(self.ring_idx).map(|r| r.name.to_uppercase()).unwrap_or_default();
                (self.hub_text.clone(), name, pal.dim)
            }
        };
        let w = (2.0 * R_HUB - 8.0) * k;
        let top = gfx::elide(&top, 14.0 * k, Weight::Bold, w);
        text_in(cv, &Text::new(&top, 14.0 * k, Weight::Bold), (c - w / 2.0, c - 21.0 * k, w, 20.0 * k),
                Align::Center, pal.text.mul_a(opacity), None);
        let sw = (2.0 * R_HUB - 12.0) * k;
        let lines = gfx::wrap(&sub, 10.0 * k, Weight::Regular, sw, 2);
        for (i, line) in lines.iter().enumerate() {
            text_in(cv, &Text::new(line, 10.0 * k, Weight::Regular),
                    (c - sw / 2.0, c + (2.0 + 13.0 * i as f32) * k, sw, 13.0 * k), Align::Center,
                    sub_col.mul_a(opacity), None);
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
