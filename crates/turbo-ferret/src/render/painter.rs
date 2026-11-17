//! Quad batching, translation, opacity and clip, on top of the narrow [`Renderer`].
//!
//! The backend never sees a state machine: the painter bakes translation and
//! opacity into each quad and hands the renderer batches of quads that share one
//! bitmap and one clip rect (port-design.md section 14, H-17).
//!
//! TB: renderers/tb_renderer_batcher.cpp (TBRendererBatcher), tb_skin.cpp
//! (TBSkin::PaintRect, TBSkin::PaintRectFill)

use alloc::vec::Vec;

use crate::geom::{Color, Point, Rect, Size};
use crate::render::{BitmapId, Quad, Renderer};

/// Quads per batch before it is flushed. TB: VERTEX_BATCH_SIZE (2048 quads)
pub const BATCH_CAPACITY: usize = 1024;

/// Collects quads and draws them in batches. Created per frame: `new` calls
/// `begin_frame`, and `finish` (or dropping the painter) flushes and calls
/// `end_frame`, so frames are always balanced.
pub struct Painter<'r> {
    r: &'r mut dyn Renderer,
    viewport: Rect,
    translation: Point,
    opacity: f32,
    /// Current clip, screen space, always inside the viewport.
    clip: Rect,
    quads: Vec<Quad>,
    /// Bitmap and clip of the pending quads (meaningful while `quads` is not empty).
    batch_bitmap: Option<BitmapId>,
    batch_clip: Rect,
}

impl<'r> Painter<'r> {
    /// Start a frame. The clip is the whole viewport. TB: TBRendererBatcher::BeginPaint
    pub fn new(r: &'r mut dyn Renderer, viewport: Size) -> Self {
        r.begin_frame(viewport);
        let viewport = Rect::from_size(viewport);
        Painter {
            r,
            viewport,
            translation: Point::ZERO,
            opacity: 1.0,
            clip: viewport,
            quads: Vec::with_capacity(BATCH_CAPACITY),
            batch_bitmap: None,
            batch_clip: viewport,
        }
    }

    /// Flush and end the frame. TB: TBRendererBatcher::EndPaint
    pub fn finish(self) {
        // Drop does the work, so a painter that is dropped early still balances.
    }

    /// The renderer, with any pending batch flushed first, so bitmaps can be
    /// updated or destroyed safely (H-22).
    pub fn renderer(&mut self) -> &mut dyn Renderer {
        self.flush();
        &mut *self.r
    }

    pub fn viewport(&self) -> Size {
        self.viewport.size()
    }

    /// Added to every destination rect.
    pub fn translation(&self) -> Point {
        self.translation
    }

    pub fn set_translation(&mut self, p: Point) {
        self.translation = p;
    }

    /// Multiplies the alpha of every color (0..1).
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// TB: TBRendererBatcher::SetOpacity
    pub fn set_opacity(&mut self, o: f32) {
        self.opacity = if o.is_nan() { 0.0 } else { o.clamp(0.0, 1.0) };
    }

    /// The clip rect, screen space (not translated).
    pub fn clip(&self) -> Rect {
        self.clip
    }

    /// Set the clip rect (screen space, clamped to the viewport) and return the
    /// previous one. The pending batch is flushed lazily, when a quad with a
    /// different clip is added. TB: TBRendererBatcher::SetClipRect
    pub fn set_clip(&mut self, screen: Rect) -> Rect {
        let old = self.clip;
        self.clip = screen.clip(&self.viewport);
        old
    }

    /// Fill a rect (local coordinates) with a straight-alpha color.
    /// TB: TBSkin::PaintRectFill
    pub fn fill_rect(&mut self, local: Rect, color: Color) {
        self.add_quad(None, local, Rect::ZERO, color);
    }

