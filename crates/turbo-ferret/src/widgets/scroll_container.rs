//! `ScrollContainer`: a container with scroll bars that scrolls its content.
//! TB: tb_scroll_container.cpp (TBScrollContainer, TBScrollContainerRoot,
//! TBScrollBarVisibility)
//!
//! TB's root asked its parent for the scroll bar values when it painted. Here the
//! container pushes the scroll position into the root (and keeps a copy for its own
//! `scroll_info`), so both answer without reaching into another widget.

use crate::arena::WidgetId;
use crate::event::{Event, EventKind, SpecialKey};
use crate::geom::{Edges, Point, Rect, Size};
use crate::layout::size::{Gravity, PreferredSize, SizeConstraints};
use crate::res::ResNode;
use crate::ui::{Ctx, NodeRef};
use crate::widget::{Axis, ScrollInfo, Widget};

use super::scroll::ScrollBar;

/// Which scroll bars a `ScrollContainer` shows. TB: SCROLL_MODE
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum ScrollMode {
    /// X and Y always (`scroll-mode: xy`).
    #[default]
    XY,
    /// Y always, X never (`scroll-mode: y`).
    Y,
    /// Y when needed, X never (`scroll-mode: y-auto`).
    YAuto,
    /// X and Y when needed (`scroll-mode: auto`).
    Auto,
    /// No scroll bars (`scroll-mode: off`).
    Off,
}

impl ScrollMode {
    /// TB: TBScrollContainer::OnInflate ("scroll-mode")
    pub fn parse(s: &str) -> Option<ScrollMode> {
        Some(match s {
            "xy" => ScrollMode::XY,
            "y" => ScrollMode::Y,
            "y-auto" => ScrollMode::YAuto,
            "auto" => ScrollMode::Auto,
            "off" => ScrollMode::Off,
            _ => return None,
        })
    }

    /// TB: TBScrollBarVisibility::IsAlwaysOnX
    pub fn always_x(self) -> bool {
        self == ScrollMode::XY
    }

    /// TB: TBScrollBarVisibility::IsAlwaysOnY
    pub fn always_y(self) -> bool {
        matches!(self, ScrollMode::XY | ScrollMode::Y)
    }
}

/// Which scroll bars are on, and the space left for the content.
/// TB: TBScrollBarVisibility
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct ScrollBarVisibility {
    pub x_on: bool,
    pub y_on: bool,
    pub visible: Size,
}

impl ScrollBarVisibility {
    /// TB: TBScrollBarVisibility::Solve
    pub fn solve(
        mode: ScrollMode,
        content: Size,
        available: Size,
        scrollbar_x_h: i32,
        scrollbar_y_w: i32,
    ) -> ScrollBarVisibility {
        let mut v = ScrollBarVisibility { x_on: false, y_on: false, visible: available };
        match mode {
            ScrollMode::XY => {
                v.x_on = true;
                v.y_on = true;
                v.visible.w -= scrollbar_y_w;
                v.visible.h -= scrollbar_x_h;
            }
            ScrollMode::Off => {}
            ScrollMode::Y => {
                v.y_on = true;
                v.visible.w -= scrollbar_y_w;
            }
            ScrollMode::YAuto => {
                if content.h > available.h {
                    v.y_on = true;
                    v.visible.w -= scrollbar_y_w;
                }
            }
            ScrollMode::Auto => {
                // One bar can make the other one needed, so X is checked twice.
                if content.w > v.visible.w {
                    v.x_on = true;
                    v.visible.h = available.h - scrollbar_x_h;
                }
                if content.h > v.visible.h {
                    v.y_on = true;
                    v.visible.w = available.w - scrollbar_y_w;
                }
                if content.w > v.visible.w {
                    v.x_on = true;
                    v.visible.h = available.h - scrollbar_x_h;
                }
            }
        }
        v
    }
}

/// The clipped, scrolled parent of a `ScrollContainer`'s content.
/// TB: TBScrollContainerRoot
#[derive(Default)]
pub struct ScrollContainerRoot {
    /// Scroll position (the child translation is its negative).
    pos: Point,
    /// Content is hidden beyond the left, top, right and bottom edges.
    more: [bool; 4],
}

