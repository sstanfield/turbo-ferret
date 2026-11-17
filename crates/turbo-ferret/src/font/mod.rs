//! Fonts: descriptions, the font manager, backends and the glyph cache
//! (port-design.md section 15).
//!
//! - [`FontManager`] registers font files by name, creates faces per [`FontDesc`],
//!   measures through [`FontFace`] (no renderer needed) and draws with
//!   [`FontManager::draw_text`].
//! - [`FontBackend`] opens a font file as a [`GlyphSource`]. [`TbbfBackend`], TB's
//!   bitmap font, is built in.
//! - [`GlyphCache`] holds rendered glyphs in one atlas, uploaded at paint.
//!
//! Text is UTF-8, one code point per glyph, with no kerning, shaping or bidi (as TB).

pub mod backend;
pub mod cache;
#[cfg(feature = "default-font")]
pub mod default_font;
pub mod desc;
pub mod effect;
pub mod manager;
pub mod tbbf;

pub use backend::{FontBackend, FontMetrics, GlyphFormat, GlyphImage, GlyphMetrics, GlyphSource, LoadCtx};
pub use cache::{GLYPH_CACHE_PAGES, GLYPH_PAGE_SIZE, Glyph, GlyphCache};
#[cfg(feature = "default-font")]
pub use default_font::{DEFAULT_FONT, DEFAULT_FONT_LICENSE, DEFAULT_FONT_NAME, DEFAULT_FONT_SIZES, default_font};
pub use desc::FontDesc;
pub use effect::{FontEffect, MAX_BLUR_RADIUS};
pub use manager::{BOX_FONT, BackgroundFont, FontFace, FontInfo, FontManager};
pub use tbbf::{TbbfBackend, TbbfFont};