    /// 1 pixel outline inside `local`. TB: TBSkin::PaintRect (thickness 1)
    pub fn draw_rect(&mut self, local: Rect, color: Color) {
        if local.w < 2 || local.h < 2 {
            self.fill_rect(local, color);
            return;
        }
        let Rect { x, y, w, h } = local;
        self.fill_rect(Rect::new(x, y, w, 1), color); // top
        self.fill_rect(Rect::new(x, y + h - 1, w, 1), color); // bottom
        self.fill_rect(Rect::new(x, y + 1, 1, h - 2), color); // left
        self.fill_rect(Rect::new(x + w - 1, y + 1, 1, h - 2), color); // right
    }

    /// Draw texels `src` of `bitmap` into `local_dst`, multiplied by a straight-alpha
    /// `color` (usually `Color::WHITE`; glyphs pass the text color). See
    /// [`Quad::src`] for the texel mapping and mirroring.
    /// TB: TBRendererBatcher::DrawBitmap, DrawBitmapColored
    pub fn draw_bitmap(&mut self, local_dst: Rect, src: Rect, bitmap: BitmapId, color: Color) {
        self.add_quad(Some(bitmap), local_dst, src, color);
    }

    /// Draw the pending batch, if any. TB: TBRendererBatcher::Batch::Flush
    pub fn flush(&mut self) {
        if !self.quads.is_empty() {
            self.r.draw(self.batch_bitmap, self.batch_clip, &self.quads);
            self.quads.clear();
        }
    }

    /// TB: TBRendererBatcher::AddQuadInternal
    fn add_quad(&mut self, bitmap: Option<BitmapId>, local: Rect, src: Rect, color: Color) {
        let dst = local.offset_by(self.translation);
        // Culled: outside the clip (or empty).
        if !dst.intersects(&self.clip) {
            return;
        }
        let color = if self.opacity < 1.0 { color.with_opacity(self.opacity) } else { color };
        if color.a == 0 {
            return; // Premultiplied, it would draw nothing.
        }
        if !self.quads.is_empty()
            && (self.batch_bitmap != bitmap || self.batch_clip != self.clip || self.quads.len() >= BATCH_CAPACITY)
        {
            self.flush();
        }
        self.batch_bitmap = bitmap;
        self.batch_clip = self.clip;
        self.quads.push(Quad { dst, src, color: color.premultiplied() });
    }
}

