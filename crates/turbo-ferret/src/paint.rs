//! The paint walk and `PaintCtx` (port-design.md section 14).
//!
//! TB: TBWidget::InvokePaint, OnPaintChildren, CalculateOpacityInternal. Paint
//! borrows the tree immutably (decision 3): translation, opacity and clip are walk
//! parameters (H-17), and the painter batches into the narrow renderer.

use crate::arena::WidgetId;
use crate::font::{FontDesc, FontManager};
use crate::geom::{Color, Point, Rect};
use crate::id::TbId;
use crate::images::{ImageHandle, ImageManager};
use crate::layout::size::Visibility;
use crate::msg::Millis;
use crate::render::painter::Painter;
use crate::render::{BitmapId, Renderer};
use crate::skin::{Skin, SkinElement};
use crate::ui::{NodeRef, Ui, View};
use crate::widget::{SkinState, WidgetState};

/// Read-only access to the tree plus drawing helpers, in the widget's local
/// coordinates (0, 0 is its top left).
pub struct PaintCtx<'a, 'r> {
    pub(crate) view: View<'a>,
    pub(crate) painter: &'a mut Painter<'r>,
    pub(crate) id: WidgetId,
    pub(crate) rect: Rect,
    pub(crate) state: WidgetState,
    pub(crate) element: Option<TbId>,
    pub(crate) text_color: Color,
    pub(crate) font: FontDesc,
    pub(crate) now: Millis,
    pub(crate) repaint: &'a mut bool,
    pub(crate) fonts: &'a mut FontManager,
    pub(crate) images: &'a ImageManager,
    #[cfg(feature = "debug")]
    pub(crate) debug: &'a crate::debug::DebugState,
}

