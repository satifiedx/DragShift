//! Tiny software renderer: a premultiplied BGRA canvas (the layout UpdateLayeredWindow wants),
//! anti-aliased shapes from signed distance functions, image blits and text through ab_glyph
//! with the system's Segoe UI fonts memory-mapped (no copy on the heap).

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: a as f32 / 255.0 }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::rgba(r, g, b, 255)
    }
    pub fn with_a(self, a: f32) -> Color {
        Color { a, ..self }
    }
    pub fn mul_a(self, k: f32) -> Color {
        Color { a: self.a * k, ..self }
    }
}

/// Composite a premultiplied pixel (0..255 channels, alpha 0..1) over `p` at opacity `k`.
#[inline]
fn over_px(p: &mut [u8; 4], s: [f32; 4], k: f32) {
    let a = s[3] * k;
    if a <= 0.001 {
        return;
    }
    let inv = 1.0 - a;
    p[0] = (s[0] * k + p[0] as f32 * inv + 0.5).min(255.0) as u8;
    p[1] = (s[1] * k + p[1] as f32 * inv + 0.5).min(255.0) as u8;
    p[2] = (s[2] * k + p[2] as f32 * inv + 0.5).min(255.0) as u8;
    p[3] = (a * 255.0 + p[3] as f32 * inv + 0.5).min(255.0) as u8;
}

/// Premultiplied BGRA pixels, row-major, top-down.
#[derive(Clone)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
}

