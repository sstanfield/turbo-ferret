//! The stack layout algorithm (TB's TBLayout), written once for the X axis with
//! rotate-in / rotate-out helpers. Measure and arrange are the same function, as in TB.
//!
//! TB: tb_layout.cpp (ValidateLayout, RotPreferredSize, RotRect, RotGravity...).

use alloc::vec::Vec;

use crate::arena::WidgetId;
use crate::geom::Rect;
use crate::layout::size::{Gravity, PreferredSize, SizeConstraints, SizeDep, Visibility};
use crate::ui::Ctx;
use crate::widget::Axis;

/// How children are sized on the cross axis. TB: LAYOUT_SIZE
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum LayoutSize {
    /// Fill if the child's gravity says so (top and bottom), else preferred.
    #[default]
    Gravity,
    Preferred,
    /// Up to the available size (and the child's max).
    Available,
}

/// Where children sit on the cross axis. TB: LAYOUT_POSITION
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum LayoutPosition {
    #[default]
    Center,
    LeftTop,
    RightBottom,
    Gravity,
}

/// How extra space on the main axis is distributed. TB: LAYOUT_DISTRIBUTION
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum LayoutDistribution {
    #[default]
    Preferred,
    /// All children may grow up to their max.
    Available,
    /// Children with left and right gravity may grow.
    Gravity,
}

/// Where the group of children sits on the main axis when there is extra space.
/// TB: LAYOUT_DISTRIBUTION_POSITION
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum LayoutDistributionPosition {
    #[default]
    Center,
    LeftTop,
    RightBottom,
}

/// TB: LAYOUT_OVERFLOW
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum LayoutOverflow {
    #[default]
    Clip,
    Scroll,
}

impl LayoutSize {
    pub fn parse(s: &str) -> Self {
        if s.contains("available") {
            LayoutSize::Available
        } else if s.contains("gravity") {
            LayoutSize::Gravity
        } else {
            LayoutSize::Preferred
        }
    }
}
impl LayoutPosition {
    pub fn parse(s: &str) -> Self {
        if s.contains("left") || s.contains("top") {
            LayoutPosition::LeftTop
        } else if s.contains("right") || s.contains("bottom") {
            LayoutPosition::RightBottom
        } else if s.contains("gravity") {
            LayoutPosition::Gravity
        } else {
            LayoutPosition::Center
        }
    }
}
impl LayoutDistribution {
    pub fn parse(s: &str) -> Self {
        if s.contains("available") {
            LayoutDistribution::Available
        } else if s.contains("gravity") {
            LayoutDistribution::Gravity
        } else {
            LayoutDistribution::Preferred
        }
    }
}
impl LayoutDistributionPosition {
    pub fn parse(s: &str) -> Self {
        if s.contains("left") || s.contains("top") {
            LayoutDistributionPosition::LeftTop
        } else if s.contains("right") || s.contains("bottom") {
            LayoutDistributionPosition::RightBottom
        } else {
            LayoutDistributionPosition::Center
        }
    }
}

/// The settings the algorithm needs (spacing already resolved to pixels).
#[derive(Copy, Clone, Debug, Default)]
pub struct StackSettings {
    pub axis: Axis,
    pub spacing: i32,
    pub size: LayoutSize,
    pub position: LayoutPosition,
    pub distribution: LayoutDistribution,
    pub distribution_position: LayoutDistributionPosition,
    pub reverse: bool,
}

// ---- rotation helpers (TB: RotPreferredSize, RotSizeConstraints, RotRect, RotGravity) ----

pub fn rot_ps(ps: PreferredSize, axis: Axis) -> PreferredSize {
    if axis == Axis::X {
        return ps;
    }
    let mut dep = SizeDep::NONE;
    if ps.dependency.contains(SizeDep::WIDTH_DEPEND_ON_HEIGHT) {
        dep |= SizeDep::HEIGHT_DEPEND_ON_WIDTH;
    }
    if ps.dependency.contains(SizeDep::HEIGHT_DEPEND_ON_WIDTH) {
        dep |= SizeDep::WIDTH_DEPEND_ON_HEIGHT;
    }
    PreferredSize {
        min: crate::geom::Size::new(ps.min.h, ps.min.w),
        max: crate::geom::Size::new(ps.max.h, ps.max.w),
        pref: crate::geom::Size::new(ps.pref.h, ps.pref.w),
        dependency: dep,
    }
}

pub fn rot_sc(sc: SizeConstraints, axis: Axis) -> SizeConstraints {
    if axis == Axis::X { sc } else { SizeConstraints::new(sc.available.h, sc.available.w) }
}

pub fn rot_rect(r: Rect, axis: Axis) -> Rect {
    if axis == Axis::X { r } else { Rect::new(r.y, r.x, r.h, r.w) }
}

pub fn rot_gravity(g: Gravity, axis: Axis) -> Gravity {
    if axis == Axis::X {
        return g;
    }
    let mut r = Gravity::NONE;
    if g.contains(Gravity::LEFT) {
        r |= Gravity::TOP;
    }
    if g.contains(Gravity::TOP) {
        r |= Gravity::LEFT;
    }
    if g.contains(Gravity::RIGHT) {
        r |= Gravity::BOTTOM;
    }
    if g.contains(Gravity::BOTTOM) {
        r |= Gravity::RIGHT;
    }
    r
}

/// TB: TBLayout::QualifyForExpansion
fn qualifies(s: &StackSettings, g: Gravity) -> bool {
    match s.distribution {
        LayoutDistribution::Available => true,
        LayoutDistribution::Gravity => g.fills_x(),
        LayoutDistribution::Preferred => false,
    }
}