impl<'a, 'r> PaintCtx<'a, 'r> {
    pub fn this(&self) -> WidgetId {
        self.id
    }
    pub fn this_node(&self) -> NodeRef<'a> {
        self.view.node(self.id).expect("painting a live node")
    }
    pub fn node(&self, id: WidgetId) -> Option<NodeRef<'a>> {
        self.view.node(id)
    }
    /// Local rect: (0, 0, w, h).
    pub fn rect(&self) -> Rect {
        self.rect
    }
    /// Explicit plus automatic state.
    pub fn state(&self) -> WidgetState {
        self.state
    }
    pub fn auto_state(&self, id: WidgetId) -> WidgetState {
        self.view.auto_state(id)
    }
    /// The skin element used for the background (after overrides).
    pub fn element(&self) -> Option<&'a SkinElement> {
        self.element.and_then(|e| self.view.skin.element(e))
    }
    pub fn skin(&self) -> &'a Skin {
        self.view.skin
    }
    pub fn padding_rect(&self) -> Rect {
        self.rect.shrink_edges(self.view.padding(self.id))
    }
    /// True if this widget has the keyboard focus (whether or not the focus is shown).
    pub fn is_focused(&self) -> bool {
        self.view.input.focused == Some(self.id)
    }

    /// Width of `text` in a specific font.
    pub fn text_width_with(&self, font: FontDesc, text: &str) -> i32 {
        self.fonts.face(font).string_width(text)
    }

    /// Text color from the skin element, else inherited.
    pub fn text_color(&self) -> Color {
        self.text_color
    }
    /// The font in effect for this widget.
    pub fn font(&self) -> FontDesc {
        self.font
    }
    pub fn now(&self) -> Millis {
        self.now
    }
    /// Ask for another frame (for example to animate). The only write paint may do.
    pub fn request_repaint(&mut self) {
        *self.repaint = true;
    }

    /// Paint a skin element at `rect` with `state`. Returns the element used.
    pub fn draw_skin(&mut self, rect: Rect, skin: TbId, state: SkinState) -> Option<TbId> {
        self.view.skin.paint_skin(self.painter, rect, skin, state, &self.view, self.id)
    }

    /// Paint a skin element's overlays.
    pub fn draw_skin_overlays(&mut self, rect: Rect, skin: TbId, state: SkinState) {
        if let Some(e) = self.view.skin.element(skin) {
            self.view.skin.paint_overlay(self.painter, rect, e, state, &self.view, self.id);
        }
    }

    /// Paint the fade-out skins over the edges of `rect` where content is scrolled
    /// out of view: `skin_x` on the left and right edges, `skin_y` on the top and
    /// bottom ones, each only if its hidden distance is above zero.
    /// TB: DrawEdgeFadeout
    #[allow(clippy::too_many_arguments)]
    pub fn draw_edge_fadeout(
        &mut self,
        rect: Rect,
        skin_x: TbId,
        skin_y: TbId,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) {
        self.view.skin.paint_edge_fadeout(self.painter, rect, skin_x, skin_y, left, top, right, bottom);
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.painter.fill_rect(rect, color);
    }

    /// 1 px outline. TB: TBSkin::PaintRect
    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        self.painter.draw_rect(rect, color);
    }

    pub fn draw_bitmap(&mut self, dst: Rect, src: Rect, bitmap: BitmapId, color: Color) {
        self.painter.draw_bitmap(dst, src, bitmap, color);
    }

    /// Draw an image of the image manager, stretched to `dst`. Nothing is drawn for
    /// an empty handle. TB: TBImageWidget::OnPaint (g_renderer->DrawBitmap with a fragment)
    pub fn draw_image(&mut self, dst: Rect, image: &ImageHandle) {
        let Some(frag) = self.images.fragment(image) else { return };
        if let Some(bitmap) = self.images.atlas().bitmap(frag.page) {
            self.painter.draw_bitmap(dst, frag.rect, bitmap, Color::WHITE);
        }
    }

    /// The image manager (for inspection).
    pub fn images(&self) -> &'a ImageManager {
        self.images
    }

    /// Run `f` with drawing clipped to `rect` (local); the clip is restored after.
    pub fn with_clip(&mut self, rect: Rect, f: impl FnOnce(&mut PaintCtx<'a, 'r>)) {
        let screen = rect.offset_by(self.painter.translation());
        let old = self.painter.clip();
        self.painter.set_clip(screen.clip(&old));
        f(self);
        self.painter.set_clip(old);
    }

    /// Run `f` with opacity multiplied by `o`.
    pub fn with_opacity(&mut self, o: f32, f: impl FnOnce(&mut PaintCtx<'a, 'r>)) {
        let old = self.painter.opacity();
        self.painter.set_opacity(old * o);
        f(self);
        self.painter.set_opacity(old);
    }

    /// Run `f` with drawing offset by `d`.
    pub fn with_offset(&mut self, d: Point, f: impl FnOnce(&mut PaintCtx<'a, 'r>)) {
        let old = self.painter.translation();
        self.painter.set_translation(old + d);
        f(self);
        self.painter.set_translation(old);
    }

    /// Width of `text` in this widget's font.
    pub fn text_width(&self, text: &str) -> i32 {
        self.fonts.face(self.font).string_width(text)
    }

    /// Line height of this widget's font.
    pub fn font_height(&self) -> i32 {
        self.fonts.face(self.font).height()
    }

    /// Draw `text` with its top left at `pos` (local coordinates) in this widget's font.
    pub fn draw_text(&mut self, pos: Point, color: Color, text: &str) {
        self.fonts.draw_text(self.painter, self.font, pos, color, text);
    }

    /// Draw `text` in a specific font.
    pub fn draw_text_with(&mut self, font: FontDesc, pos: Point, color: Color, text: &str) {
        self.fonts.draw_text(self.painter, font, pos, color, text);
    }

    pub fn fonts(&self) -> &FontManager {
        self.fonts
    }

    pub fn painter(&mut self) -> &mut Painter<'r> {
        self.painter
    }
}

#[derive(Copy, Clone)]
struct PaintState {
    offset: Point,
    opacity: f32,
    text_color: Color,
    font: FontDesc,
}