#[inline]
fn cov(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, px: vec![[0; 4]; w * h] }
    }

    pub fn clear(&mut self) {
        self.px.iter_mut().for_each(|p| *p = [0; 4]);
    }

    #[inline]
    pub fn blend(&mut self, x: usize, y: usize, c: Color, coverage: f32) {
        let a = c.a * coverage;
        if a <= 0.002 {
            return;
        }
        let p = &mut self.px[y * self.w + x];
        let inv = 1.0 - a;
        p[0] = (c.b * a * 255.0 + p[0] as f32 * inv + 0.5) as u8;
        p[1] = (c.g * a * 255.0 + p[1] as f32 * inv + 0.5) as u8;
        p[2] = (c.r * a * 255.0 + p[2] as f32 * inv + 0.5) as u8;
        p[3] = (a * 255.0 + p[3] as f32 * inv + 0.5) as u8;
    }


    fn bounds(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> (usize, usize, usize, usize) {
        let bx0 = x0.floor().max(0.0) as usize;
        let by0 = y0.floor().max(0.0) as usize;
        let bx1 = (x1.ceil().max(0.0) as usize).min(self.w);
        let by1 = (y1.ceil().max(0.0) as usize).min(self.h);
        (bx0, by0, bx1, by1)
    }

    /// Fill where `sdf` < 0 inside the box, colouring each pixel with `color`.
    pub fn fill_sdf(&mut self, bx: (f32, f32, f32, f32), sdf: impl Fn(f32, f32) -> f32,
                    color: impl Fn(f32, f32) -> Color) {
        let (x0, y0, x1, y1) = self.bounds(bx.0 - 1.0, bx.1 - 1.0, bx.2 + 1.0, bx.3 + 1.0);
        for y in y0..y1 {
            let fy = y as f32 + 0.5;
            for x in x0..x1 {
                let fx = x as f32 + 0.5;
                let c = cov(sdf(fx, fy));
                if c > 0.0 {
                    self.blend(x, y, color(fx, fy), c);
                }
            }
        }
    }

    pub fn fill_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        self.fill_sdf((x, y, x + w, y + h), |px, py| sd_rrect(px, py, x, y, w, h, r), |_, _| c);
    }

    pub fn stroke_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32,
                        color: impl Fn(f32, f32) -> Color) {
        let hw = width / 2.0;
        self.fill_sdf((x - hw, y - hw, x + w + hw, y + h + hw),
                      |px, py| sd_rrect(px, py, x, y, w, h, r).abs() - hw, color);
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        self.fill_sdf((cx - r, cy - r, cx + r, cy + r), |x, y| ((x - cx).hypot(y - cy)) - r, |_, _| c);
    }

    pub fn stroke_circle(&mut self, cx: f32, cy: f32, r: f32, width: f32,
                         color: impl Fn(f32, f32) -> Color) {
        let e = r + width;
        self.fill_sdf((cx - e, cy - e, cx + e, cy + e),
                      |x, y| ((x - cx).hypot(y - cy) - r).abs() - width / 2.0, color);
    }

    /// Arc of a circle from `a0` sweeping `sweep` radians (screen angles, clockwise positive).
    pub fn stroke_arc(&mut self, cx: f32, cy: f32, r: f32, width: f32, a0: f32, sweep: f32,
                      color: impl Fn(f32, f32) -> Color) {
        let e = r + width;
        self.fill_sdf((cx - e, cy - e, cx + e, cy + e), |x, y| {
            let ring = ((x - cx).hypot(y - cy) - r).abs() - width / 2.0;
            let ang = (y - cy).atan2(x - cx);
            let rel = (ang - a0).rem_euclid(std::f32::consts::TAU);
            if rel <= sweep {
                ring
            } else {
                // round caps
                let a1 = a0 + sweep;
                let cap = |a: f32| (x - cx - r * a.cos()).hypot(y - cy - r * a.sin());
                cap(a0).min(cap(a1)) - width / 2.0
            }
        }, color);
    }

    /// Bilinear sample (premultiplied, 0..255 channels, alpha 0..1).
    #[inline]
    pub fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let x = x - 0.5;
        let y = y - 0.5;
        if x < -1.0 || y < -1.0 || x > self.w as f32 || y > self.h as f32 {
            return [0.0; 4];
        }
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let get = |ix: i32, iy: i32| -> [f32; 4] {
            if ix < 0 || iy < 0 || ix >= self.w as i32 || iy >= self.h as i32 {
                return [0.0; 4];
            }
            let p = self.px[iy as usize * self.w + ix as usize];
            [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32 / 255.0]
        };
        let (ix, iy) = (x0 as i32, y0 as i32);
        let a = get(ix, iy);
        let b = get(ix + 1, iy);
        let c = get(ix, iy + 1);
        let d = get(ix + 1, iy + 1);
        let mut o = [0.0; 4];
        for i in 0..4 {
            o[i] = a[i] * (1.0 - fx) * (1.0 - fy) + b[i] * fx * (1.0 - fy) + c[i] * (1.0 - fx) * fy + d[i] * fx * fy;
        }
        o
    }


    /// Composite `src` mapped by screen = src·k + (tx, ty) at opacity `alpha`, on all cores.
    /// Unscaled and opaque onto an empty canvas it is a plain copy.
    pub fn draw_scaled(&mut self, src: &Canvas, k: f32, tx: f32, ty: f32, alpha: f32) {
        if (k - 1.0).abs() < 1e-4 && tx.abs() < 1e-3 && ty.abs() < 1e-3 && alpha >= 0.999 && src.w == self.w && src.h == self.h
            && self.px.iter().all(|p| p[3] == 0) {
            self.px.copy_from_slice(&src.px);
            return;
        }
        let w = self.w;
        crate::shape::par_rows(&mut self.px, w, |y, row| {
            let sy = (y as f32 + 0.5 - ty) / k;
            if sy < -1.0 || sy > src.h as f32 + 1.0 {
                return;
            }
            for (x, p) in row.iter_mut().enumerate() {
                let sx = (x as f32 + 0.5 - tx) / k;
                let s = src.sample(sx, sy);
                over_px(p, s, alpha);
            }
        });
    }

    /// Paint a straight-alpha BGRA thumbnail inside a rounded rect.
    pub fn draw_thumb(&mut self, bgra: &[u8], tw: usize, th: usize, x: f32, y: f32, size: f32, r: f32) {
        let scale = (tw.min(th) as f32) / size;
        let (ox, oy) = ((tw as f32 - size * scale) / 2.0, (th as f32 - size * scale) / 2.0);
        self.fill_sdf((x, y, x + size, y + size), |px, py| sd_rrect(px, py, x, y, size, size, r), |px, py| {
            let sx = (((px - x) * scale + ox) as usize).min(tw - 1);
            let sy = (((py - y) * scale + oy) as usize).min(th - 1);
            let i = (sy * tw + sx) * 4;
            Color { b: bgra[i] as f32 / 255.0, g: bgra[i + 1] as f32 / 255.0, r: bgra[i + 2] as f32 / 255.0,
                    a: bgra[i + 3] as f32 / 255.0 }
        });
    }
}

