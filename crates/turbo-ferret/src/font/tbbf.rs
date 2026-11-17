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
//! # Glyph tables (a Turbo Ferret addition)
//!
//! A strip found by its clear columns cannot hold glyphs that reach under their
//! neighbours or have gaps, and every glyph is as high as the strip. So a size node
//! may list its glyphs instead, which is what `turbo-ferret-fontgen` writes when it
//! turns a TrueType font into bitmaps:
//!
//! ```text
//! size 14
//!     bitmap vera_14.png
//!     ascent 13
//!     descent 3
//!     height 16               line height, if not ascent + descent
//!     glyphs
//!         U+0020 0 0 0 0 0 0 4        a space: no image, advance 4
//!         U+0041 1 1 10 11 0 -11 10   x y w h in the bitmap, then the offset of
//!                                     the image from the pen (x) and from the
//!                                     baseline (y, negative is up), then the advance
//! ```
//!
//! With a glyph table, `glyph_str`, `x_ofs`, `advance_delta` and `space_advance` are
//! not used, and the bitmap can be packed in any way.
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

/// One glyph: where its image is in the bitmap and how it is placed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Cell {
    /// The image in the bitmap (`w` or `h` 0: nothing to draw).
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// Offset of the image from the pen and from the baseline.
    ofs_x: i32,
    ofs_y: i32,
    advance: i32,
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
    glyphs: BTreeMap<char, Cell>,
    /// The glyphs come from a glyph table, not from the strip's clear columns.
    table: bool,
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
        let height = size_node.int("height", ascent.saturating_add(descent));
        let metrics = FontMetrics { ascent, descent, height };

        // The bitmap for the size, in the same folder as the description.
        let bitmap = size_node.str("bitmap", "");
        let bitmap_path = cx.loader.resolve(path, &bitmap);
        let bytes = cx.loader.load(&bitmap_path).ok()?;
        let image = cx.images.decode(&bytes).ok()?;
        let (w, h) = (image.size.w.max(0) as usize, image.size.h.max(0) as usize);
        if image.rgba.len() < w * h * 4 {
            return None;
        }

        let x_ofs = size_node.int("x_ofs", 0);
        let advance_delta = size_node.int("advance_delta", 0);
        let table = size_node.get("glyphs");
        let glyphs = match table {
            Some(table) => table_glyphs(table, image.size),
            None => {
                // TB: TBBFRenderer::FindGlyphs (fails without a glyph string)
                let glyph_str = node.value_at("info>glyph_str")?.as_str();
                find_glyphs(&glyph_str, image.size, &image.rgba)
                    .into_iter()
                    .map(|(c, g)| {
                        let cell = Cell {
                            x: g.x,
                            y: 0,
                            w: g.w,
                            h: image.size.h,
                            ofs_x: x_ofs,
                            ofs_y: -ascent,
                            advance: g.w.saturating_add(advance_delta),
                        };
                        (c, cell)
                    })
                    .collect()
            }
        };

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
            x_ofs,
            advance_delta,
            space_advance: size_node.int("space_advance", 0),
            rgb: node.int("info>rgb", 0) != 0,
            strip_size: image.size,
            pixels,
            glyphs,
            table: table.is_some(),
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

    /// True if the glyphs come from a glyph table in the description (see the
    /// module documentation), false if they were found in the strip.
    pub fn has_glyph_table(&self) -> bool {
        self.table
    }

    /// The glyph for `c`, or `?` for characters the strip lacks.
    fn lookup(&self, c: char) -> Option<Cell> {
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
        if c == ' ' && !self.table {
            // A strip cannot hold a space: its width is given.
            return Some(GlyphMetrics {
                advance: self.space_advance,
                x: self.x_ofs,
                y: -self.metrics.ascent,
                w: 0,
                h: 0,
            });
        }
        let g = self.lookup(c)?;
        Some(GlyphMetrics { advance: g.advance, x: g.ofs_x, y: g.ofs_y, w: g.w, h: g.h })
    }

    // TB: tb_font_renderer_tbbf.cpp TBBFRenderer::RenderGlyph
    fn rasterize(&mut self, c: char, out: &mut GlyphImage) -> bool {
        if c == ' ' && !self.table {
            return false;
        }
        let Some(g) = self.lookup(c) else {
            return false;
        };
        if g.w <= 0 || g.h <= 0 {
            return false;
        }
        let stride = self.strip_size.w as usize * 4;
        let (x, w) = (g.x as usize * 4, g.w as usize * 4);
        out.size = Size::new(g.w, g.h);
        out.format = GlyphFormat::Rgba;
        out.color = self.rgb;
        out.data.clear();
        for row in self.pixels.chunks_exact(stride).skip(g.y as usize).take(g.h as usize) {
            out.data.extend_from_slice(&row[x..x + w]);
        }
        true
    }
}

