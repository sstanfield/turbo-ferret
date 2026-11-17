//! Painting skin elements through the `Painter`. TB: TBSkin::PaintSkin,
//! PaintSkinOverlay, PaintElement*, GetFlippedRect

use crate::arena::WidgetId;
use crate::geom::{Color, Rect};
use crate::id::TbId;
use crate::render::painter::Painter;
use crate::render::{BitmapId, FragmentId};
use crate::ui::View;
use crate::widget::SkinState;

use super::element::{MatchRule, SkinElement, SkinKind, find_state_element};
use super::{MAX_DEPTH, Skin};

impl Skin {
    /// Paint element `id` (with overrides and children) at `dst`. Returns the element
    /// that was used (an override, or `id` itself). TB: TBSkin::PaintSkin
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_skin(
        &self,
        p: &mut Painter,
        dst: Rect,
        id: TbId,
        state: SkinState,
        v: &View<'_>,
        w: WidgetId,
        depth: u32,
    ) -> Option<TbId> {
        let e = self.element(id)?;
        if depth >= MAX_DEPTH {
            return None;
        }
        let mut used = e.id;
        let mut overridden = false;
        if let Some(o) = find_state_element(&e.overrides, state, v, w, MatchRule::Default) {
            if let Some(u) = self.paint_skin(p, dst, o.element, state, v, w, depth + 1) {
                used = u;
                overridden = true;
            }
        }
        if !overridden {
            self.paint_element(p, dst, e);
        }
        for c in &e.children {
            if c.is_match(state, v, w, MatchRule::Default) {
                self.paint_skin(p, dst, c.element, c.state & state, v, w, depth + 1);
            }
        }
        Some(used)
    }

    /// TB: TBSkin::PaintSkinOverlay
    pub(crate) fn paint_overlay(
        &self,
        p: &mut Painter,
        dst: Rect,
        e: &SkinElement,
        state: SkinState,
        v: &View<'_>,
        w: WidgetId,
    ) {
        for o in &e.overlays {
            if o.is_match(state, v, w, MatchRule::Default) {
                self.paint_skin(p, dst, o.element, o.state & state, v, w, 1);
            }
        }
    }

    fn bitmap(&self, f: FragmentId) -> Option<BitmapId> {
        self.atlas.bitmap(f.page)
    }

    /// TB: TBSkin::PaintElement
    pub(crate) fn paint_element(&self, p: &mut Painter, dst: Rect, e: &SkinElement) {
        if let Some(bg) = e.bg_color {
            if bg.a != 0 {
                p.fill_rect(dst, bg);
            }
        }
        let Some(frag) = e.bitmap else { return };
        let Some(bmp) = self.bitmap(frag) else { return };
        match e.kind {
            SkinKind::Image => paint_image(p, dst, e, frag, bmp),
            SkinKind::Tile => paint_tile(p, dst, e, frag, bmp),
            _ if e.kind == SkinKind::StretchImage || e.cut == 0 => paint_stretch_image(p, dst, e, frag, bmp),
            SkinKind::StretchBorder => paint_stretch_box(p, dst, e, frag, bmp, false),
            _ => paint_stretch_box(p, dst, e, frag, bmp, true),
        }
    }
}

/// Fragment-local source rect to atlas texels, with the element's flips.
/// TB: GetFlippedRect
fn src(frag: FragmentId, local: Rect, flip_x: bool, flip_y: bool) -> Rect {
    let mut r = local.offset(frag.rect.x, frag.rect.y);
    if flip_x {
        r.x += r.w;
        r.w = -r.w;
    }
    if flip_y {
        r.y += r.h;
        r.h = -r.h;
    }
    r
}

/// TB: PaintElementImage
fn paint_image(p: &mut Painter, dst: Rect, e: &SkinElement, frag: FragmentId, bmp: BitmapId) {
    let (bw, bh) = (frag.rect.w, frag.rect.h);
    let r = dst.expand_xy(e.expand, e.expand);
    let d = Rect::new(
        r.x + e.img_ofs.x + (r.w - bw) * e.img_position.x / 100,
        r.y + e.img_ofs.y + (r.h - bh) * e.img_position.y / 100,
        bw,
        bh,
    );
    p.draw_bitmap(d, src(frag, Rect::new(0, 0, bw, bh), e.flip_x, e.flip_y), bmp, Color::WHITE);
}

