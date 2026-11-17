//! `Layout`: stacks children along an axis. TB: tb_layout.cpp (TBLayout)

use crate::event::{Event, EventKind};
use crate::geom::{Point, Rect};
use crate::layout::size::{Gravity, PreferredSize, SizeConstraints};
use crate::layout::stack::{
    self, LayoutDistribution, LayoutDistributionPosition, LayoutOverflow, LayoutPosition, LayoutSize, StackSettings,
};
use crate::res::ResNode;
use crate::ui::Ctx;
use crate::widget::{Axis, ScrollInfo, Widget};

#[derive(Clone, Debug)]
pub struct Layout {
    pub axis: Axis,
    /// `None` = from the skin element, else the skin default (TB: SPACING_FROM_SKIN).
    pub spacing: Option<i32>,
    pub size: LayoutSize,
    pub position: LayoutPosition,
    pub overflow: LayoutOverflow,
    pub distribution: LayoutDistribution,
    pub distribution_position: LayoutDistributionPosition,
    pub reverse: bool,
    pub paint_overflow_fadeout: bool,
    overflow_amount: i32,
    overflow_scroll: i32,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            axis: Axis::X,
            spacing: None,
            size: LayoutSize::Gravity,
            position: LayoutPosition::Center,
            overflow: LayoutOverflow::Clip,
            distribution: LayoutDistribution::Preferred,
            distribution_position: LayoutDistributionPosition::Center,
            reverse: false,
            paint_overflow_fadeout: true,
            overflow_amount: 0,
            overflow_scroll: 0,
        }
    }
}

impl Layout {
    pub fn new(axis: Axis) -> Layout {
        Layout { axis, ..Default::default() }
    }

    pub fn overflow(&self) -> i32 {
        self.overflow_amount
    }

    pub fn overflow_scroll(&self) -> i32 {
        self.overflow_scroll
    }

    /// TB: TBLayout::CalculateSpacing
    fn settings(&self, cx: &Ctx) -> StackSettings {
        let me = cx.this();
        let spacing = self.spacing.or_else(|| cx.skin_spacing(me)).unwrap_or_else(|| cx.default_spacing());
        StackSettings {
            axis: self.axis,
            spacing,
            size: self.size,
            position: self.position,
            distribution: self.distribution,
            distribution_position: self.distribution_position,
            reverse: self.reverse,
        }
    }

    /// TB: TBLayout::SetOverflowScroll
    fn set_overflow_scroll(&mut self, cx: &mut Ctx, v: i32) {
        let v = v.min(self.overflow_amount).max(0);
        if v != self.overflow_scroll {
            self.overflow_scroll = v;
            cx.invalidate();
        }
    }
}

impl Widget for Layout {
    fn type_name(&self) -> &'static str {
        "Layout"
    }

    /// TB: TBLayout::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        if let Some(s) = res.get("spacing") {
            self.spacing = Some(cx.px(&s.value.as_str(), 0));
        }
        let me = cx.this();
        cx.set_gravity(me, Gravity::ALL);
        if let Some(n) = res.get("size") {
            self.size = LayoutSize::parse(&n.value.as_str());
        }
        if let Some(n) = res.get("position") {
            self.position = LayoutPosition::parse(&n.value.as_str());
        }
        if let Some(n) = res.get("overflow") {
            self.overflow =
                if n.value.as_str().contains("scroll") { LayoutOverflow::Scroll } else { LayoutOverflow::Clip };
        }
        if let Some(n) = res.get("distribution") {
            self.distribution = LayoutDistribution::parse(&n.value.as_str());
        }
        if let Some(n) = res.get("distribution-position") {
            self.distribution_position = LayoutDistributionPosition::parse(&n.value.as_str());
        }
    }

    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        let s = self.settings(cx);
        stack::measure(cx, &s, sc)
    }

    /// TB: TBLayout::OnResized / OnProcess (ValidateLayout)
    fn arrange(&mut self, cx: &mut Ctx, _old: crate::geom::Size, _new: crate::geom::Size) {
        let s = self.settings(cx);
        self.overflow_amount = stack::arrange(cx, &s);
        let v = self.overflow_scroll;
        self.set_overflow_scroll(cx, v);
    }

    /// TB: TBLayout::OnEvent (wheel scrolls overflow)
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if ev.kind == EventKind::Wheel && ev.mods.is_empty() && self.overflow_amount > 0 {
            let old = self.overflow_scroll;
            let d = if self.axis == Axis::X && ev.delta.x != 0 { ev.delta.x } else { ev.delta.y };
            let step = cx.config().pixels_per_line;
            self.set_overflow_scroll(cx, old + d * step);
            return self.overflow_scroll != old;
        }
        false
    }

    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {
        if self.axis != axis {
            self.axis = axis;
            let me = cx.this();
            cx.invalidate_layout(me);
        }
    }

    fn axis(&self) -> Axis {
        self.axis
    }

    /// TB: TBLayout::GetChildTranslation
    fn child_translation(&self) -> Point {
        match self.axis {
            Axis::X => Point::new(-self.overflow_scroll, 0),
            Axis::Y => Point::new(0, -self.overflow_scroll),
        }
    }

    /// TB: TBLayout::OnPaintChildren (clips only when overflowing, with some fluff on
    /// the sides that are not scrolled).
    fn children_clip(&self, padding: Rect) -> Option<Rect> {
        if self.overflow_amount == 0 {
            return None;
        }
        const FLUFF: i32 = 100;
        let start = if self.overflow_scroll == 0 { FLUFF } else { 0 };
        let end = if self.overflow_scroll == self.overflow_amount { FLUFF } else { 0 };
        Some(match self.axis {
            Axis::X => padding.expand(start, FLUFF, end, FLUFF),
            Axis::Y => padding.expand(FLUFF, start, FLUFF, end),
        })
    }

    fn scroll_info(&self) -> Option<ScrollInfo> {
        let mut i = ScrollInfo::default();
        match self.axis {
            Axis::X => {
                i.max.x = self.overflow_amount;
                i.pos.x = self.overflow_scroll;
            }
            Axis::Y => {
                i.max.y = self.overflow_amount;
                i.pos.y = self.overflow_scroll;
            }
        }
        Some(i)
    }

    fn scroll_to(&mut self, cx: &mut Ctx, p: Point) {
        let v = if self.axis == Axis::X { p.x } else { p.y };
        self.set_overflow_scroll(cx, v);
    }
}
