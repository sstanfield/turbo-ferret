//! The font manager: font files by name, faces by description, the glyph cache, and
//! text measuring and drawing (port-design.md section 15).
//!
//! Font id 0 is a built-in fallback that draws boxes, so [`FontManager::face`] never
//! fails. Measuring needs only metrics, so it works without a renderer; glyphs are
//! rasterized into the cache on first draw and uploaded at paint.
//!
//! TB: tb_font_renderer.cpp (TBFontManager, TBFontFace, TBFontInfo)

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;

use crate::geom::{Color, Point, Rect, Size};
use crate::id::TbId;
use crate::render::Renderer;
use crate::render::painter::Painter;

use super::backend::{FontBackend, FontMetrics, GlyphMetrics, GlyphSource, LoadCtx};
use super::cache::{GLYPH_CACHE_PAGES, GLYPH_PAGE_SIZE, GlyphCache};
use super::desc::FontDesc;
use super::tbbf::TbbfBackend;

/// The built-in box font that every lookup falls back to. TB: m_test_font_desc
pub const BOX_FONT: FontDesc = FontDesc::new(TbId::NONE, 16);

/// How deep background fonts of background fonts are drawn.
const MAX_BACKGROUND_DEPTH: u32 = 4;

/// TB skips this code point (its UTF-8 decoder's "invalid" marker) when measuring
/// and drawing.
const SKIPPED: char = '\u{FFFF}';

/// A font file registered under a name. TB: TBFontInfo
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontInfo {
    name: String,
    path: String,
    id: TbId,
}

impl FontInfo {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// `TbId::new(name)`, the id to use in a [`FontDesc`].
    pub fn id(&self) -> TbId {
        self.id
    }
}

/// A font drawn behind a face (for example a shadow), at an offset and in a color
/// of its own. TB: TBFontFace::SetBackgroundFont
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BackgroundFont {
    pub desc: FontDesc,
    pub color: Color,
    pub offset: Point,
}

/// A loaded font at one size: measures strings. Drawing goes through
/// [`FontManager::draw_text`], which owns the glyph cache. TB: TBFontFace
pub struct FontFace {
    desc: FontDesc,
    metrics: FontMetrics,
    /// `None` for the box font. A `RefCell` so measuring works through `&FontFace`.
    source: Option<RefCell<Box<dyn GlyphSource>>>,
    background: Option<BackgroundFont>,
}

impl fmt::Debug for FontFace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontFace")
            .field("desc", &self.desc)
            .field("metrics", &self.metrics)
            .field("box_font", &self.is_box_font())
            .field("background", &self.background)
            .finish_non_exhaustive()
    }
}

impl FontFace {
    /// TB: TBFontFace::TBFontFace
    fn new(desc: FontDesc, source: Box<dyn GlyphSource>) -> FontFace {
        FontFace { desc, metrics: source.metrics(), source: Some(RefCell::new(source)), background: None }
    }

    /// The box font at `desc.size`. TB: TBFontFace::TBFontFace ("invent some metrics
    /// for the test font")
    fn boxes(desc: FontDesc) -> FontFace {
        let size = i32::from(desc.size);
        let metrics = FontMetrics { ascent: size - size / 4, descent: size / 4, height: size };
        FontFace { desc, metrics, source: None, background: None }
    }

    /// The description this face was created for (after fallback, the face actually
    /// used). TB: TBFontFace::GetFontDescription
    pub fn desc(&self) -> FontDesc {
        self.desc
    }

    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    /// Line height in pixels. TB: TBFontFace::GetHeight
    pub fn height(&self) -> i32 {
        self.metrics.height
    }

    /// TB: TBFontFace::GetAscent
    pub fn ascent(&self) -> i32 {
        self.metrics.ascent
    }

    /// TB: TBFontFace::GetDescent
    pub fn descent(&self) -> i32 {
        self.metrics.descent
    }

    /// True for the built-in fallback that draws boxes (font id 0).
    pub fn is_box_font(&self) -> bool {
        self.source.is_none()
    }

    pub fn background(&self) -> Option<BackgroundFont> {
        self.background
    }