/// Widget opacity with the skin's and the disabled factor. TB: CalculateOpacityInternal
fn node_opacity(v: &View<'_>, n: NodeRef<'_>, state: WidgetState, e: Option<&SkinElement>) -> f32 {
    let mut o = n.opacity();
    if let Some(e) = e {
        o *= e.opacity;
    }
    if state.contains(WidgetState::DISABLED) {
        o *= v.skin.defaults.disabled_opacity;
    }
    o.clamp(0.0, 1.0)
}

impl Ui {
    /// Paint the whole UI with `renderer`. Bitmaps (skin atlas pages) are created or
    /// updated first. TB: TBWidget::InvokePaint from the root
    pub fn paint(&mut self, renderer: &mut dyn Renderer) {
        let vp = self.viewport;
        let root = self.root();
        let now = self.now;
        let Ui { tree, input, skin, repaint, default_font, fonts, images, .. } = self;
        fonts.begin_frame();
        skin.atlas.upload_all(renderer);
        images.begin_frame(renderer);
        let mut painter = Painter::new(renderer, vp);
        #[cfg(feature = "debug")]
        painter.set_debug_batches(self.debug.settings.render_batches);
        let view = View { tree, input, skin };
        let st =
            PaintState { offset: Point::ZERO, opacity: 1.0, text_color: skin.defaults.text_color, font: *default_font };
        let mut frame = Frame {
            fonts,
            images,
            now,
            again: false,
            #[cfg(feature = "debug")]
            debug: &self.debug,
        };
        paint_node(&view, &mut frame, &mut painter, root, st);
        let again = frame.again;
        #[cfg(feature = "debug")]
        {
            painter.flush();
            let (batches, triangles) = painter.debug_stats();
            let stats = crate::debug::FrameStats { batches, triangles };
            painter.finish();
            // TB: TBRendererBatcher::EndPaint (it printed this every frame)
            if stats != self.debug.frame && self.debug.settings.render_batches {
                self.services.log(
                    crate::services::Level::Info,
                    format_args!("Frame rendered using {batches} batches and a total of {triangles} triangles."),
                );
            }
            self.debug.frame = stats;
            self.debug.prune(now);
        }
        #[cfg(not(feature = "debug"))]
        painter.finish();
        *repaint = again;
    }
}

/// What every node of one paint walk shares.
struct Frame<'a> {
    fonts: &'a mut FontManager,
    images: &'a ImageManager,
    now: Millis,
    /// A widget asked for another frame.
    again: bool,
    #[cfg(feature = "debug")]
    debug: &'a crate::debug::DebugState,
}