/// Signed distance to a rounded rectangle.
#[inline]
pub fn sd_rrect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    let qx = (px - cx).abs() - (w / 2.0 - r);
    let qy = (py - cy).abs() - (h / 2.0 - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

/// Signed distance to an annular sector between angles a0..a1 (degrees, CCW from +x, y up)
/// with rounded corners: same construction as the Qt/numpy version.
#[inline]
pub fn sd_sector(rad: f32, ang: f32, r0: f32, r1: f32, mid: f32, half: f32, round_r: f32) -> f32 {
    use std::f32::consts::PI;
    let mut a = ang - mid;
    a = (a + PI).rem_euclid(2.0 * PI) - PI;
    let off = a.abs() - half;
    let side = if off > PI / 2.0 { rad } else { rad * off.sin() };
    let ring = (r0 - rad).max(rad - r1);
    let ring_s = ring + round_r;
    let side_s = side + round_r;
    ring_s.max(0.0).hypot(side_s.max(0.0)) + ring_s.max(side_s).min(0.0) - round_r
}

// ------------------------------------------------------------------ fonts and text

pub struct Fonts {
    pub regular: FontRef<'static>,
    pub bold: FontRef<'static>,
    pub semibold: FontRef<'static>,
    pub symbol: Option<FontRef<'static>>,
    pub emoji: Option<FontRef<'static>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Weight {
    Regular,
    Semibold,
    Bold,
}

fn map_font(name: &str) -> Option<FontRef<'static>> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let path = format!("{windir}\\Fonts\\{name}");
    let bytes = crate::util::map_file(&path)?;
    FontRef::try_from_slice(bytes).ok()
}

static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();

pub fn fonts() -> Option<&'static Fonts> {
    FONTS
        .get_or_init(|| {
            let regular = map_font("segoeui.ttf").or_else(|| map_font("arial.ttf"))?;
            let bold = map_font("segoeuib.ttf").unwrap_or_else(|| regular.clone());
            let semibold = map_font("seguisb.ttf").unwrap_or_else(|| bold.clone());
            Some(Fonts { regular, bold, semibold, symbol: map_font("seguisym.ttf"), emoji: map_font("seguiemj.ttf") })
        })
        .as_ref()
}

impl Fonts {
    fn face(&self, w: Weight) -> &FontRef<'static> {
        match w {
            Weight::Regular => &self.regular,
            Weight::Semibold => &self.semibold,
            Weight::Bold => &self.bold,
        }
    }

    /// The font that has `c`, falling back to Segoe UI Symbol / Emoji.
    fn pick(&self, w: Weight, c: char) -> &FontRef<'static> {
        let f = self.face(w);
        if f.glyph_id(c).0 != 0 {
            return f;
        }
        for alt in [&self.symbol, &self.emoji].into_iter().flatten() {
            if alt.glyph_id(c).0 != 0 {
                return alt;
            }
        }
        f
    }
}

/// Em-size `px` (like CSS/Qt pixel size) to ab_glyph's line-height based scale.
fn scale_for(f: &FontRef<'static>, px: f32) -> PxScale {
    let upem = f.units_per_em().unwrap_or(2048.0);
    let h = f.ascent_unscaled() - f.descent_unscaled();
    PxScale::from(px * h / upem)
}