/// TB: PaintElementTile (DrawBitmapTile: repeat the bitmap from the top left).
fn paint_tile(p: &mut Painter, dst: Rect, e: &SkinElement, frag: FragmentId, bmp: BitmapId) {
    let r = dst.expand_xy(e.expand, e.expand);
    let (bw, bh) = (frag.rect.w, frag.rect.h);
    if bw <= 0 || bh <= 0 || r.is_empty() {
        return;
    }
    let mut y = r.y;
    while y < r.bottom() {
        let h = bh.min(r.bottom() - y);
        let mut x = r.x;
        while x < r.right() {
            let w = bw.min(r.right() - x);
            p.draw_bitmap(Rect::new(x, y, w, h), src(frag, Rect::new(0, 0, w, h), false, false), bmp, Color::WHITE);
            x += bw;
        }
        y += bh;
    }
}

/// TB: PaintElementStretchImage
fn paint_stretch_image(p: &mut Painter, dst: Rect, e: &SkinElement, frag: FragmentId, bmp: BitmapId) {
    if dst.is_empty() {
        return;
    }
    let r = dst.expand_xy(e.expand, e.expand);
    let s = src(frag, Rect::new(0, 0, frag.rect.w, frag.rect.h), e.flip_x, e.flip_y);
    p.draw_bitmap(r, s, bmp, Color::WHITE);
}

/// Nine-slice. TB: PaintElementStretchBox. TB flipped the destination rect; quads
/// cannot have negative size here, so each piece is mirrored inside the rect instead.
fn paint_stretch_box(p: &mut Painter, dst: Rect, e: &SkinElement, frag: FragmentId, bmp: BitmapId, fill_center: bool) {
    if dst.is_empty() {
        return;
    }
    let r = dst.expand_xy(e.expand, e.expand);
    let cut = e.cut;
    let cw = cut.min(r.w / 2);
    let ch = cut.min(r.h / 2);
    let (bw, bh) = (frag.rect.w, frag.rect.h);
    let lr_edges = r.h > ch * 2;
    let tb_edges = r.w > cw * 2;
    let mut piece = |d: Rect, s: Rect| {
        let mut d = d;
        if e.flip_x {
            d.x = r.x + r.right() - d.right();
        }
        if e.flip_y {
            d.y = r.y + r.bottom() - d.bottom();
        }
        if !d.is_empty() {
            p.draw_bitmap(d, src(frag, s, e.flip_x, e.flip_y), bmp, Color::WHITE);
        }
    };
    piece(Rect::new(r.x, r.y, cw, ch), Rect::new(0, 0, cut, cut));
    piece(Rect::new(r.right() - cw, r.y, cw, ch), Rect::new(bw - cut, 0, cut, cut));
    piece(Rect::new(r.x, r.bottom() - ch, cw, ch), Rect::new(0, bh - cut, cut, cut));
    piece(Rect::new(r.right() - cw, r.bottom() - ch, cw, ch), Rect::new(bw - cut, bh - cut, cut, cut));
    if lr_edges {
        piece(Rect::new(r.x, r.y + ch, cw, r.h - ch * 2), Rect::new(0, cut, cut, bh - cut * 2));
        piece(Rect::new(r.right() - cw, r.y + ch, cw, r.h - ch * 2), Rect::new(bw - cut, cut, cut, bh - cut * 2));
    }
    if tb_edges {
        piece(Rect::new(r.x + cw, r.y, r.w - cw * 2, ch), Rect::new(cut, 0, bw - cut * 2, cut));
        piece(Rect::new(r.x + cw, r.bottom() - ch, r.w - cw * 2, ch), Rect::new(cut, bh - cut, bw - cut * 2, cut));
    }
    if fill_center && lr_edges && tb_edges {
        piece(
            Rect::new(r.x + cw, r.y + ch, r.w - cw * 2, r.h - ch * 2),
            Rect::new(cut, cut, bw - cut * 2, bh - cut * 2),
        );
    }
}