impl Drop for Painter<'_> {
    fn drop(&mut self) {
        self.flush();
        self.r.end_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Begin(Size),
        End,
        Draw(Option<BitmapId>, Rect, Vec<Quad>),
        Destroy(BitmapId),
    }

    #[derive(Default)]
    struct Rec {
        calls: Vec<Call>,
    }

    impl Rec {
        fn draws(&self) -> Vec<(Option<BitmapId>, Rect, usize)> {
            self.calls
                .iter()
                .filter_map(|c| match c {
                    Call::Draw(b, clip, q) => Some((*b, *clip, q.len())),
                    _ => None,
                })
                .collect()
        }
    }

    impl Renderer for Rec {
        fn begin_frame(&mut self, viewport: Size) {
            self.calls.push(Call::Begin(viewport));
        }
        fn end_frame(&mut self) {
            self.calls.push(Call::End);
        }
        fn create_bitmap(&mut self, _: Size, _: &[u8]) -> Option<BitmapId> {
            Some(BitmapId(0))
        }
        fn update_bitmap(&mut self, _: BitmapId, _: &[u8]) {}
        fn destroy_bitmap(&mut self, bitmap: BitmapId) {
            self.calls.push(Call::Destroy(bitmap));
        }
        fn draw(&mut self, bitmap: Option<BitmapId>, clip: Rect, quads: &[Quad]) {
            self.calls.push(Call::Draw(bitmap, clip, quads.to_vec()));
        }
    }

    const VP: Size = Size::new(100, 80);
    const FULL: Rect = Rect::new(0, 0, 100, 80);
    const B1: BitmapId = BitmapId(1);
    const B2: BitmapId = BitmapId(2);

    #[test]
    fn begin_and_end_frame() {
        let mut r = Rec::default();
        let p = Painter::new(&mut r, VP);
        assert_eq!(p.clip(), FULL);
        p.finish();
        assert_eq!(r.calls, [Call::Begin(VP), Call::End]);

        // Dropping without finish still flushes and ends the frame.
        let mut r = Rec::default();
        {
            let mut p = Painter::new(&mut r, VP);
            p.fill_rect(Rect::new(0, 0, 1, 1), Color::WHITE);
        }
        assert_eq!(r.calls.len(), 3);
        assert_eq!(r.calls[2], Call::End);
    }

    #[test]
    fn same_bitmap_and_clip_is_one_draw() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        for i in 0..5 {
            p.draw_bitmap(Rect::new(i * 10, 0, 8, 8), Rect::new(0, 0, 8, 8), B1, Color::WHITE);
        }
        p.set_clip(Rect::new(0, 0, 50, 50));
        p.set_clip(FULL); // back to the same clip: no split
        p.draw_bitmap(Rect::new(0, 20, 8, 8), Rect::new(0, 0, 8, 8), B1, Color::WHITE);
        p.finish();
        assert_eq!(r.draws(), [(Some(B1), FULL, 6)]);
        let Call::Draw(_, _, quads) = &r.calls[1] else { panic!() };
        assert_eq!(quads[1], Quad { dst: Rect::new(10, 0, 8, 8), src: Rect::new(0, 0, 8, 8), color: Color::WHITE });
    }

    #[test]
    fn bitmap_change_splits() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        let d = Rect::new(0, 0, 4, 4);
        p.draw_bitmap(d, d, B1, Color::WHITE);
        p.draw_bitmap(d, d, B1, Color::WHITE);
        p.draw_bitmap(d, d, B2, Color::WHITE);
        p.fill_rect(d, Color::BLACK);
        p.fill_rect(d, Color::BLACK);
        p.draw_bitmap(d, d, B1, Color::WHITE);
        p.finish();
        assert_eq!(r.draws(), [(Some(B1), FULL, 2), (Some(B2), FULL, 1), (None, FULL, 2), (Some(B1), FULL, 1)]);
    }

    #[test]
    fn clip_change_splits_and_is_clamped() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        let d = Rect::new(10, 10, 4, 4);
        p.fill_rect(d, Color::WHITE);
        let old = p.set_clip(Rect::new(5, 5, 200, 20));
        assert_eq!(old, FULL);
        assert_eq!(p.clip(), Rect::new(5, 5, 95, 20));
        p.fill_rect(d, Color::WHITE);
        p.fill_rect(d, Color::WHITE);
        assert_eq!(p.set_clip(old), Rect::new(5, 5, 95, 20));
        p.fill_rect(d, Color::WHITE);
        p.finish();
        assert_eq!(r.draws(), [(None, FULL, 1), (None, Rect::new(5, 5, 95, 20), 2), (None, FULL, 1)]);
    }

    #[test]
    fn full_buffer_splits() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        for _ in 0..BATCH_CAPACITY * 2 + 1 {
            p.fill_rect(Rect::new(0, 0, 1, 1), Color::WHITE);
        }
        p.finish();
        assert_eq!(r.draws(), [(None, FULL, BATCH_CAPACITY), (None, FULL, BATCH_CAPACITY), (None, FULL, 1)]);
    }

    #[test]
    fn culling() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        p.fill_rect(Rect::new(-10, 0, 10, 10), Color::WHITE); // left of the viewport
        p.fill_rect(Rect::new(100, 0, 10, 10), Color::WHITE); // right of it
        p.fill_rect(Rect::new(10, 10, 0, 10), Color::WHITE); // empty
        p.set_clip(Rect::new(20, 20, 10, 10));
        p.fill_rect(Rect::new(0, 0, 20, 20), Color::WHITE); // touches the clip corner only
        p.fill_rect(Rect::new(0, 0, 21, 21), Color::WHITE); // overlaps by one pixel: kept
        p.set_translation(Point::new(-50, 0));
        p.fill_rect(Rect::new(60, 22, 5, 5), Color::WHITE); // translated out of the clip
        p.fill_rect(Rect::new(72, 22, 5, 5), Color::WHITE); // translated into the clip
        p.set_clip(Rect::new(200, 200, 10, 10)); // outside the viewport: empty clip
        assert!(p.clip().is_empty());
        p.fill_rect(Rect::new(0, 0, 100, 100), Color::WHITE);
        p.finish();
        assert_eq!(r.draws(), [(None, Rect::new(20, 20, 10, 10), 2)]);
        let Call::Draw(_, _, q) = &r.calls[1] else { panic!() };
        assert_eq!(q[1].dst, Rect::new(22, 22, 5, 5));
    }

    #[test]
    fn translation_applies_to_dst_only() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        p.set_translation(Point::new(7, -3));
        assert_eq!(p.translation(), Point::new(7, -3));
        p.draw_bitmap(Rect::new(1, 5, 4, 4), Rect::new(2, 2, -4, 4), B1, Color::WHITE);
        p.draw_rect(Rect::new(0, 10, 5, 4), Color::BLACK);
        p.finish();
        let Call::Draw(_, _, q) = &r.calls[1] else { panic!() };
        assert_eq!(q[0].dst, Rect::new(8, 2, 4, 4));
        assert_eq!(q[0].src, Rect::new(2, 2, -4, 4));
        let Call::Draw(_, _, q) = &r.calls[2] else { panic!() };
        let dsts: Vec<Rect> = q.iter().map(|q| q.dst).collect();
        assert_eq!(
            dsts,
            [Rect::new(7, 7, 5, 1), Rect::new(7, 10, 5, 1), Rect::new(7, 8, 1, 2), Rect::new(11, 8, 1, 2)]
        );
    }

    #[test]
    fn draw_rect_small_is_a_fill() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        p.draw_rect(Rect::new(0, 0, 1, 5), Color::WHITE);
        p.finish();
        assert_eq!(r.draws(), [(None, FULL, 1)]);
    }

    #[test]
    fn opacity_folds_into_premultiplied_color() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        let d = Rect::new(0, 0, 4, 4);
        p.fill_rect(d, Color::rgba(200, 100, 50, 255)); // opaque: unchanged
        p.fill_rect(d, Color::rgba(200, 100, 50, 128)); // premultiplied
        p.set_opacity(0.5);
        assert_eq!(p.opacity(), 0.5);
        p.fill_rect(d, Color::rgba(200, 100, 50, 255)); // alpha 128
        p.draw_bitmap(d, d, B1, Color::WHITE); // white at alpha 128
        p.set_opacity(0.0);
        p.fill_rect(d, Color::WHITE); // skipped
        p.set_opacity(1.0);
        p.fill_rect(d, Color::TRANSPARENT); // skipped
        p.set_opacity(7.0);
        assert_eq!(p.opacity(), 1.0);
        p.finish();
        let colors: Vec<Color> = r
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::Draw(_, _, q) => Some(q.iter().map(|q| q.color)),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(
            colors,
            [
                Color::rgba(200, 100, 50, 255),
                Color::rgba(100, 50, 25, 128),
                Color::rgba(100, 50, 25, 128),
                Color::rgba(128, 128, 128, 128),
            ]
        );
    }

    #[test]
    fn renderer_flushes_first() {
        let mut r = Rec::default();
        let mut p = Painter::new(&mut r, VP);
        let d = Rect::new(0, 0, 4, 4);
        p.draw_bitmap(d, d, B1, Color::WHITE);
        p.renderer().destroy_bitmap(B1);
        p.renderer(); // nothing pending: no draw
        p.draw_bitmap(d, d, B2, Color::WHITE);
        p.flush();
        p.flush();
        p.finish();
        assert_eq!(
            r.calls,
            vec![
                Call::Begin(VP),
                Call::Draw(Some(B1), FULL, vec![Quad { dst: d, src: d, color: Color::WHITE }]),
                Call::Destroy(B1),
                Call::Draw(Some(B2), FULL, vec![Quad { dst: d, src: d, color: Color::WHITE }]),
                Call::End,
            ]
        );
    }
}