    /// Width of a box-font character: a box a third of the height wide, plus a pixel.
    fn box_advance(&self) -> i32 {
        self.metrics.height / 3 + 1
    }

    /// Metrics of one glyph (`None` if the font has nothing for `c`). The box font
    /// reports its box.
    pub fn glyph_metrics(&self, c: char) -> Option<GlyphMetrics> {
        match &self.source {
            None => Some(GlyphMetrics {
                advance: self.box_advance(),
                x: 0,
                y: -self.metrics.ascent,
                w: self.metrics.height / 3,
                h: self.metrics.height,
            }),
            Some(src) => src.try_borrow_mut().ok()?.glyph_metrics(c),
        }
    }

    /// Width of `text` in pixels: the sum of the advances (no kerning). Characters
    /// the font lacks add nothing (TBBF fonts measure them as `?`).
    /// TB: TBFontFace::GetStringWidth
    pub fn string_width(&self, text: &str) -> i32 {
        let chars = text.chars().filter(|&c| c != SKIPPED);
        match &self.source {
            // This is the test font. Use same glyph width as height.
            None => self.box_advance().saturating_mul(chars.count().try_into().unwrap_or(i32::MAX)),
            Some(src) => {
                let Ok(mut src) = src.try_borrow_mut() else {
                    return 0;
                };
                chars.filter_map(|c| src.glyph_metrics(c)).fold(0, |w, m| w.saturating_add(m.advance))
            }
        }
    }
}

/// Creates and owns font faces, looked up by [`FontDesc`]. Fonts are registered by
/// name ([`add_font`](Self::add_font)), then created per size
/// ([`create_face`](Self::create_face)). Looking up a face that was not created
/// gives the default face, or the box font if there is none. TB: TBFontManager
pub struct FontManager {
    backends: Vec<Box<dyn FontBackend>>,
    infos: BTreeMap<TbId, FontInfo>,
    faces: BTreeMap<FontDesc, FontFace>,
    /// The box font at [`BOX_FONT`]; box fonts at other sizes live in `faces`.
    boxes: FontFace,
    cache: GlyphCache,
    default: FontDesc,
}

impl fmt::Debug for FontManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontManager")
            .field("backends", &self.backends.len())
            .field("infos", &self.infos)
            .field("faces", &self.faces.keys())
            .field("cache", &self.cache)
            .field("default", &self.default)
            .finish_non_exhaustive()
    }
}

impl Default for FontManager {
    fn default() -> Self {
        FontManager::new()
    }
}

impl FontManager {
    /// A manager with the TBBF backend, the default glyph cache, and the box font as
    /// the default. TB: TBFontManager::TBFontManager, register_tbbf_font_renderer
    pub fn new() -> FontManager {
        FontManager::with_glyph_cache(GLYPH_PAGE_SIZE, GLYPH_CACHE_PAGES)
    }

    /// Like [`new`](Self::new), with glyph cache pages of `page_size` and a limit of
    /// `max_pages` pages kept between frames (see [`GlyphCache::max_pages`]).
    pub fn with_glyph_cache(page_size: Size, max_pages: usize) -> FontManager {
        FontManager {
            backends: alloc::vec![Box::new(TbbfBackend) as Box<dyn FontBackend>],
            infos: BTreeMap::new(),
            faces: BTreeMap::new(),
            boxes: FontFace::boxes(BOX_FONT),
            cache: GlyphCache::new(page_size, max_pages),
            default: BOX_FONT,
        }
    }

    /// Add a backend, tried after the ones already added. TB: TBFontManager::AddRenderer
    pub fn add_backend(&mut self, backend: Box<dyn FontBackend>) {
        self.backends.push(backend);
    }

    /// Register the font file `path` under `name`, so faces can be created for
    /// `FontDesc { id: TbId::new(name), .. }`. Returns that id. Registering a name
    /// again replaces its path (faces already created keep theirs). The empty name
    /// (id 0) is the box font and cannot be registered: that returns `TbId::NONE`.
    /// TB: TBFontManager::AddFontInfo
    pub fn add_font(&mut self, name: &str, path: &str) -> TbId {
        let id = TbId::new(name);
        if id.is_some() {
            self.infos.insert(id, FontInfo { name: String::from(name), path: String::from(path), id });
        }
        id
    }

