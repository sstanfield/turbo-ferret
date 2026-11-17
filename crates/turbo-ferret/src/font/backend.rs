//! The font backend interface: open a font file at a size, then ask it for metrics
//! and glyph images (port-design.md section 15).
//!
//! TB: tb_font_renderer.h (TBFontRenderer, TBFontMetrics, TBGlyphMetrics,
//! TBFontGlyphData). TB's renderer was both the factory and the opened font; here
//! [`FontBackend`] is the factory and [`GlyphSource`] the opened font.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::geom::Size;
use crate::services::{ImageDecoder, ResourceLoader};

/// What a backend may use while opening a font: the loader (for companion files such
/// as TBBF's PNG strips), the image decoder, and the path of the file being opened.
pub struct LoadCtx<'a> {
    pub loader: &'a mut dyn ResourceLoader,
    pub images: &'a mut dyn ImageDecoder,
    /// The file being loaded. Companion files resolve against it with
    /// `loader.resolve(path, rel)`. [`FontManager::create_face`](super::FontManager::create_face)
    /// resolves registered font paths against it too, so pass `""` for paths that are
    /// relative to the loader's root.
    pub path: &'a str,
}

impl<'a> LoadCtx<'a> {
    /// A context with an empty `path` (font paths are relative to the loader's root).
    pub fn new(loader: &'a mut dyn ResourceLoader, images: &'a mut dyn ImageDecoder) -> Self {
        LoadCtx { loader, images, path: "" }
    }
}

/// Opens font files. Backends are tried in order until one accepts the file.
/// TB: TBFontRenderer::Create
pub trait FontBackend {
    /// Try to open `data` (loaded from `path`; `cx.path` is the same path) at the pixel
    /// size `size`. `None` means "not my format" or "failed to load".
    fn open(&self, path: &str, data: &[u8], size: u16, cx: &mut LoadCtx) -> Option<Box<dyn GlyphSource>>;
}

/// An opened font at one size. TB: TBFontRenderer (GetMetrics, GetGlyphMetrics,
/// RenderGlyph)
pub trait GlyphSource {
    /// Metrics of the face. TB: TBFontRenderer::GetMetrics
    fn metrics(&self) -> FontMetrics;

    /// Metrics of one glyph, or `None` if the font has nothing for `c` (it then
    /// advances by 0 and draws nothing). Called for every character measured, so it
    /// should be cheap. TB: TBFontRenderer::GetGlyphMetrics
    fn glyph_metrics(&mut self, c: char) -> Option<GlyphMetrics>;

    /// Render `c` into `out` (cleared by the caller). `false` means there is nothing to
    /// draw (for example a space). The image must match the size in
    /// [`glyph_metrics`](Self::glyph_metrics). TB: TBFontRenderer::RenderGlyph
    fn rasterize(&mut self, c: char, out: &mut GlyphImage) -> bool;
}

/// Metrics of a font face, in pixels. TB: TBFontMetrics
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct FontMetrics {
    /// Distance (positive) from the baseline to the top of the tallest glyph.
    pub ascent: i32,
    /// Distance (positive) from the baseline to the bottom of the lowest glyph.
    pub descent: i32,
    /// Line height.
    pub height: i32,
}

/// Metrics of one glyph, in pixels. TB: TBGlyphMetrics (plus the bitmap size)
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct GlyphMetrics {
    /// How far the pen moves after this glyph.
    pub advance: i32,
    /// Offset of the bitmap's left edge from the pen position.
    pub x: i32,
    /// Offset of the bitmap's top edge from the baseline (negative: above it).
    pub y: i32,
    /// Bitmap width (0 if there is nothing to draw).
    pub w: i32,
    /// Bitmap height (0 if there is nothing to draw).
    pub h: i32,
}

/// Pixel layout of a [`GlyphImage`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum GlyphFormat {
    /// One byte per pixel: coverage. Becomes white with that alpha.
    #[default]
    Alpha,
    /// Four bytes per pixel: premultiplied RGBA8.
    Rgba,
}

impl GlyphFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            GlyphFormat::Alpha => 1,
            GlyphFormat::Rgba => 4,
        }
    }
}

/// A rendered glyph, filled in by [`GlyphSource::rasterize`]. TB: TBFontGlyphData
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlyphImage {
    pub size: Size,
    pub format: GlyphFormat,
    /// Row-major, tightly packed: `size.w * size.h * format.bytes_per_pixel()` bytes.
    pub data: Vec<u8>,
    /// A color glyph: drawn as it is, ignoring the text color. TB: TBFontGlyphData::rgb
    pub color: bool,
}

impl GlyphImage {
    pub fn new() -> GlyphImage {
        GlyphImage::default()
    }

    /// Reset to an empty alpha image (keeps the allocation).
    pub fn clear(&mut self) {
        self.size = Size::ZERO;
        self.format = GlyphFormat::Alpha;
        self.data.clear();
        self.color = false;
    }

    /// Bytes the image needs for its size and format.
    pub fn byte_len(&self) -> usize {
        self.size.w.max(0) as usize * self.size.h.max(0) as usize * self.format.bytes_per_pixel()
    }

    /// Write the image as premultiplied RGBA8 into `out` (replacing its contents), as
    /// the atlas stores it. Coverage `a` becomes white `(a, a, a, a)`. `false` if the
    /// image is empty or `data` is too short.
    /// TB: tb_font_renderer.cpp TBFontFace::RenderGlyph (the uint8 to uint32 conversion)
    pub fn to_premultiplied_rgba(&self, out: &mut Vec<u8>) -> bool {
        out.clear();
        let len = self.byte_len();
        if len == 0 || self.data.len() < len {
            return false;
        }
        match self.format {
            GlyphFormat::Alpha => out.extend(self.data[..len].iter().flat_map(|&a| [a, a, a, a])),
            GlyphFormat::Rgba => out.extend_from_slice(&self.data[..len]),
        }
        true
    }
}