impl Widget for ScrollContainerRoot {
    fn type_name(&self) -> &'static str {
        "ScrollContainerRoot"
    }

    /// TB: TBScrollContainerRoot::GetChildTranslation
    fn child_translation(&self) -> Point {
        Point::new(-self.pos.x, -self.pos.y)
    }

    /// Clip only towards hidden content, so expanded skins on the other sides are
    /// not cut. TB: TBScrollContainerRoot::OnPaintChildren
    fn children_clip(&self, padding: Rect) -> Option<Rect> {
        const FLUFF: i32 = 100;
        let f = |more: bool| if more { 0 } else { FLUFF };
        Some(padding.expand(f(self.more[0]), f(self.more[1]), f(self.more[2]), f(self.more[3])))
    }
}

/// A container with scroll bars that can scroll its children. TB: TBScrollContainer
pub struct ScrollContainer {
    mode: ScrollMode,
    adapt_to_content: bool,
    adapt_content: bool,
    info: ScrollInfo,
    /// Space taken by the visible scroll bars (right, bottom).
    bars: Size,
    scrollbar_x: Option<WidgetId>,
    scrollbar_y: Option<WidgetId>,
    root: Option<WidgetId>,
}

impl Default for ScrollContainer {
    fn default() -> Self {
        ScrollContainer {
            mode: ScrollMode::XY,
            adapt_to_content: false,
            adapt_content: false,
            info: ScrollInfo::default(),
            bars: Size::ZERO,
            scrollbar_x: None,
            scrollbar_y: None,
            root: None,
        }
    }
}

impl ScrollContainer {
    pub fn new(mode: ScrollMode) -> ScrollContainer {
        ScrollContainer { mode, ..Default::default() }
    }

    pub fn scroll_mode(&self) -> ScrollMode {
        self.mode
    }

    /// TB: TBScrollContainer::SetScrollMode
    pub fn set_scroll_mode(&mut self, cx: &mut Ctx, mode: ScrollMode) {
        if mode != self.mode {
            self.mode = mode;
            let me = cx.this();
            cx.invalidate_layout(me);
        }
    }

    pub fn adapt_to_content_size(&self) -> bool {
        self.adapt_to_content
    }

    /// Whether the preferred size of the container follows the preferred size of its
    /// content (off by default). TB: TBScrollContainer::SetAdaptToContentSize
    pub fn set_adapt_to_content_size(&mut self, cx: &mut Ctx, adapt: bool) {
        if adapt != self.adapt_to_content {
            self.adapt_to_content = adapt;
            let me = cx.this();
            // Without it the size is fixed, so content changes stay inside.
            cx.set_layout_boundary(me, !adapt);
            cx.invalidate_layout(me);
        }
    }

    pub fn adapt_content_size(&self) -> bool {
        self.adapt_content
    }

    /// Whether the content grows to the size of the container when that is larger
    /// than its preferred size. TB: TBScrollContainer::SetAdaptContentSize
    pub fn set_adapt_content_size(&mut self, cx: &mut Ctx, adapt: bool) {
        if adapt != self.adapt_content {
            self.adapt_content = adapt;
            let me = cx.this();
            cx.invalidate_layout(me);
        }
    }

    pub fn scrollbar_x(&self) -> Option<WidgetId> {
        self.scrollbar_x
    }

    pub fn scrollbar_y(&self) -> Option<WidgetId> {
        self.scrollbar_y
    }

    /// The clipped node the content lives in.
    pub fn root(&self) -> Option<WidgetId> {
        self.root
    }

    /// Take the scroll position and limits from the scroll bars, then `publish`.
    /// Returns true if the position changed.
    fn sync(&mut self, cx: &mut Ctx) -> bool {
        let old = self.info.pos;
        if let Some(sb) = self.scrollbar_x.and_then(|s| cx.get::<ScrollBar>(s)) {
            let (min, max, _) = sb.limits();
            self.info.min.x = min as i32;
            self.info.max.x = max as i32;
            self.info.pos.x = sb.value_f() as i32;
        }
        if let Some(sb) = self.scrollbar_y.and_then(|s| cx.get::<ScrollBar>(s)) {
            let (min, max, _) = sb.limits();
            self.info.min.y = min as i32;
            self.info.max.y = max as i32;
            self.info.pos.y = sb.value_f() as i32;
        }
        self.publish(cx, old)
    }

    /// Hand the scroll position to the root (which translates and clips the content).
    fn publish(&mut self, cx: &mut Ctx, old: Point) -> bool {
        let i = self.info;
        if let Some(r) = self.root.and_then(|r| cx.get_mut::<ScrollContainerRoot>(r)) {
            r.pos = i.pos;
            r.more = [i.can_scroll_left(), i.can_scroll_up(), i.can_scroll_right(), i.can_scroll_down()];
        }
        let changed = i.pos != old;
        if changed {
            cx.invalidate();
        }
        changed
    }

