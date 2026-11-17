//! TBBF, Turbo Badger's bitmap font: a `.tb.txt` description plus one PNG strip per
//! size. Glyph boundaries are found from fully transparent columns in the strip.
//!
//! Description format (TB resource syntax; children are indented with tabs):
//!
//! ```text
//! info
//!     glyph_str !"#$%...      the characters in the strip, in order (UTF-8)
//!     rgb 0                   1: color font, drawn ignoring the text color
//! size 14                     one node per available size
//!     bitmap font_14.png      the strip, relative to the description file
//!     ascent 15
//!     descent 5
//!     x_ofs 0                 x offset of every glyph (for glows)
//!     advance_delta -3        added to every glyph's width (for baked shadows)
//!     space_advance 5         advance of ' '
//! ```
//!
//! Every glyph must touch a pixel with alpha > 0 in each column from its left to its
//! right edge (a `"` needs something between its two marks), and glyphs are
//! separated by at least one clear column.
//!
//! TB: tb_font_renderer_tbbf.cpp (TBBFRenderer)

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::geom::Size;
use crate::res::{self, ParseCtx, ResNode};

use super::backend::{FontBackend, FontMetrics, GlyphFormat, GlyphImage, GlyphMetrics, GlyphSource, LoadCtx};

/// The TBBF [`FontBackend`]. Accepts paths containing `.tb.txt`.
/// TB: tb_font_renderer_tbbf.cpp register_tbbf_font_renderer
#[derive(Copy, Clone, Debug, Default)]
pub struct TbbfBackend;

impl FontBackend for TbbfBackend {
    // TB: tb_font_renderer_tbbf.cpp TBBFRenderer::Create
    fn open(&self, path: &str, data: &[u8], size: u16, cx: &mut LoadCtx) -> Option<Box<dyn GlyphSource>> {
        if !path.contains(".tb.txt") {
            return None;
        }
        let font = TbbfFont::load(path, data, size, cx)?;
        Some(Box::new(font))
    }
}

/// Columns of one glyph in the strip. TB: GLYPH
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Column {
    x: i32,
    w: i32,
}

/// A TBBF font loaded at one size (one strip). TB: TBBFRenderer
pub struct TbbfFont {
    size: u16,
    metrics: FontMetrics,
    x_ofs: i32,
    advance_delta: i32,
    space_advance: i32,
    rgb: bool,
    strip_size: Size,
    /// The strip, premultiplied RGBA8.
    pixels: Vec<u8>,
    glyphs: BTreeMap<char, Column>,
}

impl core::fmt::Debug for TbbfFont {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TbbfFont")
            .field("size", &self.size)
            .field("metrics", &self.metrics)
            .field("strip_size", &self.strip_size)
            .field("glyphs", &self.glyphs.len())
            .finish_non_exhaustive()
    }
}