/// Cross-axis size (in rotated space). TB: TBLayout::GetWantedHeight
fn wanted_height(s: &StackSettings, g: Gravity, ps: &PreferredSize, available: i32) -> i32 {
    let h = match s.size {
        LayoutSize::Gravity => {
            if g.fills_y() {
                available
            } else {
                available.min(ps.pref.h)
            }
        }
        LayoutSize::Preferred => available.min(ps.pref.h),
        LayoutSize::Available => available.min(ps.max.h),
    };
    h.min(ps.max.h)
}

/// Non-gone children in layout order.
fn layout_children(cx: &Ctx, s: &StackSettings) -> Vec<WidgetId> {
    let mut v: Vec<WidgetId> =
        cx.children(cx.this()).iter().copied().filter(|&c| cx.visibility(c) != Visibility::Gone).collect();
    if s.reverse {
        v.reverse();
    }
    v
}

/// Measure: the preferred size of the content (inside padding), given the inner
/// constraints. TB: ValidateLayout(constraints, &calculate_ps)
pub fn measure(cx: &mut Ctx, s: &StackSettings, inner_sc: SizeConstraints) -> PreferredSize {
    let kids = layout_children(cx, s);
    let mut ps = PreferredSize { max: crate::geom::Size::ZERO, ..PreferredSize::default() };
    let n = kids.len();
    for (i, c) in kids.into_iter().enumerate() {
        let ending = if i + 1 < n { s.spacing } else { 0 };
        let cps = rot_ps(cx.preferred_size(c, inner_sc), s.axis);
        let g = rot_gravity(cx.gravity(c), s.axis);
        ps.min.h = ps.min.h.max(cps.min.h);
        ps.pref.h = ps.pref.h.max(cps.pref.h);
        ps.min.w += cps.min.w + ending;
        ps.pref.w += cps.pref.w + ending;
        ps.max.w += cps.max.w + ending;
        let h = wanted_height(s, g, &cps, cps.max.h);
        ps.max.h = ps.max.h.max(h);
        ps.dependency |= cps.dependency;
    }
    rot_ps(ps, s.axis)
}

/// Arrange children inside the padding rect. Returns the overflow (used space beyond
/// the available main-axis size). TB: ValidateLayout(constraints, nullptr)
pub fn arrange(cx: &mut Ctx, s: &StackSettings) -> i32 {
    let this = cx.this();
    let rect = cx.rect(this);
    let pr = cx.padding_rect(this);
    let lr = rot_rect(pr, s.axis);
    let inner_sc = SizeConstraints::new(rect.w, rect.h).constrain_by_padding(rect.w - pr.w, rect.h - pr.h);
    let kids = layout_children(cx, s);
    let n = kids.len();

    // Pass 1: totals.
    let mut items = Vec::with_capacity(n);
    let mut total_pref = 0;
    let mut total_min_diff = 0;
    let mut total_max_diff = 0;
    for (i, &c) in kids.iter().enumerate() {
        let ending = if i + 1 < n { s.spacing } else { 0 };
        let ps = rot_ps(cx.preferred_size(c, inner_sc), s.axis);
        let g = rot_gravity(cx.gravity(c), s.axis);
        total_pref += ps.pref.w + ending;
        total_min_diff += ps.pref.w - ps.min.w;
        if qualifies(s, g) {
            total_max_diff += lr.w.min(ps.max.w) - ps.pref.w;
        }
        items.push((c, ps, g, ending));
    }

    let mut missing = (total_pref - lr.w).max(0);
    let mut extra = (lr.w - total_pref).max(0);
    let mut offset = lr.x;
    if extra != 0 && s.distribution_position != LayoutDistributionPosition::LeftTop {
        let mut used = total_pref;
        if s.distribution != LayoutDistribution::Preferred {
            used += extra.min(total_max_diff);
        }
        if s.distribution_position == LayoutDistributionPosition::Center {
            offset += (lr.w - used) / 2;
        } else {
            offset += lr.w - used;
        }
    }

    // Pass 2: place.
    let mut used = 0;
    for (c, ps, g, ending) in items {
        let mut width = ps.pref.w;
        if missing != 0 && total_min_diff != 0 {
            let diff = ps.pref.w - ps.min.w;
            let factor = diff as f32 / total_min_diff as f32;
            let removed = ((missing as f32 * factor) as i32).min(diff);
            width -= removed;
            total_min_diff -= diff;
            missing -= removed;
        } else if extra != 0 && total_max_diff != 0 && qualifies(s, g) {
            let capped = lr.w.min(ps.max.w);
            let diff = capped - ps.pref.w;
            let factor = diff as f32 / total_max_diff as f32;
            let added = ((extra as f32 * factor) as i32).min(diff);
            width += added;
            total_max_diff -= diff;
            extra -= added;
        }
        let avail_h = lr.h;
        let height = wanted_height(s, g, &ps, avail_h);
        let mut pos = lr.y;
        match s.position {
            LayoutPosition::Center => pos += (avail_h - height) / 2,
            LayoutPosition::RightBottom => pos += avail_h - height,
            LayoutPosition::Gravity => {
                if g.fills_y() {
                    pos += (avail_h - height) / 2;
                } else if g.contains(Gravity::BOTTOM) {
                    pos += avail_h - height;
                }
            }
            LayoutPosition::LeftTop => {}
        }
        let r = Rect::new(used + offset, pos, width, height);
        used += width + ending;
        cx.set_rect(c, rot_rect(r, s.axis));
    }
    (used - lr.w).max(0)
}
