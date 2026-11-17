//! The renderer interface: textured and colored quads inside a clip rect, plus
//! bitmap management. Everything else (translation, opacity, batching, atlasing)
//! lives in the core (port-design.md section 14).
//!
//! TB: tb_renderer.h (TBRenderer), made narrower.

use crate::geom::{Color, Rect, Size};

/// A bitmap handle, chosen by the renderer.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BitmapId(pub u32);

/// One rectangle to draw.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Quad {
    /// Destination in screen pixels (translation already applied).
    pub dst: Rect,
    /// Source texels in the bitmap (ignored for solid fills). A negative `w` (or `h`)
    /// mirrors horizontally (vertically): `x` (or `y`) is then the right (bottom) edge.
    /// Texel coordinate of destination pixel `i` is `src.x + (i + 0.5) * src.w / dst.w`.
    pub src: Rect,
    /// Premultiplied color; multiplies the texel. Opacity is already folded in.
    pub color: Color,
}

/// Implemented by backends (`turbo-ferret-soft`, `turbo-ferret-miniquad`, ...).
///
/// Blending is always premultiplied alpha: `out = texel * color + dst * (1 - src_alpha)`.
pub trait Renderer {
    fn begin_frame(&mut self, _viewport: Size) {}
    fn end_frame(&mut self) {}
    /// Create a bitmap from RGBA8 data: premultiplied alpha, row-major,
    /// `size.w * size.h * 4` bytes. Any size (no power-of-two requirement).
    fn create_bitmap(&mut self, size: Size, rgba: &[u8]) -> Option<BitmapId>;
    /// Replace the whole contents (same size as created).
    ///
    /// The core calls this in the middle of a frame only to add glyphs to a glyph
    /// cache page: texels that quads drawn earlier in the frame use are never
    /// changed. So a backend whose draws are only queued until the end of the frame
    /// (Metal) may let those quads see the new contents.
    fn update_bitmap(&mut self, bitmap: BitmapId, rgba: &[u8]);
    fn destroy_bitmap(&mut self, bitmap: BitmapId);
    /// Draw quads using one bitmap (`None` = solid color) inside one clip rect
    /// (screen pixels). Output = texel × color, blended with (ONE, ONE_MINUS_SRC_ALPHA).
    fn draw(&mut self, bitmap: Option<BitmapId>, clip: Rect, quads: &[Quad]);
}
