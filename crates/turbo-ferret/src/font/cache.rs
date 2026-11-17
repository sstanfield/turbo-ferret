//! The glyph cache: rendered glyphs of every face, packed into one [`Atlas`].
//!
//! Glyphs are rasterized into the atlas's CPU pages on first use (no renderer
//! needed) and pages are uploaded at paint time, before the first quad that uses
//! them. The cache never frees a glyph during a frame, so a pending batch never
//! references texels that were overwritten. When the cache holds more than its page
//! limit between frames ([`FontManager::begin_frame`](super::FontManager::begin_frame)),
//! it is cleared and the glyphs in use are rasterized again (TB instead dropped the
//! least recently used glyphs while drawing).
//!
//! The atlas has no gutter: glyphs are always drawn 1:1 at integer positions, so
//! filtering never samples a neighbour (TB's glyph cache did not add a border either).
//!
//! TB: tb_font_renderer.cpp (TBFontGlyphCache, TBFontFace::GetGlyph, TBFontFace::RenderGlyph)

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;

use crate::geom::Size;
use crate::render::Renderer;
use crate::render::atlas::{Atlas, FragmentId};
use crate::render::painter::Painter;

use super::backend::{GlyphImage, GlyphMetrics, GlyphSource};
use super::desc::FontDesc;
use super::effect::FontEffect;

/// Size of a glyph cache page. Larger glyphs are not cached (and not drawn).
/// TB: TB_GLYPH_CACHE_WIDTH, TB_GLYPH_CACHE_HEIGHT
pub const GLYPH_PAGE_SIZE: Size = Size::new(512, 512);

/// Pages the glyph cache keeps between frames before it is cleared (TB used one).
pub const GLYPH_CACHE_PAGES: usize = 2;

/// A glyph in the cache: its metrics and where its image is. TB: TBFontGlyph
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub metrics: GlyphMetrics,
    /// The image in the atlas, or `None` if there is nothing to draw (a space, a
    /// missing glyph, or one larger than a page).
    pub frag: Option<FragmentId>,
    /// A color glyph, drawn ignoring the text color. TB: TBFontGlyph::has_rgb
    pub color: bool,
}

/// Caches rendered glyphs of all faces, keyed by face and character.
/// TB: TBFontGlyphCache
pub struct GlyphCache {
    atlas: Atlas,
    glyphs: BTreeMap<(FontDesc, char), Glyph>,
    max_pages: usize,
    /// `begin_frame` cleared the cache before the last frame.
    cleared_last_frame: bool,
    /// Scratch buffers for rasterizing.
    image: GlyphImage,
    rgba: Vec<u8>,
}

impl fmt::Debug for GlyphCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlyphCache")
            .field("glyphs", &self.glyphs.len())
            .field("pages", &self.atlas.page_count())
            .field("max_pages", &self.max_pages)
            .finish_non_exhaustive()
    }
}

impl GlyphCache {
    /// TB: TBFontGlyphCache::TBFontGlyphCache
    pub(crate) fn new(page_size: Size, max_pages: usize) -> GlyphCache {
        GlyphCache {
            atlas: Atlas::with_gutter(page_size, false),
            glyphs: BTreeMap::new(),
            max_pages: max_pages.max(1),
            cleared_last_frame: false,
            image: GlyphImage::new(),
            rgba: Vec::new(),
        }
    }

    /// The cached glyph, if `c` has been rendered for `face` since the last clear.
    /// TB: TBFontGlyphCache::GetGlyph
    pub fn glyph(&self, face: FontDesc, c: char) -> Option<&Glyph> {
        self.glyphs.get(&(face, c))
    }

    /// Number of cached glyphs (including ones with nothing to draw).
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Pages in the atlas.
    pub fn page_count(&self) -> usize {
        self.atlas.page_count()
    }

    /// Pages kept between frames before the cache is cleared. Raised when a single
    /// frame needs more, so a large working set does not re-rasterize every frame.
    pub fn max_pages(&self) -> usize {
        self.max_pages
    }

    /// The atlas holding the glyph images.
    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// The glyph for `c` in `face`, rendering it with `src` if it is not cached yet.
    /// TB: TBFontFace::GetGlyph (render_if_needed), TBFontFace::RenderGlyph,
    /// TBFontGlyphCache::CreateFragment
    pub(crate) fn render(
        &mut self,
        face: FontDesc,
        c: char,
        src: &mut dyn GlyphSource,
        effect: &mut FontEffect,
    ) -> Glyph {
        if let Some(g) = self.glyphs.get(&(face, c)) {
            return *g;
        }
        // TB gives a glyph the font lacks zero metrics.
        let metrics = src.glyph_metrics(c).unwrap_or_default();
        let mut glyph = Glyph { metrics, frag: None, color: false };
        self.image.clear();
        if src.rasterize(c, &mut self.image) {
            // TB: TBFontFace::RenderGlyph (m_effect.Render)
            effect.apply(&mut glyph.metrics, &mut self.image);
            glyph.color = self.image.color;
            let size = self.image.size;
            let page = self.atlas.default_page_size();
            // Don't bother if the glyph is larger than a page.
            if size.w <= page.w && size.h <= page.h && self.image.to_premultiplied_rgba(&mut self.rgba) {
                glyph.frag = self.atlas.add(size, &self.rgba);
            }
        }
        self.glyphs.insert((face, c), glyph);
        glyph
    }

    /// Create or update every page with new glyphs. The painter's pending batch is
    /// flushed only if something is uploaded. TB: TBBitmapFragmentMap::ValidateBitmap
    pub(crate) fn upload(&mut self, painter: &mut Painter) {
        let pages = self.atlas.page_count();
        if !(0..pages).any(|p| self.atlas.is_dirty(p as u16)) {
            return;
        }
        let r = painter.renderer();
        for p in 0..pages {
            if self.atlas.is_dirty(p as u16) {
                self.atlas.page_bitmap(p as u16, r);
            }
        }
    }

    /// Call between frames (before painting). If the cache holds more than its page
    /// limit it is cleared, and the next frame rasterizes the glyphs it uses again.
    /// If that frame alone needs more pages, the limit is raised to fit it instead.
    pub(crate) fn begin_frame(&mut self) {
        let over = self.atlas.page_count() > self.max_pages;
        if over && self.cleared_last_frame {
            self.max_pages = self.atlas.page_count();
            self.cleared_last_frame = false;
        } else if over {
            self.clear();
            self.cleared_last_frame = true;
        } else {
            self.cleared_last_frame = false;
        }
    }

    /// Forget the glyphs of one face, so they are rendered again when next used
    /// (their atlas space is reclaimed when the cache is next cleared).
    pub(crate) fn forget_face(&mut self, face: FontDesc) {
        self.glyphs.retain(|(f, _), _| *f != face);
    }

    /// Drop every glyph. Page bitmaps are destroyed on the next upload (or
    /// `destroy_bitmaps`). Only call between frames.
    pub(crate) fn clear(&mut self) {
        self.atlas.clear();
        self.glyphs.clear();
    }

    /// TB: TBFontGlyphCache::OnContextLost (here the bitmaps are just forgotten)
    pub(crate) fn renderer_lost(&mut self) {
        self.atlas.renderer_lost();
    }

    /// TB: TBFontGlyphCache::OnContextLost (TBBitmapFragmentManager::DeleteBitmaps)
    pub(crate) fn destroy_bitmaps(&mut self, r: &mut dyn Renderer) {
        self.atlas.destroy_bitmaps(r);
    }
}
