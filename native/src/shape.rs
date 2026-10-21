//! Shapes as signed distance fields, filled on all cores, plus the soft drop shadow.

use crate::gfx::{Canvas, Color};

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

/// Soft drop shadow outside the shape, composited under `shape`.
pub fn with_shadow(shape: &Canvas, field: &Field, spread: f32, opacity: f32, offset: (i32, i32)) -> Canvas {
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
            // shape over shadow
            let g = shape.px[y * w + x];
            let ga = g[3] as f32 / 255.0;
            let sa = a * (1.0 - ga);
            *o = [g[0], g[1], g[2], ((ga + sa) * 255.0).min(255.0) as u8];
        }
    });
    out
}

/// Paint the shape flat: `color(x, y)` inside, blended into a hairline `edge` of width `edge_w`
/// along the rim, anti-aliased from the distance field.
pub fn fill(field: &Field, color: impl Fn(f32, f32) -> Color + Sync, edge: Color, edge_w: f32) -> Canvas {
    let w = field.w;
    let mut out = Canvas::new(w, field.h);
    par_rows(&mut out.px, w, |y, row| {
        for (x, o) in row.iter_mut().enumerate() {
            let d = field.d[y * w + x];
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let base = color(x as f32 + 0.5, y as f32 + 0.5);
            let rim = (1.0 - (-d - edge_w + 0.5).clamp(0.0, 1.0)) * edge.a;
            let c = crate::theme::mix(base, edge.with_a(1.0), rim);
            let a = cov * c.a;
            *o = [(c.b * a * 255.0 + 0.5) as u8, (c.g * a * 255.0 + 0.5) as u8, (c.r * a * 255.0 + 0.5) as u8,
                  (a * 255.0 + 0.5) as u8];
        }
    });
    out
}