/// Read a glyph table: one child per glyph, named by its code point (`U+0041`), with
/// seven numbers: x, y, w, h in the bitmap, x and y offset, advance. Entries that do
/// not parse or do not fit in the bitmap are left out.
fn table_glyphs(table: &ResNode, bitmap: Size) -> BTreeMap<char, Cell> {
    let mut glyphs = BTreeMap::new();
    for g in &table.children {
        let Some(c) = g.name.strip_prefix("U+").and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32)
        else {
            continue;
        };
        let Some(v) = g.value.as_array().filter(|v| v.len() == 7) else {
            continue;
        };
        let n = |i: usize| v[i].as_int();
        let cell = Cell { x: n(0), y: n(1), w: n(2), h: n(3), ofs_x: n(4), ofs_y: n(5), advance: n(6) };
        let fits = cell.x >= 0
            && cell.y >= 0
            && cell.w >= 0
            && cell.h >= 0
            && cell.x.checked_add(cell.w).is_some_and(|r| r <= bitmap.w)
            && cell.y.checked_add(cell.h).is_some_and(|b| b <= bitmap.h);
        if fits {
            glyphs.insert(c, cell);
        }
    }
    glyphs
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
    /// A glyph table: the 6x2 bitmap's top row is opaque white, its bottom row the
    /// half-transparent color (see `strip`).
    const TABLE: &str = "size 10\n\tbitmap f10.png\n\tascent 8\n\tdescent 2\n\theight 12\n\tglyphs\n\
                         \t\tU+0020 0 0 0 0 0 0 3\n\
                         \t\tU+0061 0 0 2 2 1 -7 4\n\
                         \t\tU+0062 3 1 3 1 -1 -1 2\n\
                         \t\tU+003F 2 0 1 1 0 -5 6\n\
                         \t\tU+0063 4 0 3 2 0 0 5\n\
                         \t\tU+0064 0 1 6 2 0 0 5\n\
                         \t\tU+0065 -1 0 2 2 0 0 5\n\
                         \t\tU+0066 0 0 2 2 0 0\n\
                         \t\tU+D800 0 0 1 1 0 0 5\n\
                         \t\tx 0 0 1 1 0 0 5\n\
                         \t\tU+1F600 5 1 1 1 0 -1 9\n";

    #[test]
    fn glyph_tables_place_glyphs_freely() {
        let mut f = load(TABLE, 10, "######").unwrap();
        assert!(f.has_glyph_table());
        assert_eq!(f.metrics, FontMetrics { ascent: 8, descent: 2, height: 12 });
        // The entries that parse and fit: space, a, b, ?, and the emoji.
        assert_eq!(f.glyph_count(), 5);
        for c in "cdef".chars() {
            assert!(!f.has_glyph(c), "{c:?}: outside the bitmap or malformed");
        }
        assert_eq!(f.glyph_metrics('a'), Some(GlyphMetrics { advance: 4, x: 1, y: -7, w: 2, h: 2 }));
        assert_eq!(f.glyph_metrics('b'), Some(GlyphMetrics { advance: 2, x: -1, y: -1, w: 3, h: 1 }));
        assert_eq!(f.glyph_metrics('😀'), Some(GlyphMetrics { advance: 9, x: 0, y: -1, w: 1, h: 1 }));
        // The space comes from the table, not from `space_advance`.
        assert_eq!(f.glyph_metrics(' '), Some(GlyphMetrics { advance: 3, ..Default::default() }));
        // Others are the question mark.
        assert_eq!(f.glyph_metrics('z'), f.glyph_metrics('?'));
        assert_eq!(f.glyph_columns('b'), Some((3, 3)));

        let mut img = GlyphImage::new();
        assert!(f.rasterize('a', &mut img));
        assert_eq!((img.size, img.format, img.color), (Size::new(2, 2), GlyphFormat::Rgba, false));
        let (top, bottom) = ([255, 255, 255, 255], [100, 50, 25, 128]);
        assert_eq!(img.data, [top, top, bottom, bottom].concat());
        // One row from the middle of the bitmap.
        assert!(f.rasterize('b', &mut img));
        assert_eq!(img.size, Size::new(3, 1));
        assert_eq!(img.data, [bottom, bottom, bottom].concat());
        assert!(!f.rasterize(' ', &mut img), "nothing to draw");
    }

    #[test]
    fn a_glyph_table_needs_no_glyph_string() {
        assert!(load(TABLE, 10, "######").is_some());
        // An empty table is a font with no glyphs, not a failure.
        let f = load("size 10\n\tbitmap f10.png\n\tglyphs\n", 10, "######").unwrap();
        assert!(f.has_glyph_table());
        assert_eq!(f.glyph_count(), 0);
    }
}