    /// TB: TBFontManager::GetFontInfo
    pub fn font_info(&self, id: TbId) -> Option<&FontInfo> {
        self.infos.get(&id)
    }

    /// True if a face was created for exactly `desc`. The box font at [`BOX_FONT`]
    /// always exists. TB: TBFontManager::HasFontFace
    pub fn has_face(&self, desc: FontDesc) -> bool {
        self.lookup(desc).is_some()
    }

    /// Load the font registered for `desc.id` at `desc.size` and add its face. The
    /// registered path is resolved against `cx.path` (use `""` for paths relative to
    /// the loader's root), loaded through `cx.loader`, and offered to each backend in
    /// order. Font id 0 creates a box font of that size. True if the face exists
    /// afterwards (it is not reloaded if it already did). TB: TBFontManager::CreateFontFace
    pub fn create_face(&mut self, desc: FontDesc, cx: &mut LoadCtx) -> bool {
        if self.has_face(desc) {
            return true;
        }
        if desc.id.is_none() {
            // Is this the test dummy font
            self.faces.insert(desc, FontFace::boxes(desc));
            return true;
        }
        let Some(info) = self.infos.get(&desc.id) else {
            return false;
        };
        let path = if cx.path.is_empty() { info.path.clone() } else { cx.loader.resolve(cx.path, &info.path) };
        let Ok(data) = cx.loader.load(&path) else {
            return false;
        };
        let mut fcx = LoadCtx { loader: &mut *cx.loader, images: &mut *cx.images, path: &path };
        // Iterate through font backends until we find one capable of opening this file.
        for backend in &self.backends {
            if let Some(source) = backend.open(&path, &data, desc.size, &mut fcx) {
                self.faces.insert(desc, FontFace::new(desc, source));
                return true;
            }
        }
        false
    }

    /// The face for `desc`, else the default face, else the box font at [`BOX_FONT`].
    /// Never fails. TB: TBFontManager::GetFontFace
    pub fn face(&self, desc: FontDesc) -> &FontFace {
        self.lookup(desc).or_else(|| self.lookup(self.default)).unwrap_or(&self.boxes)
    }

    fn lookup(&self, desc: FontDesc) -> Option<&FontFace> {
        if desc == BOX_FONT { Some(&self.boxes) } else { self.faces.get(&desc) }
    }

    /// Set the font used when a widget sets none. The face is not created here.
    /// TB: TBFontManager::SetDefaultFontDescription
    pub fn set_default(&mut self, desc: FontDesc) {
        self.default = desc;
    }

    /// The default font (initially [`BOX_FONT`]). TB: TBFontManager::GetDefaultFontDescription
    pub fn default_desc(&self) -> FontDesc {
        self.default
    }

    /// Draw `background` behind every string drawn with the face for `desc` (or stop
    /// with `None`). False if there is no face for exactly `desc`.
    /// TB: TBFontFace::SetBackgroundFont
    pub fn set_background_font(&mut self, desc: FontDesc, background: Option<BackgroundFont>) -> bool {
        let face = if desc == BOX_FONT { Some(&mut self.boxes) } else { self.faces.get_mut(&desc) };
        match face {
            Some(face) => {
                face.background = background;
                true
            }
            None => false,
        }
    }

    /// The glyph cache (for inspection).
    pub fn glyph_cache(&self) -> &GlyphCache {
        &self.cache
    }

    /// Rasterize the glyphs of `text` into the glyph cache now (they are otherwise
    /// rendered on first draw). No renderer is needed. False if some character has no
    /// glyph at all (TBBF fonts render missing characters as `?`).
    /// TB: TBFontFace::RenderGlyphs
    pub fn render_glyphs(&mut self, desc: FontDesc, text: &str) -> bool {
        let key = self.face(desc).desc;
        let FontManager { faces, cache, .. } = self;
        let Some(src) = faces.get_mut(&key).and_then(|f| f.source.as_mut()) else {
            return true; // This is the test font
        };
        let src = &mut **src.get_mut();
        let mut has_all = true;
        for c in text.chars().filter(|&c| c != SKIPPED) {
            let g = cache.render(key, c, src);
            has_all &= g.frag.is_some() || g.metrics != GlyphMetrics::default();
        }
        has_all
    }