impl TbbfFont {
    /// Parse the description `data` (loaded from `path`), pick the size node closest
    /// to `size`, then load and decode its strip (relative to `path`) and find the
    /// glyphs. `None` if the description does not parse, has no size node or no
    /// `info>glyph_str`, or the strip cannot be loaded or decoded.
    ///
    /// There is no scaling: a size with no strip of its own uses the nearest one, and
    /// its metrics (on a tie, the first in the file wins).
    /// TB: tb_font_renderer_tbbf.cpp TBBFRenderer::Load
    pub fn load(path: &str, data: &[u8], size: u16, cx: &mut LoadCtx) -> Option<TbbfFont> {
        let src = core::str::from_utf8(data).ok()?;
        let node = res::parse(src, &mut ParseCtx::new(path).with_loader(&mut *cx.loader)).ok()?;

        // Check for size nodes and get the one closest to the size we want.
        let dist = |n: &ResNode| (i64::from(size) - i64::from(n.value.as_int())).abs();
        let mut size_node: Option<&ResNode> = None;
        for n in node.iter_named("size") {
            if size_node.is_none_or(|best| dist(n) < dist(best)) {
                size_node = Some(n);
            }
        }
        let size_node = size_node?;

        // Metrics
        let ascent = size_node.int("ascent", 0);
        let descent = size_node.int("descent", 0);
        let metrics = FontMetrics { ascent, descent, height: ascent.saturating_add(descent) };

        // The bitmap for the size, in the same folder as the description.
        let bitmap = size_node.str("bitmap", "");
        let bitmap_path = cx.loader.resolve(path, &bitmap);
        let bytes = cx.loader.load(&bitmap_path).ok()?;
        let image = cx.images.decode(&bytes).ok()?;
        let (w, h) = (image.size.w.max(0) as usize, image.size.h.max(0) as usize);
        if image.rgba.len() < w * h * 4 {
            return None;
        }

        // TB: TBBFRenderer::FindGlyphs (fails without a glyph string)
        let glyph_str = node.value_at("info>glyph_str")?.as_str();
        let glyphs = find_glyphs(&glyph_str, image.size, &image.rgba);

        // Premultiply once; glyphs are copied out of the strip as they are.
        let mut pixels = image.rgba;
        pixels.truncate(w * h * 4);
        for px in pixels.chunks_exact_mut(4) {
            let a = u32::from(px[3]);
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * a + 127) / 255) as u8;
            }
        }

        Some(TbbfFont {
            size: u16::try_from(size_node.value.as_int()).unwrap_or(size),
            metrics,
            x_ofs: size_node.int("x_ofs", 0),
            advance_delta: size_node.int("advance_delta", 0),
            space_advance: size_node.int("space_advance", 0),
            rgb: node.int("info>rgb", 0) != 0,
            strip_size: image.size,
            pixels,
            glyphs,
        })
    }

    /// The size of the strip in use (the value of its `size` node).
    pub fn size(&self) -> u16 {
        self.size
    }

    /// Size of the strip image.
    pub fn strip_size(&self) -> Size {
        self.strip_size
    }

    /// Number of glyphs found in the strip.
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// True if `c` was found in the strip (others draw as `?`).
    pub fn has_glyph(&self, c: char) -> bool {
        self.glyphs.contains_key(&c)
    }

    /// Left column and width of `c` in the strip, if it was found (no `?` fallback).
    pub fn glyph_columns(&self, c: char) -> Option<(i32, i32)> {
        self.glyphs.get(&c).map(|g| (g.x, g.w))
    }

    /// True for color fonts (`info>rgb 1`), drawn ignoring the text color.
    pub fn is_color(&self) -> bool {
        self.rgb
    }

    pub fn x_ofs(&self) -> i32 {
        self.x_ofs
    }

    pub fn advance_delta(&self) -> i32 {
        self.advance_delta
    }

    pub fn space_advance(&self) -> i32 {
        self.space_advance
    }

    /// The glyph for `c`, or `?` for characters the strip lacks.
    fn lookup(&self, c: char) -> Option<Column> {
        self.glyphs.get(&c).or_else(|| self.glyphs.get(&'?')).copied()
    }
}

impl GlyphSource for TbbfFont {
    // TB: TBBFRenderer::GetMetrics
    fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    // TB: tb_font_renderer_tbbf.cpp TBBFRenderer::GetGlyphMetrics
    fn glyph_metrics(&mut self, c: char) -> Option<GlyphMetrics> {
        let (advance, w, h) = if c == ' ' {
            (self.space_advance, 0, 0)
        } else {
            let g = self.lookup(c)?;
            (g.w.saturating_add(self.advance_delta), g.w, self.strip_size.h)
        };
        Some(GlyphMetrics { advance, x: self.x_ofs, y: -self.metrics.ascent, w, h })
    }

    // TB: tb_font_renderer_tbbf.cpp TBBFRenderer::RenderGlyph
    fn rasterize(&mut self, c: char, out: &mut GlyphImage) -> bool {
        if c == ' ' {
            return false;
        }
        let Some(g) = self.lookup(c) else {
            return false;
        };
        let stride = self.strip_size.w as usize * 4;
        let (x, w) = (g.x as usize * 4, g.w as usize * 4);
        out.size = Size::new(g.w, self.strip_size.h);
        out.format = GlyphFormat::Rgba;
        out.color = self.rgb;
        out.data.clear();
        for row in self.pixels.chunks_exact(stride) {
            out.data.extend_from_slice(&row[x..x + w]);
        }
        true
    }
}

