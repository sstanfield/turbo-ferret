//! The `Widget` trait: per-type behaviour of a node (port-design.md section 6).
//!
//! TB: tb_widgets.h (TBWidget's virtual methods). One trait with default methods for
//! every hook; the framework applies the generic part around each hook, so there is
//! no "call the base" rule (H-07).

use core::any::Any;

use crate::arena::WidgetId;
use crate::event::Event;
use crate::flags::flags;
use crate::geom::{Edges, Point, Size};
use crate::layout::size::{PreferredSize, SizeConstraints};
use crate::msg::Message;
use crate::paint::PaintCtx;
use crate::res::ResNode;
use crate::skin::Condition;
use crate::ui::{Ctx, NodeRef};
use crate::value::Value;

flags! {
    /// Widget state bits; the same type as skin states. TB: WIDGET_STATE / SKIN_STATE
    pub struct WidgetState: u8 {
        const DISABLED = 1;
        const FOCUSED = 2;
        const PRESSED = 4;
        const SELECTED = 8;
        const HOVERED = 16;
        const ALL = 31;
    }
}

/// Skin states are widget states. TB: SKIN_STATE
pub type SkinState = WidgetState;

impl WidgetState {
    /// TB: tb_skin.cpp StringToState
    pub fn parse(s: &str) -> WidgetState {
        let mut st = WidgetState::NONE;
        if s.contains("all") {
            return WidgetState::ALL;
        }
        if s.contains("disabled") {
            st |= WidgetState::DISABLED;
        }
        if s.contains("focused") {
            st |= WidgetState::FOCUSED;
        }
        if s.contains("pressed") {
            st |= WidgetState::PRESSED;
        }
        if s.contains("selected") {
            st |= WidgetState::SELECTED;
        }
        if s.contains("hovered") {
            st |= WidgetState::HOVERED;
        }
        st
    }
}

/// TB: AXIS
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Axis {
    #[default]
    X,
    Y,
}

impl Axis {
    pub fn parse(s: &str) -> Axis {
        if s == "y" { Axis::Y } else { Axis::X }
    }
    pub fn other(self) -> Axis {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        }
    }
}

/// TB: TB_ALIGN
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Align {
    Left,
    Top,
    Right,
    Bottom,
    #[default]
    Center,
}

impl Align {
    pub fn parse(s: &str) -> Align {
        match s {
            "left" => Align::Left,
            "top" => Align::Top,
            "right" => Align::Right,
            "bottom" => Align::Bottom,
            _ => Align::Center,
        }
    }
}

/// Z order position. TB: WIDGET_Z
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Z {
    /// Drawn on top of siblings (last child).
    #[default]
    Top,
    /// Drawn behind siblings (first child).
    Bottom,
}

/// Position relative to a reference sibling. TB: WIDGET_Z_REL
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ZRel {
    /// Visually behind the reference.
    Before,
    /// Visually above the reference.
    After,
}

/// TB: WIDGET_HIT_STATUS
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HitStatus {
    NoHit,
    Hit,
    /// Hit, and children should not be hit-tested.
    HitNoChildren,
}

impl HitStatus {
    /// TB: TBWidget::GetHitStatus
    pub fn default_for(this: NodeRef<'_>, p: Point) -> HitStatus {
        if !this.is_interactable() {
            return HitStatus::NoHit;
        }
        let r = this.rect();
        if p.x >= 0 && p.y >= 0 && p.x < r.w && p.y < r.h { HitStatus::Hit } else { HitStatus::NoHit }
    }
}

/// What a widget wants when asked to die. See `Ui::die`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DieAction {
    /// Remove now.
    Remove,
    /// The widget started something (an animation with `OnDone::Remove`) that removes it.
    KeepAlive,
}

/// Scroll position and limits. TB: TBWidget::ScrollInfo
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ScrollInfo {
    pub min: Point,
    pub max: Point,
    pub pos: Point,
}

impl ScrollInfo {
    pub fn can_scroll_x(&self) -> bool {
        self.max.x > self.min.x
    }
    pub fn can_scroll_y(&self) -> bool {
        self.max.y > self.min.y
    }
    pub fn can_scroll_left(&self) -> bool {
        self.pos.x > self.min.x
    }
    pub fn can_scroll_right(&self) -> bool {
        self.pos.x < self.max.x
    }
    pub fn can_scroll_up(&self) -> bool {
        self.pos.y > self.min.y
    }
    pub fn can_scroll_down(&self) -> bool {
        self.pos.y < self.max.y
    }
}

/// Per-type widget behaviour. Implemented the same way by built-in and app widgets.
///
/// Lifecycle and frame hooks get `&mut self` (the behaviour, taken out of its node) and
/// `&mut Ctx` (all of `Ui`; `cx.this()` is the node). Queries take `&self` so the
/// framework can ask them without a take-out, including during paint and hit testing.
#[allow(unused_variables)]
pub trait Widget: Any {
    // ---- identity ----
    /// "Button"; used by skin defaults and debugging.
    fn type_name(&self) -> &'static str;

