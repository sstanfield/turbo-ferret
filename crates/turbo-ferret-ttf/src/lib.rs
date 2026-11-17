//! TrueType fonts for Turbo Ferret: a parser, an anti-aliased rasterizer and a
//! [`FontBackend`] that puts them behind the font manager.
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author.
//!
//! # Using it
//!
//! ```ignore
//! ui.fonts_mut().add_backend(Box::new(turbo_ferret_ttf::TtfBackend::new()));
//! ui.add_font("Vera", "vera.ttf");
//! ui.create_font_face(FontDesc::new(tbid!("Vera"), 14));
//! ```
//!
//! Glyphs are rasterized on first use into the core's glyph cache, like any other
//! font. An app that must not carry this crate can rasterize its fonts ahead of time
//! with the `turbo-ferret-fontgen` tool and ship the bitmaps.
//!
//! # What is supported
//!
//! - TrueType outlines (`glyf`): simple and composite glyphs. Fonts with PostScript
//!   outlines (`.otf` with a `CFF ` table) are refused.
//! - Character maps of formats 4 and 12 (and 0 and 6) in a Unicode encoding.
//! - The first font of a `.ttc` collection.
//! - No hinting, no kerning, no shaping, no variable-font axes, no color glyphs.
//!   Turbo Badger's TrueType backends did not kern or shape either, and its
//!   stb_truetype backend did not hint.
//!
//! # Size
//!
//! `FontDesc::size` is the em size in pixels, as in CSS and FreeType (and TB's
//! FreeType backend). TB's stb_truetype backend instead scaled the ascent plus
//! descent to 1.3 times the size.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod font;
mod raster;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use turbo_ferret::font::{FontBackend, FontMetrics, GlyphFormat, GlyphImage, GlyphMetrics, GlyphSource, LoadCtx};
use turbo_ferret::geom::Size;
use turbo_ferret::math::{ceil, floor, round_i32};

pub use font::{Bounds, Font, OutlineSink};
pub use raster::Raster;

/// The largest glyph bitmap that is rasterized, in pixels per side.
const MAX_GLYPH_SIDE: i32 = 2048;

/// The TrueType [`FontBackend`]. Accepts files that start with a TrueType header,
/// whatever their name. A file opened at several sizes is parsed once.
/// TB: tb_font_renderer_stb.cpp register_stb_font_renderer
#[derive(Default)]
pub struct TtfBackend {
    /// Fonts opened so far, by path.
    fonts: RefCell<Vec<(String, Rc<Font>)>>,
}

impl TtfBackend {
    pub fn new() -> TtfBackend {
        TtfBackend::default()
    }
}

impl FontBackend for TtfBackend {
    // TB: tb_font_renderer_stb.cpp STBFontRenderer::Create
    fn open(&self, path: &str, data: &[u8], size: u16, _cx: &mut LoadCtx) -> Option<Box<dyn GlyphSource>> {
        let mut fonts = self.fonts.try_borrow_mut().ok()?;
        // The same file again (another size): share the parsed font.
        let known = fonts.iter().position(|(p, f)| p == path && f.data() == data);
        let font = match known {
            Some(i) => fonts[i].1.clone(),
            None => {
                let font = Rc::new(Font::parse(data.to_vec())?);
                fonts.retain(|(p, _)| p != path);
                fonts.push((String::from(path), font.clone()));
                font
            }
        };
        Some(Box::new(TtfFace::new(font, size)))
    }
}

/// A TrueType font at one pixel size. TB: STBFontRenderer
pub struct TtfFace {
    font: Rc<Font>,
    /// Pixels per font unit.
    scale: f32,
    metrics: FontMetrics,
    /// Glyph index and metrics of the characters asked for so far.
    glyphs: BTreeMap<char, (u16, GlyphMetrics)>,
    raster: Raster,
}