pub struct Text<'a> {
    pub s: &'a str,
    pub px: f32,
    pub weight: Weight,
    pub spacing: f32,
}

impl<'a> Text<'a> {
    pub fn new(s: &'a str, px: f32, weight: Weight) -> Text<'a> {
        Text { s, px, weight, spacing: 0.0 }
    }
    pub fn spaced(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }
}

pub fn measure(t: &Text) -> f32 {
    let Some(fs) = fonts() else { return 0.0 };
    let mut w = 0.0;
    let mut prev: Option<(ab_glyph::GlyphId, *const FontRef<'static>)> = None;
    for c in t.s.chars() {
        let f = fs.pick(t.weight, c);
        let sf = f.as_scaled(scale_for(f, t.px));
        let id = sf.glyph_id(c);
        if let Some((p, pf)) = prev {
            if std::ptr::eq(pf, f) {
                w += sf.kern(p, id);
            }
        }
        w += sf.h_advance(id) + t.spacing;
        prev = Some((id, f as *const _));
    }
    (w - t.spacing).max(0.0)
}

/// Vertical metrics in pixels: (ascent, descent as a positive number).
pub fn metrics(px: f32, weight: Weight) -> (f32, f32) {
    let Some(fs) = fonts() else { return (px, px * 0.25) };
    let f = fs.face(weight);
    let sf = f.as_scaled(scale_for(f, px));
    (sf.ascent(), -sf.descent())
}

/// Coverage → alpha with a touch of gamma (c^0.85): grayscale AA otherwise looks thin.
fn gamma(c: f32) -> u8 {
    static LUT: OnceLock<[u8; 256]> = OnceLock::new();
    let t = LUT.get_or_init(|| std::array::from_fn(|i| ((i as f32 / 255.0).powf(0.85) * 255.0 + 0.5) as u8));
    t[(c.clamp(0.0, 1.0) * 255.0 + 0.5) as usize]
}

/// A rasterised glyph: coverage (gamma applied) and its offset from the whole-pixel pen position.
struct Glyph {
    x: i32,
    y: i32,
    w: usize,
    cov: Vec<u8>,
}

type GlyphKey = (usize, u16, u32, u8);

thread_local! {
    // Glyphs at quarter-pixel positions: animated frames draw the same few labels over and over.
    static GLYPHS: std::cell::RefCell<std::collections::HashMap<GlyphKey, Option<std::rc::Rc<Glyph>>>> =
        Default::default();
}

/// Let the glyph cache go (the UI is idle).
pub fn forget_glyphs() {
    GLYPHS.with(|g| *g.borrow_mut() = Default::default());
}

fn glyph(f: &FontRef<'static>, id: ab_glyph::GlyphId, scale: PxScale, sub: (u8, u8)) -> Option<std::rc::Rc<Glyph>> {
    let key = (f as *const _ as usize, id.0, (scale.y * 16.0) as u32, sub.0 * 4 + sub.1);
    GLYPHS.with(|cache| {
        if let Some(g) = cache.borrow().get(&key) {
            return g.clone();
        }
        let pos = ab_glyph::point(sub.0 as f32 / 4.0, sub.1 as f32 / 4.0);
        let g = f.outline_glyph(id.with_scale_and_position(scale, pos)).map(|o| {
            let b = o.px_bounds();
            let (w, h) = (b.width() as usize, b.height() as usize);
            let mut cov = vec![0u8; w * h];
            o.draw(|gx, gy, c| {
                if (gx as usize) < w && (gy as usize) < h {
                    cov[gy as usize * w + gx as usize] = gamma(c);
                }
            });
            std::rc::Rc::new(Glyph { x: b.min.x as i32, y: b.min.y as i32, w, cov })
        });
        let mut c = cache.borrow_mut();
        if c.len() > 4096 {
            c.clear();
        }
        c.insert(key, g.clone());
        g
    })
}

/// Draw text with its left edge at `x` and baseline at `baseline`.
pub fn draw_text(cv: &mut Canvas, t: &Text, x: f32, baseline: f32, color: Color) {
    let Some(fs) = fonts() else { return };
    let mut pen = x;
    let qy = (baseline * 4.0).round() / 4.0;
    let (iy, sy) = (qy.floor(), ((qy - qy.floor()) * 4.0) as u8);
    let mut prev: Option<(ab_glyph::GlyphId, *const FontRef<'static>)> = None;
    let (cw, ch) = (cv.w as i32, cv.h as i32);
    for c in t.s.chars() {
        let f = fs.pick(t.weight, c);
        let scale = scale_for(f, t.px);
        let sf = f.as_scaled(scale);
        let id = sf.glyph_id(c);
        if let Some((p, pf)) = prev {
            if std::ptr::eq(pf, f) {
                pen += sf.kern(p, id);
            }
        }
        let qx = (pen * 4.0).round() / 4.0;
        let (ix, sx) = (qx.floor(), ((qx - qx.floor()) * 4.0) as u8);
        if let Some(g) = glyph(f, id, scale, (sx, sy)) {
            let (ox, oy) = (ix as i32 + g.x, iy as i32 + g.y);
            for (gy, row) in g.cov.chunks_exact(g.w.max(1)).enumerate() {
                let y = oy + gy as i32;
                if y < 0 || y >= ch {
                    continue;
                }
                for (gx, &a) in row.iter().enumerate() {
                    let x = ox + gx as i32;
                    if a > 0 && x >= 0 && x < cw {
                        cv.blend(x as usize, y as usize, color, a as f32 / 255.0);
                    }
                }
            }
        }
        pen += sf.h_advance(id) + t.spacing;
        prev = Some((id, f as *const _));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Single line, vertically centred in the box, optional 1 px drop shadow.
pub fn text_in(cv: &mut Canvas, t: &Text, bx: (f32, f32, f32, f32), align: Align, color: Color,
               shadow: Option<Color>) {
    let w = measure(t);
    let (asc, desc) = metrics(t.px, t.weight);
    let x = match align {
        Align::Left => bx.0,
        Align::Center => bx.0 + (bx.2 - w) / 2.0,
        Align::Right => bx.0 + bx.2 - w,
    };
    let baseline = bx.1 + (bx.3 + asc - desc) / 2.0 - asc * 0.04;
    if let Some(s) = shadow {
        draw_text(cv, t, x, baseline + 1.0, s);
    }
    draw_text(cv, t, x, baseline, color);
}

/// Cut `s` to fit `maxw`, ending with an ellipsis.
pub fn elide(s: &str, px: f32, weight: Weight, maxw: f32) -> String {
    if measure(&Text::new(s, px, weight)) <= maxw {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut n = chars.len();
    while n > 0 {
        n -= 1;
        let cand: String = chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
        if measure(&Text::new(&cand, px, weight)) <= maxw {
            return cand;
        }
    }
    "…".into()
}


/// Greedy word wrap into at most `max_lines` lines (the last one elided).
pub fn wrap(s: &str, px: f32, weight: Weight, maxw: f32, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let words: Vec<&str> = s.split(' ').collect();
    let mut i = 0;
    while i < words.len() {
        let cand = if cur.is_empty() { words[i].to_string() } else { format!("{cur} {}", words[i]) };
        if measure(&Text::new(&cand, px, weight)) <= maxw || cur.is_empty() {
            cur = cand;
            i += 1;
        } else {
            lines.push(std::mem::take(&mut cur));
            if lines.len() == max_lines - 1 {
                let rest = words[i..].join(" ");
                lines.push(elide(&rest, px, weight, maxw));
                return lines;
            }
        }
    }
    if !cur.is_empty() {
        if lines.len() < max_lines {
            lines.push(elide(&cur, px, weight, maxw));
        }
    }
    lines
}
