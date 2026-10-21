//! Liquid-glass renderer (port of liquid.py, per pixel and multi-threaded).
//!
//! The look comes from refracting a snapshot of what is behind the surface:
//!   * lensing in a thin bezel at the edge, the middle stays clear
//!   * chromatic dispersion in that bezel (R, G, B bend differently): rainbow fringe
//!   * a specular rim, brightest on the side facing the light
//!   * a light blur + saturation boost and a tint that adapts to the backdrop

use crate::gfx::{blur_rgb, Canvas};

#[derive(Clone, Copy)]
pub struct Style {
    pub bezel: f32,
    pub strength: f32,
    pub dispersion: f32,
    pub blur: f32,
    pub saturation: f32,
    pub spec: f32,
    pub light: (f32, f32),
}

impl Default for Style {
    fn default() -> Self {
        Style { bezel: 22.0, strength: 16.0, dispersion: 0.32, blur: 2.2, saturation: 1.45, spec: 0.95,
                light: (-0.55, -0.83) }
    }
}

impl Style {
    pub fn scaled(mut self, s: f32) -> Style {
        self.bezel *= s;
        self.strength *= s;
        self.blur *= s;
        self
    }
}

/// RGB snapshot of the screen behind a surface.
pub struct Backdrop {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 3]>,
}

/// Signed distance field (negative inside).
pub struct Field {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl Field {
    pub fn from_fn(w: usize, h: usize, f: impl Fn(f32, f32) -> f32 + Sync) -> Field {
        let mut d = vec![0f32; w * h];
        par_rows(&mut d, w, |y, row| {
            let fy = y as f32 + 0.5;
            for (x, v) in row.iter_mut().enumerate() {
                *v = f(x as f32 + 0.5, fy);
            }
        });
        Field { w, h, d }
    }
}

/// Run `f(y, row)` over rows on all cores.
pub fn par_rows<T: Send>(buf: &mut [T], w: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let h = buf.len() / w.max(1);
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
    let per = h.div_ceil(n).max(1);
    std::thread::scope(|s| {
        for (ci, chunk) in buf.chunks_mut(per * w).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (ri, row) in chunk.chunks_mut(w).enumerate() {
                    f(ci * per + ri, row);
                }
            });
        }
    });
}

fn bilinear(src: &[[f32; 3]], w: usize, h: usize, x: f32, y: f32, ch: usize) -> f32 {
    let x = x.clamp(0.0, w as f32 - 1.001);
    let y = y.clamp(0.0, h as f32 - 1.001);
    let (x0, y0) = (x as usize, y as usize);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let a = src[y0 * w + x0][ch];
    let b = src[y0 * w + x0 + 1][ch];
    let c = src[(y0 + 1) * w + x0][ch];
    let d = src[(y0 + 1) * w + x0 + 1][ch];
    a * (1.0 - fx) * (1.0 - fy) + b * fx * (1.0 - fy) + c * (1.0 - fx) * fy + d * fx * fy
}

pub struct Glass {
    pub canvas: Canvas,
    pub light: bool,
}