fn paint_node(v: &View<'_>, f: &mut Frame<'_>, p: &mut Painter, id: WidgetId, st: PaintState) {
    let Some(n) = v.node(id) else { return };
    let r = n.rect();
    if n.opacity() == 0.0 || r.is_empty() || n.visibility() != Visibility::Visible {
        return;
    }
    let state = v.auto_state(id);
    let elem = v.element(id);
    let opacity = st.opacity * node_opacity(v, n, state, elem);
    if opacity <= 0.0 {
        return;
    }
    let origin = st.offset + r.origin();
    p.set_translation(origin);
    p.set_opacity(opacity);
    let local = Rect::from_size(r.size());
    let used = elem.and_then(|e| v.skin.paint_skin(p, local, e.id, state, v, id));
    #[cfg(feature = "debug")]
    if f.debug.settings.layout_bounds {
        // TB: TBWidget::InvokePaint (LAYOUT_BOUNDS)
        p.draw_rect(local, Color::rgba(255, 255, 255, 50));
    }
    let used_e = used.and_then(|u| v.skin.element(u));
    let text_color = used_e.and_then(|e| e.text_color).unwrap_or(st.text_color);
    let font = n.font().unwrap_or(st.font);
    let b = n.behaviour();
    if let Some(b) = b {
        let mut cx = PaintCtx {
            view: *v,
            painter: p,
            id,
            rect: local,
            state,
            element: used,
            text_color,
            font,
            now: f.now,
            repaint: &mut f.again,
            fonts: &mut *f.fonts,
            images: f.images,
            #[cfg(feature = "debug")]
            debug: f.debug,
        };
        b.paint(&mut cx);
    }

    // Children. TB: OnPaintChildren
    let base = origin + used_e.map(|e| e.content_ofs).unwrap_or(Point::ZERO);
    let child_offset = base + b.map(|b| b.child_translation()).unwrap_or(Point::ZERO);
    let child_clip = b.and_then(|b| b.children_clip(local.shrink_edges(v.padding(id))));
    let old_clip = child_clip.map(|c| {
        let old = p.clip();
        p.set_clip(c.offset_by(base).clip(&old));
        old
    });
    let clip = p.clip();
    let child_st = PaintState { offset: child_offset, opacity, text_color, font };
    if !clip.is_empty() {
        for &c in n.children() {
            let Some(cn) = v.node(c) else { continue };
            if clip.intersects(&cn.rect().offset_by(child_offset)) {
                paint_node(v, f, p, c, child_st);
            }
        }
        // Overlays are painted after all children. TB: OnPaintChildren overlay loop
        for &c in n.children() {
            let Some(cn) = v.node(c) else { continue };
            if cn.visibility() != Visibility::Visible || !clip.intersects(&cn.rect().offset_by(child_offset)) {
                continue;
            }
            let Some(ce) = v.element(c) else { continue };
            if !ce.has_overlays() {
                continue;
            }
            let cs = v.auto_state(c);
            let co = opacity * node_opacity(v, cn, cs, Some(ce));
            if co > 0.0 {
                p.set_translation(child_offset);
                p.set_opacity(co);
                v.skin.paint_overlay(p, cn.rect(), ce, cs, v, c);
            }
        }
        // Generic focus for a focused child whose skin has no focused state.
        if let Some(fw) = v.input.focused.filter(|&fw| v.tree.parent(fw) == Some(id)) {
            if let Some(fnode) = v.node(fw) {
                let fs = v.auto_state(fw);
                let has = v.element(fw).is_some_and(|e| e.has_state(WidgetState::FOCUSED, v, fw));
                if !has && fs.contains(WidgetState::FOCUSED) {
                    p.set_translation(child_offset);
                    p.set_opacity(opacity);
                    v.skin.paint_skin(p, fnode.rect(), crate::tbid!("generic_focus"), fs, v, fw);
                }
            }
        }
    }
    #[cfg(feature = "debug")]
    if let Some(c) = child_clip.filter(|_| f.debug.settings.layout_clipping) {
        // TB: TBScrollContainerRoot::OnPaintChildren (LAYOUT_CLIPPING)
        p.set_translation(base);
        p.set_opacity(opacity);
        p.draw_rect(c, Color::rgba(255, 0, 0, 200));
    }
    if let Some(old) = old_clip {
        p.set_clip(old);
    }
    #[cfg(feature = "debug")]
    if f.debug.settings.layout_size_calculation {
        // Paint recently arranged layouts with red and recently measured widgets
        // with yellow, and keep repainting until they have timed out.
        // TB: TBWidget::InvokePaint (LAYOUT_PS_DEBUGGING)
        let recent = |t: Option<&Millis>| t.is_some_and(|&t| f.now < t + crate::debug::HIGHLIGHT_MS);
        let (arranged, measured) = (recent(f.debug.arranged.get(&id)), recent(f.debug.measured.get(&id)));
        p.set_translation(origin);
        p.set_opacity(opacity);
        if arranged {
            p.draw_rect(local, Color::rgba(255, 30, 30, 200));
        }
        if measured {
            p.draw_rect(local.shrink(1, 1, 1, 1), Color::rgba(255, 255, 30, 200));
        }
        f.again |= arranged || measured;
    }
    if let Some(b) = b {
        p.set_translation(origin);
        p.set_opacity(opacity);
        let mut cx = PaintCtx {
            view: *v,
            painter: p,
            id,
            rect: local,
            state,
            element: used,
            text_color,
            font,
            now: f.now,
            repaint: &mut f.again,
            fonts: &mut *f.fonts,
            images: f.images,
            #[cfg(feature = "debug")]
            debug: f.debug,
        };
        b.paint_after_children(&mut cx);
    }
}
