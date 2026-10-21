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

pub const WHITE: Color = Color::rgb(255, 255, 255);

/// Pastel rainbow used for the iridescent rims (same stops as the Qt version).
pub const IRIDESCENT: [Color; 8] = [
    Color::rgb(0xff, 0x6b, 0x9d),
    Color::rgb(0xff, 0xb3, 0x6b),
    Color::rgb(0xff, 0xe6, 0x6b),
    Color::rgb(0x7d, 0xff, 0xb0),
    Color::rgb(0x6b, 0xd5, 0xff),
    Color::rgb(0x8a, 0x7b, 0xff),
    Color::rgb(0xe0, 0x7b, 0xff),
    Color::rgb(0xff, 0x6b, 0x9d),
];

pub fn iridescent(t: f32) -> Color {
    let t = t.rem_euclid(1.0) * 7.0;
    let i = (t.floor() as usize).min(6);
    let f = t - i as f32;
    let (a, b) = (IRIDESCENT[i], IRIDESCENT[i + 1]);
    Color { r: a.r + (b.r - a.r) * f, g: a.g + (b.g - a.g) * f, b: a.b + (b.b - a.b) * f, a: 1.0 }
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

    /// Composite a premultiplied pixel over this one.
    #[inline]
    pub fn over(&mut self, x: usize, y: usize, s: [f32; 4], k: f32) {
        let a = s[3] * k;
        if a <= 0.001 {
            return;
        }
        let p = &mut self.px[y * self.w + x];
        let inv = 1.0 - a;
        p[0] = (s[0] * k + p[0] as f32 * inv + 0.5).min(255.0) as u8;
        p[1] = (s[1] * k + p[1] as f32 * inv + 0.5).min(255.0) as u8;
        p[2] = (s[2] * k + p[2] as f32 * inv + 0.5).min(255.0) as u8;
        p[3] = (a * 255.0 + p[3] as f32 * inv + 0.5).min(255.0) as u8;
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
            if rel <= sweep { ring } else { 1.0 }
        }, color);
    }

    /// Straight copy of `src` over this canvas at (dx, dy), scaled by opacity `k`.
    pub fn draw(&mut self, src: &Canvas, dx: i32, dy: i32, k: f32) {
        for sy in 0..src.h {
            let y = sy as i32 + dy;
            if y < 0 || y >= self.h as i32 {
                continue;
            }
            for sx in 0..src.w {
                let x = sx as i32 + dx;
                if x < 0 || x >= self.w as i32 {
                    continue;
                }
                let s = src.px[sy * src.w + sx];
                if s[3] == 0 {
                    continue;
                }
                self.over(x as usize, y as usize,
                          [s[0] as f32, s[1] as f32, s[2] as f32, s[3] as f32 / 255.0], k);
            }
        }
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


    /// Draw `src` mapped by screen = src·k + (tx, ty), clipped by `mask` in source coordinates.
    pub fn draw_affine(&mut self, src: &Canvas, k: f32, tx: f32, ty: f32, alpha: f32,
                       region: (f32, f32, f32, f32), mask: impl Fn(f32, f32) -> f32) {
        let (x0, y0, x1, y1) = self.bounds(region.0, region.1, region.2, region.3);
        let nearest = (k - 1.0).abs() < 1e-4 && tx.fract() == 0.0 && ty.fract() == 0.0;
        for y in y0..y1 {
            let sy = (y as f32 + 0.5 - ty) / k;
            for x in x0..x1 {
                let sx = (x as f32 + 0.5 - tx) / k;
                let m = mask(sx, sy);
                if m <= 0.0 {
                    continue;
                }
                let s = if nearest {
                    let (ix, iy) = (sx as i32, sy as i32);
                    if ix < 0 || iy < 0 || ix >= src.w as i32 || iy >= src.h as i32 {
                        continue;
                    }
                    let p = src.px[iy as usize * src.w + ix as usize];
                    [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32 / 255.0]
                } else {
                    src.sample(sx, sy)
                };
                self.over(x, y, s, alpha * m);
            }
        }
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

/// Draw text with its left edge at `x` and baseline at `baseline`.
pub fn draw_text(cv: &mut Canvas, t: &Text, x: f32, baseline: f32, color: Color) {
    let Some(fs) = fonts() else { return };
    let mut pen = x;
    let mut prev: Option<(ab_glyph::GlyphId, *const FontRef<'static>)> = None;
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
        let g = id.with_scale_and_position(scale, ab_glyph::point(pen, baseline));
        if let Some(o) = f.outline_glyph(g) {
            let b = o.px_bounds();
            let (w, h) = (cv.w as i32, cv.h as i32);
            o.draw(|gx, gy, c| {
                let x = b.min.x as i32 + gx as i32;
                let y = b.min.y as i32 + gy as i32;
                if x >= 0 && y >= 0 && x < w && y < h {
                    // a touch of gamma: grayscale AA otherwise looks thin on dark glass
                    cv.blend(x as usize, y as usize, color, c.powf(0.85));
                }
            });
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

// ------------------------------------------------------------------ blur

/// Three box blurs ≈ a Gaussian of `sigma`, on a planar f32 RGB buffer.
pub fn blur_rgb(img: &mut [[f32; 3]], w: usize, h: usize, sigma: f32) {
    if sigma <= 0.3 {
        return;
    }
    let r = ((sigma * sigma * 4.0 + 1.0).sqrt() / 2.0).round().max(1.0) as usize;
    let mut tmp = vec![[0f32; 3]; w * h];
    for _ in 0..3 {
        box_h(img, &mut tmp, w, h, r);
        box_v(&tmp, img, w, h, r);
    }
}

fn box_h(src: &[[f32; 3]], dst: &mut [[f32; 3]], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut acc = [0f32; 3];
        for i in 0..=(2 * r) {
            let p = row[(i as isize - r as isize).clamp(0, w as isize - 1) as usize];
            for c in 0..3 {
                acc[c] += p[c];
            }
        }
        for x in 0..w {
            let o = &mut dst[y * w + x];
            for c in 0..3 {
                o[c] = acc[c] * norm;
            }
            let add = row[(x + r + 1).min(w - 1)];
            let sub = row[(x as isize - r as isize).max(0) as usize];
            for c in 0..3 {
                acc[c] += add[c] - sub[c];
            }
        }
    }
}

fn box_v(src: &[[f32; 3]], dst: &mut [[f32; 3]], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for x in 0..w {
        let mut acc = [0f32; 3];
        for i in 0..=(2 * r) {
            let p = src[(i as isize - r as isize).clamp(0, h as isize - 1) as usize * w + x];
            for c in 0..3 {
                acc[c] += p[c];
            }
        }
        for y in 0..h {
            let o = &mut dst[y * w + x];
            for c in 0..3 {
                o[c] = acc[c] * norm;
            }
            let add = src[(y + r + 1).min(h - 1) * w + x];
            let sub = src[(y as isize - r as isize).max(0) as usize * w + x];
            for c in 0..3 {
                acc[c] += add[c] - sub[c];
            }
        }
    }
}

/// Blur a single-channel buffer (used for inpainting masks).
pub fn blur_gray(img: &mut [f32], w: usize, h: usize, sigma: f32) {
    let mut rgb: Vec<[f32; 3]> = img.iter().map(|&v| [v, 0.0, 0.0]).collect();
    blur_rgb(&mut rgb, w, h, sigma);
    for (o, p) in img.iter_mut().zip(rgb) {
        *o = p[0];
    }
}
