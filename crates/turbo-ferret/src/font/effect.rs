//! An effect applied to each glyph as it is rendered into the glyph cache: a
//! Gaussian blur, for glows and soft shadows (a blurred face drawn behind another
//! as its background font).
//!
//! TB: tb_font_renderer.cpp (TBFontEffect, blurGlyph)

use alloc::vec::Vec;

use crate::geom::Size;
use crate::math::{exp, sqrt};

use super::backend::{GlyphFormat, GlyphImage, GlyphMetrics};

/// The largest blur radius, in pixels.
pub const MAX_BLUR_RADIUS: u8 = 64;

/// The effect of one face. TB: TBFontEffect
#[derive(Clone, Debug, Default)]
pub struct FontEffect {
    blur_radius: usize,
    /// `2 * blur_radius + 1` weights that add up to 1.
    kernel: Vec<f32>,
    /// Scratch: the image after the horizontal pass, and the result.
    temp: Vec<f32>,
    out: Vec<u8>,
}

impl FontEffect {
    /// The blur radius in pixels (0: no effect). TB: TBFontEffect::SetBlurRadius
    pub fn blur_radius(&self) -> u8 {
        self.blur_radius as u8
    }

    /// Blur every glyph by `radius` pixels (0 turns the effect off). A glyph grows
    /// by the radius on every side; its advance stays. Radii above
    /// [`MAX_BLUR_RADIUS`] are clamped. TB: TBFontEffect::SetBlurRadius
    pub fn set_blur_radius(&mut self, radius: u8) {
        let radius = usize::from(radius.min(MAX_BLUR_RADIUS));
        if radius == self.blur_radius {
            return;
        }
        self.blur_radius = radius;
        self.kernel.clear();
        if radius > 0 {
            // A Gaussian with a standard deviation of half the radius.
            let dev = radius as f32 / 2.0;
            let two_dev_sq = 2.0 * dev * dev;
            let scale = 1.0 / sqrt(core::f32::consts::PI * two_dev_sq);
            self.kernel.extend((0..2 * radius + 1).map(|k| {
                let x = k as f32 - radius as f32;
                scale * exp(-(x * x / two_dev_sq))
            }));
            let sum: f32 = self.kernel.iter().sum();
            for k in &mut self.kernel {
                *k /= sum;
            }
        }
    }

    /// Apply the effect to a rendered glyph and adjust its metrics. Only coverage
    /// (alpha) glyphs are blurred; color glyphs are left alone, as in TB.
    /// TB: TBFontEffect::Render
    pub(crate) fn apply(&mut self, metrics: &mut GlyphMetrics, image: &mut GlyphImage) {
        let r = self.blur_radius;
        if r == 0 || image.format != GlyphFormat::Alpha || image.color {
            return;
        }
        let (sw, sh) = (image.size.w.max(0) as usize, image.size.h.max(0) as usize);
        if sw == 0 || sh == 0 || image.data.len() < sw * sh {
            return;
        }
        let (dw, dh) = (sw + 2 * r, sh + 2 * r);

        // Horizontal pass: source rows into rows as wide as the result.
        // TB: blurGlyph
        self.temp.clear();
        self.temp.resize(dw * sh, 0.0);
        for y in 0..sh {
            let src = &image.data[y * sw..(y + 1) * sw];
            for x in 0..dw {
                // Result column x is source column x - r; the kernel spans r either way.
                let mut val = 0.0;
                for (k, weight) in self.kernel.iter().enumerate() {
                    // sx = x - r + (k - r)
                    if let Some(sx) = (x + k).checked_sub(2 * r) {
                        if sx < sw {
                            val += f32::from(src[sx]) * weight;
                        }
                    }
                }
                self.temp[y * dw + x] = val;
            }
        }
        // Vertical pass.
        self.out.clear();
        self.out.resize(dw * dh, 0);
        for y in 0..dh {
            for x in 0..dw {
                let mut val = 0.0;
                for (k, weight) in self.kernel.iter().enumerate() {
                    if let Some(sy) = (y + k).checked_sub(2 * r) {
                        if sy < sh {
                            val += self.temp[sy * dw + x] * weight;
                        }
                    }
                }
                self.out[y * dw + x] = (val + 0.5) as u8;
            }
        }

        core::mem::swap(&mut image.data, &mut self.out);
        image.size = Size::new(dw as i32, dh as i32);
        // Adjust glyph position to compensate for larger size.
        metrics.x -= r as i32;
        metrics.y -= r as i32;
        metrics.w = dw as i32;
        metrics.h = dh as i32;
    }
}