impl TtfFace {
    /// `size` is the em size in pixels.
    pub fn new(font: Rc<Font>, size: u16) -> TtfFace {
        let scale = f32::from(size) / f32::from(font.units_per_em());
        let (asc, desc, gap) = (f32::from(font.ascent()), f32::from(font.descent()), f32::from(font.line_gap()));
        let ascent = round_i32(asc * scale);
        let descent = round_i32(-desc * scale);
        // TB: STBFontRenderer::GetMetrics
        let height = round_i32((asc - desc + gap) * scale).max(ascent + descent);
        TtfFace {
            font,
            scale,
            metrics: FontMetrics { ascent, descent, height },
            glyphs: BTreeMap::new(),
            raster: Raster::default(),
        }
    }

    pub fn font(&self) -> &Font {
        &self.font
    }

    /// True if the font has a glyph of its own for `c` (otherwise its "missing
    /// glyph" is drawn).
    pub fn has_glyph(&self, c: char) -> bool {
        self.font.glyph_index(c).is_some()
    }

    /// The glyph and metrics for `c`. A character the font lacks gets glyph 0, the
    /// font's "missing glyph" (usually a box), as in TB.
    fn lookup(&mut self, c: char) -> (u16, GlyphMetrics) {
        if let Some(g) = self.glyphs.get(&c) {
            return *g;
        }
        let glyph = self.font.glyph_index(c).unwrap_or(0);
        let mut m = GlyphMetrics {
            advance: round_i32(f32::from(self.font.advance_width(glyph)) * self.scale),
            ..Default::default()
        };
        if let Some(b) = self.font.glyph_bounds(glyph) {
            // The pixel box around the scaled outline; y flips (font units go up).
            // TB: stbtt_GetCodepointBitmapBox
            let s = self.scale;
            let x0 = floor(f32::from(b.x_min) * s) as i32;
            let y0 = floor(-f32::from(b.y_max) * s) as i32;
            let x1 = ceil(f32::from(b.x_max) * s) as i32;
            let y1 = ceil(-f32::from(b.y_min) * s) as i32;
            if x1 > x0 && y1 > y0 && x1 - x0 <= MAX_GLYPH_SIDE && y1 - y0 <= MAX_GLYPH_SIDE {
                m.x = x0;
                m.y = y0;
                m.w = x1 - x0;
                m.h = y1 - y0;
            }
        }
        self.glyphs.insert(c, (glyph, m));
        (glyph, m)
    }
}

/// Scales an outline to pixels and feeds the rasterizer.
struct PixelSink<'a> {
    raster: &'a mut Raster,
    scale: f32,
    /// The bitmap's top-left corner, in pixels.
    x0: f32,
    y0: f32,
}

impl PixelSink<'_> {
    fn px(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.scale - self.x0, -y * self.scale - self.y0)
    }
}

impl OutlineSink for PixelSink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.px(x, y);
        self.raster.move_to(p.0, p.1);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.px(x, y);
        self.raster.line_to(p.0, p.1);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let c = self.px(cx, cy);
        let p = self.px(x, y);
        self.raster.quad_to(c.0, c.1, p.0, p.1);
    }
    fn close(&mut self) {
        self.raster.close();
    }
}

impl GlyphSource for TtfFace {
    fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    // TB: STBFontRenderer::GetGlyphMetrics
    fn glyph_metrics(&mut self, c: char) -> Option<GlyphMetrics> {
        Some(self.lookup(c).1)
    }

    // TB: STBFontRenderer::RenderGlyph
    fn rasterize(&mut self, c: char, out: &mut GlyphImage) -> bool {
        let (glyph, m) = self.lookup(c);
        if m.w <= 0 || m.h <= 0 {
            return false;
        }
        self.raster.reset(m.w as usize, m.h as usize);
        let mut sink = PixelSink { raster: &mut self.raster, scale: self.scale, x0: m.x as f32, y0: m.y as f32 };
        self.font.outline(glyph, &mut sink);
        out.size = Size::new(m.w, m.h);
        out.format = GlyphFormat::Alpha;
        out.color = false;
        self.raster.coverage(&mut out.data);
        true
    }
}