    /// Draw `text` on one line with the face for `desc` (after fallback). `pos` is the
    /// top-left of the line in painter-local coordinates. The color multiplies the
    /// glyph texels (TBBF glyphs are white with a baked shadow); color glyphs ignore
    /// it. Every glyph the string needs is rasterized before the changed atlas pages
    /// are uploaded, once, and then the quads are emitted.
    /// TB: TBFontFace::DrawString
    pub fn draw_text(&mut self, painter: &mut Painter, desc: FontDesc, pos: Point, color: Color, text: &str) {
        self.draw_text_at_depth(painter, desc, pos, color, text, 0);
    }

    fn draw_text_at_depth(
        &mut self,
        painter: &mut Painter,
        desc: FontDesc,
        pos: Point,
        color: Color,
        text: &str,
        depth: u32,
    ) {
        let face = self.face(desc);
        let key = face.desc;
        if let Some(bg) = face.background {
            if depth < MAX_BACKGROUND_DEPTH {
                self.draw_text_at_depth(painter, bg.desc, pos + bg.offset, bg.color, text, depth + 1);
            }
        }

        let FontManager { faces, boxes, cache, .. } = self;
        // `key` came from `face`, so it is in `faces` unless it is the box font.
        let face = match faces.get_mut(&key) {
            Some(face) => face,
            None => boxes,
        };
        let metrics = face.metrics;
        let Some(src) = face.source.as_mut() else {
            draw_boxes(painter, metrics, pos, color, text);
            return;
        };
        let src = &mut **src.get_mut();

        // Rasterize every glyph the string needs, then upload changed pages once.
        for c in text.chars().filter(|&c| c != SKIPPED) {
            cache.render(key, c, src);
        }
        cache.upload(painter);

        let mut x = pos.x;
        for c in text.chars().filter(|&c| c != SKIPPED) {
            let Some(g) = cache.glyph(key, c) else {
                continue;
            };
            if let Some(frag) = g.frag {
                if let Some(bitmap) = cache.atlas().bitmap(frag.page) {
                    let dst =
                        Rect::new(x + g.metrics.x, pos.y + g.metrics.y + metrics.ascent, frag.rect.w, frag.rect.h);
                    painter.draw_bitmap(dst, frag.rect, bitmap, if g.color { Color::WHITE } else { color });
                }
            }
            x = x.saturating_add(g.metrics.advance);
        }
    }

    /// Call between frames, before painting: lets the glyph cache clear itself if it
    /// grew past its page limit.
    pub fn begin_frame(&mut self) {
        self.cache.begin_frame();
    }

    /// The renderer lost its bitmaps: the glyph pages are re-created on the next
    /// draw (H-22). TB: TBFontGlyphCache::OnContextLost
    pub fn renderer_lost(&mut self) {
        self.cache.renderer_lost();
    }

    /// Destroy the glyph page bitmaps (they are re-created on the next draw).
    /// TB: TBFontGlyphCache::OnContextLost (TBBitmapFragmentManager::DeleteBitmaps)
    pub fn destroy_bitmaps(&mut self, r: &mut dyn Renderer) {
        self.cache.destroy_bitmaps(r);
    }
}

/// The box font: a one pixel outline per character, a third of the height wide.
/// TB: TBFontFace::DrawString (the test font branch)
fn draw_boxes(painter: &mut Painter, metrics: FontMetrics, pos: Point, color: Color, text: &str) {
    let mut x = pos.x;
    for _ in text.chars().filter(|&c| c != SKIPPED) {
        painter.draw_rect(Rect::new(x, pos.y, metrics.height / 3, metrics.height), color);
        x = x.saturating_add(metrics.height / 3 + 1);
    }
}