/// Find each character of `glyph_str` in the strip, left to right, stopping at the
/// first one that cannot be found. `rgba` is straight or premultiplied (only alpha is
/// read). TB: tb_font_renderer_tbbf.cpp TBBFRenderer::FindGlyphs
fn find_glyphs(glyph_str: &str, size: Size, rgba: &[u8]) -> BTreeMap<char, Column> {
    let (w, h) = (size.w.max(0) as usize, size.h.max(0) as usize);
    // TB scans each column for every glyph; a column is the same every time, so scan once.
    let clear: Vec<bool> = (0..w).map(|x| (0..h).all(|y| rgba[(y * w + x) * 4 + 3] == 0)).collect();
    let mut glyphs = BTreeMap::new();
    let mut x = 0;
    for c in glyph_str.chars() {
        if c == '\0' {
            break; // TB: decode_next returns 0 at the end of the string
        }
        let Some(g) = find_next(&clear, x) else {
            break;
        };
        glyphs.insert(c, g);
        x = (g.x + g.w + 1) as usize;
    }
    glyphs
}

/// The next glyph at or after column `x`: from the first column with any alpha to
/// the next fully clear one. `None` if there is none, or it runs to the right edge.
/// TB: tb_font_renderer_tbbf.cpp TBBFRenderer::FindNext
fn find_next(clear: &[bool], x: usize) -> Option<Column> {
    let left = x + clear.get(x..)?.iter().position(|&c| !c)?;
    let w = clear[left..].iter().position(|&c| c)?;
    Some(Column { x: left as i32, w: w as i32 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{DecodeError, Image, ImageDecoder, MemLoader, ResourceLoader};
    use alloc::vec;

    /// Decodes anything into a fixed image.
    struct Fixed(Image);
    impl ImageDecoder for Fixed {
        fn decode(&mut self, _: &[u8]) -> Result<Image, DecodeError> {
            Ok(self.0.clone())
        }
    }

    /// A strip of height 2 from a column pattern: `#` opaque white over a black
    /// half-transparent pixel, `.` clear.
    fn strip(cols: &str) -> Image {
        let w = cols.len();
        let mut rgba = vec![0u8; w * 2 * 4];
        for (x, c) in cols.bytes().enumerate() {
            if c == b'#' {
                rgba[x * 4..x * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
                let i = (w + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[200, 100, 50, 128]);
            }
        }
        Image { size: Size::new(w as i32, 2), rgba }
    }

    const DESC: &str = "info\n\tglyph_str ab?\"\nsize 10\n\tbitmap f10.png\n\tascent 8\n\tdescent 2\n\t\
                        advance_delta -1\n\tspace_advance 3\n\tx_ofs 1\nsize 20\n\tbitmap f20.png\n\tascent 16\n";

    fn load(desc: &str, size: u16, cols: &str) -> Option<TbbfFont> {
        let mut loader = MemLoader::new().with("fonts/f10.png", b"x").with("fonts/f20.png", b"y");
        loader.add_owned("fonts/f.tb.txt", desc.as_bytes().to_vec());
        let data = loader.load("fonts/f.tb.txt").unwrap();
        let mut images = Fixed(strip(cols));
        let mut cx = LoadCtx { loader: &mut loader, images: &mut images, path: "fonts/f.tb.txt" };
        TbbfFont::load("fonts/f.tb.txt", &data, size, &mut cx)
    }

    #[test]
    fn finds_glyphs_between_clear_columns() {
        //            a    b   ?   "
        let f = load(DESC, 10, "..##.###..#.##.").unwrap();
        assert_eq!(f.glyph_count(), 4);
        assert_eq!(f.glyph_columns('a'), Some((2, 2)));
        assert_eq!(f.glyph_columns('b'), Some((5, 3)));
        assert_eq!(f.glyph_columns('?'), Some((10, 1)));
        assert_eq!(f.glyph_columns('"'), Some((12, 2)));
        assert_eq!(f.metrics, FontMetrics { ascent: 8, descent: 2, height: 10 });
        assert_eq!(f.size(), 10);
    }

    #[test]
    fn stops_at_the_first_missing_glyph() {
        // The quote runs into the right edge: TB drops it (no clear column after it).
        let f = load(DESC, 10, "#.#.#.##").unwrap();
        assert_eq!(f.glyph_count(), 3);
        assert!(!f.has_glyph('"'));
        // Fewer glyphs in the strip than in glyph_str.
        let f = load(DESC, 10, "#.").unwrap();
        assert_eq!(f.glyph_count(), 1);
        // An empty strip loads with no glyphs.
        let f = load(DESC, 10, "....").unwrap();
        assert_eq!(f.glyph_count(), 0);
    }

    #[test]
    fn metrics_and_missing_glyphs() {
        let mut f = load(DESC, 10, ".##.###.#.").unwrap();
        let m = |advance, w| GlyphMetrics { advance, x: 1, y: -8, w, h: 2 };
        assert_eq!(f.glyph_metrics('a'), Some(m(1, 2)));
        assert_eq!(f.glyph_metrics('b'), Some(m(2, 3)));
        assert_eq!(f.glyph_metrics(' '), Some(GlyphMetrics { advance: 3, x: 1, y: -8, w: 0, h: 0 }));
        // Missing glyphs use '?'.
        assert_eq!(f.glyph_metrics('z'), f.glyph_metrics('?'));

        // Without '?' a missing glyph has no metrics and does not render.
        let mut f = load(DESC, 10, ".##.").unwrap();
        assert_eq!(f.glyph_metrics('z'), None);
        let mut img = GlyphImage::new();
        assert!(!f.rasterize('z', &mut img));
        assert!(!f.rasterize(' ', &mut img));
    }

    #[test]
    fn rasterize_copies_premultiplied_columns() {
        let mut f = load(DESC, 10, ".##.###.#.").unwrap();
        let mut img = GlyphImage::new();
        assert!(f.rasterize('a', &mut img));
        assert_eq!(img.size, Size::new(2, 2));
        assert_eq!(img.format, GlyphFormat::Rgba);
        assert!(!img.color);
        let px = [255, 255, 255, 255];
        let shadow = [100, 50, 25, 128]; // premultiplied from (200, 100, 50, 128)
        assert_eq!(img.data, [px, px, shadow, shadow].concat());
        // '?' stands in for missing characters.
        let mut q = GlyphImage::new();
        assert!(f.rasterize('?', &mut q));
        assert!(f.rasterize('\u{4e2d}', &mut img));
        assert_eq!(img, q);
    }

    #[test]
    fn nearest_size_wins_first_on_ties() {
        let cols = "#.#.#.#.";
        assert_eq!(load(DESC, 10, cols).unwrap().size(), 10);
        assert_eq!(load(DESC, 14, cols).unwrap().size(), 10);
        assert_eq!(load(DESC, 15, cols).unwrap().size(), 10); // tie: first
        assert_eq!(load(DESC, 16, cols).unwrap().size(), 20);
        assert_eq!(load(DESC, 0, cols).unwrap().size(), 10);
        let f = load(DESC, 400, cols).unwrap();
        assert_eq!(f.size(), 20);
        // The size node's own values; defaults are 0.
        assert_eq!(f.metrics, FontMetrics { ascent: 16, descent: 0, height: 16 });
        assert_eq!((f.x_ofs(), f.advance_delta(), f.space_advance()), (0, 0, 0));
    }

    #[test]
    fn load_failures() {
        let cols = "#.";
        assert!(load("info\n\tglyph_str a\n", 10, cols).is_none(), "no size node");
        assert!(load("size 10\n\tbitmap f10.png\n", 10, cols).is_none(), "no glyph_str");
        assert!(load("info\n\tglyph_str a\nsize 10\n\tbitmap nope.png\n", 10, cols).is_none(), "no strip");

        let mut loader = MemLoader::new();
        let mut images = Fixed(strip(cols));
        let mut cx = LoadCtx::new(&mut loader, &mut images);
        assert!(TbbfBackend.open("f.ttf", DESC.as_bytes(), 10, &mut cx).is_none(), "not .tb.txt");
        assert!(TbbfFont::load("f.tb.txt", &[0xff, 0xfe], 10, &mut cx).is_none(), "not UTF-8");
    }

    #[test]
    fn color_fonts() {
        let desc = DESC.replace("info\n", "info\n\trgb 1\n");
        let mut f = load(&desc, 10, "#.").unwrap();
        assert!(f.is_color());
        let mut img = GlyphImage::new();
        assert!(f.rasterize('a', &mut img));
        assert!(img.color);
    }
}