    // ---- lifecycle ----
    /// Build internal child nodes (H-11). Called once, right after the node is created.
    fn on_create(&mut self, cx: &mut Ctx) {}
    /// Custom resource properties; the generic ones are already applied.
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {}
    fn on_added(&mut self, cx: &mut Ctx) {}
    /// Called before detaching; descendants still resolve. Only for the widget that
    /// is removed (not for its descendants), and not if it is removed from inside
    /// one of its own hooks: release outside resources in [`on_free`](Self::on_free).
    fn on_removed(&mut self, cx: &mut Ctx) {}
    /// The widget is gone for good: release what it holds outside its own subtree
    /// (global listeners, helper widgets elsewhere in the tree). Called exactly once
    /// for every widget, when its node is freed, whichever way it was removed; its
    /// node and its children are no longer in the tree. TB: the widget's destructor
    fn on_free(&mut self, ui: &mut crate::ui::Ui) {}
    fn on_child_added(&mut self, cx: &mut Ctx, child: WidgetId) {}
    fn on_child_removed(&mut self, cx: &mut Ctx, child: WidgetId) {}
    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        DieAction::Remove
    }
    fn on_focus_changed(&mut self, cx: &mut Ctx, focused: bool) {}
    fn on_capture_changed(&mut self, cx: &mut Ctx, captured: bool) {}
    fn on_visibility_changed(&mut self, cx: &mut Ctx) {}
    fn on_skin_changed(&mut self, cx: &mut Ctx) {}
    fn on_font_changed(&mut self, cx: &mut Ctx) {}
    /// A window was activated or deactivated (only called on window nodes).
    fn on_window_active_changed(&mut self, cx: &mut Ctx, active: bool) {}

    // ---- frame ----
    /// `true` = handled, stop bubbling.
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        false
    }
    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {}
    fn on_process_states(&mut self, cx: &mut Ctx) {}
    fn on_process(&mut self, cx: &mut Ctx) {}
    fn on_process_after_children(&mut self, cx: &mut Ctx) {}

    // ---- layout ----
    /// Preferred size of the content (inside padding). Default: union of children.
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        cx.default_measure_content(sc)
    }
    /// Whether the framework adds the skin's padding and size settings around
    /// `measure_content` (TB widgets that override OnCalculatePreferredSize say no).
    fn measure_adds_skin(&self) -> bool {
        true
    }
    /// Adjust the final preferred size (after skin and padding were applied).
    /// TB: widgets overriding OnCalculatePreferredSize to post-process TBWidget's result
    fn after_measure(&self, ps: PreferredSize) -> PreferredSize {
        ps
    }
    /// Lay out children for a new size. Default: gravity anchoring (TB OnResized).
    fn arrange(&mut self, cx: &mut Ctx, old: Size, new: Size) {
        cx.default_arrange(old, new)
    }
    /// Extra padding added to the skin's (TB: overridden GetPaddingRect, e.g. a window's title).
    fn extra_padding(&self, this: NodeRef<'_>) -> Edges {
        Edges::ZERO
    }

    // ---- paint (read-only) ----
    fn paint(&self, p: &mut PaintCtx) {}
    fn paint_after_children(&self, p: &mut PaintCtx) {}

    // ---- generic properties (H-07) ----
    fn value(&self) -> Value {
        Value::Null
    }
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {}
    fn text(&self) -> Option<&str> {
        None
    }
    fn set_text(&mut self, cx: &mut Ctx, text: &str) {}
    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {}
    fn axis(&self) -> Axis {
        Axis::X
    }

    // ---- structure and routing queries ----
    /// Where children are added (`None` = this node). TB: GetContentRoot
    fn content_root(&self) -> Option<WidgetId> {
        None
    }
    /// Where unhandled events bubble. TB: GetEventDestination
    fn event_destination(&self, this: NodeRef<'_>) -> Option<WidgetId> {
        this.parent()
    }
    fn hit_test(&self, this: NodeRef<'_>, p: Point) -> HitStatus {
        HitStatus::default_for(this, p)
    }
    /// Offset applied to children (scrolling). TB: GetChildTranslation
    fn child_translation(&self) -> Point {
        Point::ZERO
    }
    /// Clip rect for children (local coordinates), given the padding rect. `None` = no
    /// clip. TB: overridden OnPaintChildren with SetClipRect
    fn children_clip(&self, padding: crate::geom::Rect) -> Option<crate::geom::Rect> {
        None
    }
    fn scroll_info(&self) -> Option<ScrollInfo> {
        None
    }
    fn scroll_to(&mut self, cx: &mut Ctx, p: Point) {}
    /// The node whose child translation moves when this scrolls. TB: GetScrollRoot
    fn scroll_root(&self) -> Option<WidgetId> {
        None
    }
    /// Alignment for the skin's `align` condition (tab containers). TB: PROPERTY_ALIGN
    fn align(&self) -> Option<Align> {
        None
    }
    fn custom_skin_condition(&self, cond: &Condition) -> bool {
        false
    }
}