    /// Set the scroll bar values (they clamp) without events, then `sync`.
    fn set_bar_values(&mut self, cx: &mut Ctx, x: f64, y: f64) -> bool {
        if let Some(sb) = self.scrollbar_x {
            cx.with_widget::<ScrollBar, _>(sb, |w, cx| w.set_value_quiet(cx, x));
        }
        if let Some(sb) = self.scrollbar_y {
            cx.with_widget::<ScrollBar, _>(sb, |w, cx| w.set_value_quiet(cx, y));
        }
        self.sync(cx)
    }

    fn bar_values(&self, cx: &Ctx) -> (f64, f64) {
        let v = |s: Option<WidgetId>| s.and_then(|s| cx.get::<ScrollBar>(s)).map(|b| b.value_f()).unwrap_or(0.0);
        (v(self.scrollbar_x), v(self.scrollbar_y))
    }

    /// TB: TBScrollContainer::ValidateLayout
    fn validate_layout(&mut self, cx: &mut Ctx) {
        let (Some(sbx), Some(sby), Some(root)) = (self.scrollbar_x, self.scrollbar_y, self.root) else { return };
        let me = cx.this();
        let size = cx.rect(me).size();

        // Lay out the scroll bars whether they are visible or not.
        let sb_y_w = cx.preferred_size(sby, SizeConstraints::NONE).pref.w;
        let sb_x_h = cx.preferred_size(sbx, SizeConstraints::NONE).pref.h;
        cx.set_rect(sbx, Rect::new(0, size.h - sb_x_h, size.w - sb_y_w, sb_x_h));
        cx.set_rect(sby, Rect::new(size.w - sb_y_w, 0, sb_y_w, size.h));

        if let Some(content) = cx.children(root).first().copied() {
            let hp = if self.mode.always_y() { sb_y_w } else { 0 };
            let vp = if self.mode.always_x() { sb_x_h } else { 0 };
            let inner = SizeConstraints::new(size.w, size.h).constrain_by_padding(hp, vp);
            let ps = cx.preferred_size(content, inner);

            let vis = ScrollBarVisibility::solve(self.mode, ps.pref, size, sb_x_h, sb_y_w);
            cx.set_opacity(sbx, if vis.x_on { 1.0 } else { 0.0 });
            cx.set_opacity(sby, if vis.y_on { 1.0 } else { 0.0 });
            cx.set_rect(root, Rect::from_size(vis.visible));

            let rs = cx.rect(root).size();
            let content_size = if self.adapt_content {
                let mut w = ps.pref.w.max(rs.w);
                if !vis.x_on && rs.w < ps.pref.w {
                    w = ps.pref.w.min(rs.w);
                }
                Size::new(w, ps.pref.h.max(rs.h))
            } else {
                ps.pref
            };
            cx.set_rect(content, Rect::from_size(content_size));

            let max_w = (content_size.w - rs.w).max(0) as f64;
            let max_h = (content_size.h - rs.h).max(0) as f64;
            cx.with_widget::<ScrollBar, _>(sbx, |w, cx| w.set_limits_quiet(cx, 0.0, max_w, rs.w as f64));
            cx.with_widget::<ScrollBar, _>(sby, |w, cx| w.set_limits_quiet(cx, 0.0, max_h, rs.h as f64));
        }
        // The padding rect leaves out the visible scroll bars. TB: GetPaddingRect
        self.bars =
            Size::new(if cx.opacity(sby) != 0.0 { sb_y_w } else { 0 }, if cx.opacity(sbx) != 0.0 { sb_x_h } else { 0 });
        self.sync(cx);
    }
}

