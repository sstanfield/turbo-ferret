//! An anti-aliased rasterizer for glyph outlines, by signed area accumulation.
//!
//! Each edge is cut into the pieces that lie in one pixel. A piece of height `h`
//! (signed by the edge's direction) covers `h` of every pixel to its right in that
//! row, and of its own pixel the part right of the piece. So a piece adds that part
//! to its own cell and the rest of `h` to the next cell, and a running sum along a
//! row then gives every pixel's coverage. That is exact for one contour. Where
//! contours overlap the sum is clamped to full coverage (the non-zero winding rule,
//! except in the anti-aliased pixels of an overlap).
//!
//! This is the general technique behind the anti-aliased rasterizers of FreeType,
//! stb_truetype and font-rs. The code is written for this crate from the geometry.

use alloc::vec;
use alloc::vec::Vec;

use turbo_ferret::math::{abs, ceil, sqrt};

/// How far (in pixels) the lines drawn for a curve may be from it.
const CURVE_TOLERANCE: f32 = 0.02;

/// Most lines one curve is cut into.
const MAX_CURVE_STEPS: u32 = 64;

/// Accumulates edges and produces a coverage bitmap.
#[derive(Clone, Debug, Default)]
pub struct Raster {
    w: usize,
    h: usize,
    /// `h` rows of `w + 2` cells: an edge at the right border writes one past it.
    acc: Vec<f32>,
    /// The start of the contour and the pen position.
    start: (f32, f32),
    pen: (f32, f32),
}

impl Raster {
    pub fn new(w: usize, h: usize) -> Raster {
        let mut r = Raster::default();
        r.reset(w, h);
        r
    }

    /// Start over with a new size (keeps the allocation).
    pub fn reset(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        self.acc.clear();
        self.acc.resize((w + 2) * h, 0.0);
        self.start = (0.0, 0.0);
        self.pen = (0.0, 0.0);
    }

    pub fn width(&self) -> usize {
        self.w
    }

    pub fn height(&self) -> usize {
        self.h
    }

    /// Begin a contour at a point (in pixels, y down).
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.start = (x, y);
        self.pen = (x, y);
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.edge(self.pen, (x, y));
        self.pen = (x, y);
    }

    /// A quadratic Bézier from the pen through the control point `c` to `(x, y)`,
    /// drawn as lines: as many as keep the error well under a pixel.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let p0 = self.pen;
        // A quadratic leaves its chord by at most |p0 - 2c + p1| / 4, and cut into n
        // equal parameter steps each piece leaves its chord by that over n squared.
        // Pick n so that this stays under a tenth of a pixel.
        let (ddx, ddy) = (p0.0 - 2.0 * cx + x, p0.1 - 2.0 * cy + y);
        let bend = sqrt(ddx * ddx + ddy * ddy) * 0.25;
        let n = if bend.is_finite() && bend > CURVE_TOLERANCE {
            (ceil(sqrt(bend / CURVE_TOLERANCE)) as u32).clamp(2, MAX_CURVE_STEPS)
        } else {
            1
        };
        let mut prev = p0;
        for i in 1..n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let p = (u * u * p0.0 + 2.0 * u * t * cx + t * t * x, u * u * p0.1 + 2.0 * u * t * cy + t * t * y);
            self.edge(prev, p);
            prev = p;
        }
        self.edge(prev, (x, y));
        self.pen = (x, y);
    }

    /// Close the contour with a line back to where it began.
    pub fn close(&mut self) {
        if self.pen != self.start {
            self.edge(self.pen, self.start);
            self.pen = self.start;
        }
    }

    /// Add one edge. Parts outside the bitmap are clipped: what is above or below
    /// is dropped, and what is left or right of it counts at the border.
    fn edge(&mut self, a: (f32, f32), b: (f32, f32)) {
        if !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) {
            return;
        }
        // A horizontal edge covers nothing.
        if a.1 == b.1 {
            return;
        }
        // Walk it downwards; its direction is the sign of what it adds.
        let (dir, top, bottom) = if a.1 < b.1 { (1.0, a, b) } else { (-1.0, b, a) };
        let slope = (bottom.0 - top.0) / (bottom.1 - top.1);
        let wf = self.w as f32;
        let stride = self.w + 2;
        let row_first = top.1.max(0.0) as usize;
        let row_end = (ceil(bottom.1).max(0.0) as usize).min(self.h);
        for row in row_first..row_end {
            // The part of the edge in this row.
            let ya = (row as f32).max(top.1);
            let yb = (row as f32 + 1.0).min(bottom.1);
            if yb <= ya {
                continue;
            }
            let xa = top.0 + (ya - top.1) * slope;
            let xb = top.0 + (yb - top.1) * slope;
            let cover = (yb - ya) * dir;
            let (xl, xr) = if xa < xb { (xa, xb) } else { (xb, xa) };
            let line = &mut self.acc[row * stride..(row + 1) * stride];
            if xr - xl < 1e-6 {
                // Upright: one piece in one cell (or at a border, if it is outside).
                let x = xl.max(0.0).min(wf);
                let col = x as usize;
                let m = x - col as f32;
                line[col] += cover * (1.0 - m);
                line[col + 1] += cover * m;
                continue;
            }
            // A piece's share of the height is its share of the width.
            let per_x = cover / (xr - xl);
            // What is left of the bitmap covers every pixel of the row: it all goes
            // to the first cell. What is right of it covers none: it goes to the
            // cell behind the last one, which is never read.
            if xl < 0.0 {
                line[0] += (xr.min(0.0) - xl) * per_x;
            }
            if xr > wf {
                line[self.w] += (xr - xl.max(wf)) * per_x;
            }
            // The part inside, cut at the pixel borders.
            let (xl, xr) = (xl.max(0.0), xr.min(wf));
            let mut x = xl;
            let mut col = xl as usize;
            while x < xr && col < self.w {
                let x_end = (col as f32 + 1.0).min(xr);
                let h = (x_end - x) * per_x;
                // Where the piece's middle is in its cell: that much of the cell is
                // left of it.
                let m = (x + x_end) * 0.5 - col as f32;
                line[col] += h * (1.0 - m);
                line[col + 1] += h * m;
                x = x_end;
                col += 1;
            }
        }
    }

    /// Write the coverage (0 to 255 per pixel, row by row) into `out`, replacing
    /// its contents.
    pub fn coverage(&self, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(self.w * self.h);
        let stride = self.w + 2;
        for row in 0..self.h {
            let mut sum = 0.0f32;
            for &a in &self.acc[row * stride..row * stride + self.w] {
                sum += a;
                out.push((abs(sum).min(1.0) * 255.0 + 0.5) as u8);
            }
        }
    }

    /// The coverage as a new buffer.
    pub fn to_coverage(&self) -> Vec<u8> {
        let mut out = vec![];
        self.coverage(&mut out);
        out
    }
}