/// Render the glass for `field` over `bg` (same size). Returns a premultiplied canvas.
pub fn render(bg: &Backdrop, field: &Field, style: &Style, tint: Option<(f32, f32, f32, f32)>) -> Glass {
    let (w, h) = (field.w, field.h);
    debug_assert!(bg.w == w && bg.h == h);
    let raw: Vec<[f32; 3]> = bg.px.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();

    // is the backdrop light? (weighted by coverage)
    let (mut lsum, mut asum) = (0f64, 0f64);
    for (i, p) in raw.iter().enumerate() {
        let a = (0.5 - field.d[i]).clamp(0.0, 1.0) as f64;
        if a > 0.0 {
            lsum += a * ((0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) / 255.0) as f64;
            asum += a;
        }
    }
    let light = asum > 0.0 && lsum / asum > 0.62;

    let mut soft = raw;
    blur_rgb(&mut soft, w, h, style.blur);
    let sat = style.saturation;
    for p in soft.iter_mut() {
        let l = 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
        for c in p.iter_mut() {
            *c = l + (*c - l) * sat;
        }
    }

    let tint = tint.unwrap_or(if light { (236.0, 232.0, 246.0, 0.14) } else { (18.0, 14.0, 30.0, 0.26) });
    let (lx, ly) = style.light;
    let d = &field.d;
    let mut out = Canvas::new(w, h);
    par_rows(&mut out.px, w, |y, row| {
        for (x, o) in row.iter_mut().enumerate() {
            let i = y * w + x;
            let sdf = d[i];
            let alpha = (0.5 - sdf).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            // central-difference gradient = outward normal
            let gx = (d[y * w + (x + 1).min(w - 1)] - d[y * w + x.saturating_sub(1)]) * 0.5;
            let gy = (d[(y + 1).min(h - 1) * w + x] - d[y.saturating_sub(1) * w + x]) * 0.5;
            let n = gx.hypot(gy) + 1e-6;
            let (nx, ny) = (gx / n, gy / n);
            let depth = -sdf;
            let t = (1.0 - depth / style.bezel).clamp(0.0, 1.0);
            let profile = 1.0 - (1.0 - t * t).clamp(0.0, 1.0).sqrt();
            let mut c = soft[i];
            if profile > 0.0 {
                for (ch, k) in [1.0 - style.dispersion, 1.0, 1.0 + style.dispersion].into_iter().enumerate() {
                    let mag = style.strength * profile * k;
                    c[ch] = bilinear(&soft, w, h, x as f32 - nx * mag, y as f32 - ny * mag, ch);
                }
            }
            for (ch, tv) in [tint.0, tint.1, tint.2].into_iter().enumerate() {
                c[ch] = c[ch] * (1.0 - tint.3) + tv * tint.3;
            }
            if light {
                let p16 = profile.powf(1.6);
                c[0] -= p16 * 62.0;
                c[1] -= p16 * 60.0;
                c[2] -= p16 * 54.0;
                let edge = (1.0 - depth / 1.6).clamp(0.0, 1.0) * 40.0;
                for v in c.iter_mut() {
                    *v -= edge;
                }
            } else {
                let g = profile * profile * 38.0;
                for v in c.iter_mut() {
                    *v += g;
                }
            }
            let dot = nx * lx + ny * ly;
            let facing = dot.clamp(0.0, 1.0);
            let back = (-dot).clamp(0.0, 1.0);
            let rim = (1.0 - depth / 2.2).clamp(0.0, 1.0);
            let rim2 = (1.0 - depth / 7.0).clamp(0.0, 1.0);
            let spec = rim * (0.35 + 0.65 * facing.powf(1.4)) + rim2 * facing.powi(3) * 0.35 + rim * back * back * 0.45;
            let add = spec * 255.0 * style.spec;
            // premultiply into BGRA
            let r = (c[0] + add).clamp(0.0, 255.0) * alpha;
            let g = (c[1] + add).clamp(0.0, 255.0) * alpha;
            let b = (c[2] + add).clamp(0.0, 255.0) * alpha;
            *o = [b as u8, g as u8, r as u8, (alpha * 255.0) as u8];
        }
    });
    Glass { canvas: out, light }
}

/// Soft drop shadow outside the shape, composited under `glass`.
pub fn with_shadow(glass: &Canvas, field: &Field, spread: f32, opacity: f32, offset: (i32, i32)) -> Canvas {
    let (w, h) = (field.w, field.h);
    let mut out = Canvas::new(w, h);
    par_rows(&mut out.px, w, |y, row| {
        for (x, o) in row.iter_mut().enumerate() {
            let sx = x as i32 - offset.0;
            let sy = y as i32 - offset.1;
            let d = if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                field.d[sy as usize * w + sx as usize]
            } else {
                1e3
            };
            let mut a = 0.0;
            if field.d[y * w + x] >= 0.0 {
                let k = (1.0 - d.max(0.0) / spread).clamp(0.0, 1.0);
                a = k * k * opacity;
            }
            // glass over shadow
            let g = glass.px[y * w + x];
            let ga = g[3] as f32 / 255.0;
            let sa = a * (1.0 - ga);
            *o = [g[0], g[1], g[2], ((ga + sa) * 255.0).min(255.0) as u8];
        }
    });
    out
}