impl Widget for ScrollContainer {
    fn type_name(&self) -> &'static str {
        "ScrollContainer"
    }

    /// TB: TBScrollContainer::TBScrollContainer
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        let sbx = cx.add(me, ScrollBar::default());
        let sby = cx.add(me, ScrollBar::default());
        cx.with_widget::<ScrollBar, _>(sby, |w, cx| w.set_axis(cx, Axis::Y));
        let root = cx.add(me, ScrollContainerRoot::default());
        self.scrollbar_x = Some(sbx);
        self.scrollbar_y = Some(sby);
        self.root = Some(root);
        cx.set_layout_boundary(me, !self.adapt_to_content);
    }

    /// TB: TBScrollContainer::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        let me = cx.this();
        cx.set_gravity(me, Gravity::ALL);
        let adapt_content = res.int("adapt-content", self.adapt_content as i32) != 0;
        self.set_adapt_content_size(cx, adapt_content);
        let adapt_to_content = res.int("adapt-to-content", self.adapt_to_content as i32) != 0;
        self.set_adapt_to_content_size(cx, adapt_to_content);
        if let Some(mode) = res.get("scroll-mode").and_then(|n| ScrollMode::parse(&n.value.as_str())) {
            self.set_scroll_mode(cx, mode);
        }
    }

    fn content_root(&self) -> Option<WidgetId> {
        self.root
    }

    fn scroll_root(&self) -> Option<WidgetId> {
        self.root
    }

    fn scroll_info(&self) -> Option<ScrollInfo> {
        Some(self.info)
    }

    /// TB: TBScrollContainer::ScrollTo
    fn scroll_to(&mut self, cx: &mut Ctx, p: Point) {
        self.set_bar_values(cx, p.x as f64, p.y as f64);
    }

    /// TB: TBScrollContainer::GetPaddingRect
    fn extra_padding(&self, _this: NodeRef<'_>) -> Edges {
        Edges::new(0, 0, self.bars.w, self.bars.h)
    }

    /// TB: TBScrollContainer::OnCalculatePreferredContentSize
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        let mut ps = PreferredSize { pref: Size::new(100, 100), min: Size::new(50, 50), ..Default::default() };
        if !self.adapt_to_content {
            return ps;
        }
        let content = self.root.and_then(|r| cx.children(r).first().copied());
        if let (Some(content), Some(sbx), Some(sby)) = (content, self.scrollbar_x, self.scrollbar_y) {
            ps = cx.preferred_size(content, sc);
            let sb_y_w = cx.preferred_size(sby, SizeConstraints::NONE).pref.w;
            let sb_x_h = cx.preferred_size(sbx, SizeConstraints::NONE).pref.h;
            ps.pref.w += sb_y_w;
            ps.max.w += sb_y_w;
            if matches!(self.mode, ScrollMode::XY | ScrollMode::Auto) {
                ps.pref.h += sb_x_h;
                ps.max.h += sb_x_h;
            }
        }
        ps
    }

    /// TB: TBScrollContainer::OnResized / OnProcess (ValidateLayout)
    fn arrange(&mut self, cx: &mut Ctx, _old: Size, _new: Size) {
        self.validate_layout(cx);
    }

    /// TB: TBScrollContainer::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let me = cx.this();
        match ev.kind {
            // The user moved a scroll bar. It is busy firing this event, so the new
            // value comes from the event.
            EventKind::Changed if Some(ev.target) == self.scrollbar_x || Some(ev.target) == self.scrollbar_y => {
                if let Some(v) = ev.value.as_ref().map(|v| v.as_float() as i32) {
                    let old = self.info.pos;
                    if Some(ev.target) == self.scrollbar_x {
                        self.info.pos.x = v;
                    } else {
                        self.info.pos.y = v;
                    }
                    self.publish(cx, old);
                }
                true
            }
            EventKind::Wheel if ev.mods.is_empty() => {
                let step = cx.config().pixels_per_line;
                let (x, y) = self.bar_values(cx);
                self.set_bar_values(cx, x + (ev.delta.x * step) as f64, y + (ev.delta.y * step) as f64);
                self.bar_values(cx) != (x, y)
            }
            EventKind::KeyDown => {
                let line = cx.config().pixels_per_line;
                let page = cx.padding_rect(me).h;
                let i = self.info;
                match ev.special {
                    SpecialKey::Left if i.can_scroll_left() => cx.scroll_by_smooth(me, Point::new(-line, 0)),
                    SpecialKey::Right if i.can_scroll_right() => cx.scroll_by_smooth(me, Point::new(line, 0)),
                    SpecialKey::Up if i.can_scroll_up() => cx.scroll_by_smooth(me, Point::new(0, -line)),
                    SpecialKey::Down if i.can_scroll_down() => cx.scroll_by_smooth(me, Point::new(0, line)),
                    SpecialKey::PageUp if i.can_scroll_up() => cx.scroll_by_smooth(me, Point::new(0, -page)),
                    SpecialKey::PageDown if i.can_scroll_down() => cx.scroll_by_smooth(me, Point::new(0, page)),
                    SpecialKey::Home => cx.scroll_to_smooth(me, Point::new(i.pos.x, 0)),
                    SpecialKey::End => cx.scroll_to_smooth(me, Point::new(i.pos.x, i.max.y)),
                    _ => return false,
                }
                true
            }
            _ => false,
        }
    }
}
