//! Small widgets. TB: tb_widgets_common.cpp (TBSkinImage, TBSeparator,
//! TBProgressSpinner, TBRadioCheckBox, TBMover, TBResizer, TBDimmer)

use crate::anim::{ALMOST_ZERO_OPACITY, AnimSpec, OnDone};
use crate::event::{Event, EventKind};
use crate::geom::{Point, Rect};
use crate::id::TbId;
use crate::layout::size::{Gravity, PreferredSize};
use crate::msg::Message;
use crate::paint::PaintCtx;
use crate::ui::{Ctx, NodeRef};
use crate::value::Value;
use crate::widget::{DieAction, HitStatus, Widget, WidgetState};

/// An image from the skin; never larger than its preferred size. TB: TBSkinImage
#[derive(Default)]
pub struct SkinImage;

impl Widget for SkinImage {
    fn type_name(&self) -> &'static str {
        "SkinImage"
    }
    fn after_measure(&self, mut ps: PreferredSize) -> PreferredSize {
        ps.max = ps.pref;
        ps
    }
}

/// A line separator (disabled, so it gets no input). TB: TBSeparator
#[derive(Default)]
pub struct Separator;

impl Widget for Separator {
    fn type_name(&self) -> &'static str {
        "Separator"
    }
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBSeparator"));
        cx.set_state(me, WidgetState::DISABLED, true);
    }
}

const SPIN_SPEED: u32 = 1000 / 30;
const SPIN: TbId = TbId(1);

/// A spinner shown while its value is above 0. TB: TBProgressSpinner
#[derive(Default)]
pub struct ProgressSpinner {
    value: i32,
    frame: u32,
}

impl ProgressSpinner {
    pub fn is_running(&self) -> bool {
        self.value > 0
    }
}

impl Widget for ProgressSpinner {
    fn type_name(&self) -> &'static str {
        "ProgressSpinner"
    }
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBProgressSpinner"));
    }
    fn value(&self) -> Value {
        Value::Int(self.value)
    }
    /// TB: TBProgressSpinner::SetValue
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        let v = v.as_int().max(0);
        if v == self.value {
            return;
        }
        let me = cx.this();
        self.value = v;
        if v > 0 {
            if !cx.has_message(me, SPIN) {
                self.frame = 0;
                cx.post_message_delayed(me, SPIN, Value::Null, SPIN_SPEED);
            }
        } else {
            cx.cancel_messages(me, Some(SPIN));
        }
        cx.invalidate();
    }
    fn on_message(&mut self, cx: &mut Ctx, _m: &Message) {
        // Stopped while this message was on its way: no more frames.
        if self.value <= 0 {
            return;
        }
        self.frame += 1;
        cx.invalidate();
        let me = cx.this();
        cx.post_message_delayed(me, SPIN, Value::Null, SPIN_SPEED);
    }
    /// TB: TBProgressSpinner::OnPaint (frames side by side in one strip)
    fn paint(&self, p: &mut PaintCtx) {
        if !self.is_running() {
            return;
        }
        let Some(e) = p.skin().element(crate::tbid!("TBProgressSpinner.fg")) else { return };
        let Some(f) = e.bitmap else { return };
        let Some(bmp) = p.skin().atlas().bitmap(f.page) else { return };
        let size = f.rect.h;
        if size <= 0 {
            return;
        }
        let frames = (f.rect.w / size).max(1) as u32;
        let cur = (self.frame % frames) as i32;
        let dst = p.padding_rect();
        p.draw_bitmap(dst, Rect::new(f.rect.x + cur * size, f.rect.y, size, size), bmp, crate::geom::Color::WHITE);
    }
}

/// Common part of check boxes and radio buttons. TB: TBRadioCheckBox
#[derive(Default)]
pub struct RadioCheckBox {
    value: i32,
    skin: TbId,
    name: &'static str,
}

impl RadioCheckBox {
    fn create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_focusable(me, true);
        cx.set_click_by_key(me, true);
        cx.set_skin_quiet(me, self.skin);
    }

    /// TB: TBRadioCheckBox::SetValue
    fn set(&mut self, cx: &mut Ctx, v: i32) {
        if self.value == v {
            return;
        }
        self.value = v;
        let me = cx.this();
        cx.set_state(me, WidgetState::SELECTED, v != 0);
        cx.emit_changed(Value::Int(v));
        if v != 0 && cx.node(me).is_some_and(|n| n.group_id().is_some()) {
            cx.update_group_widgets(me);
        }
    }

    /// TB: TBRadioCheckBox::OnEvent
    fn event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if ev.target == cx.this() && ev.kind == EventKind::Click {
            let grouped = cx.node(cx.this()).is_some_and(|n| n.group_id().is_some());
            if !(grouped && self.value != 0) {
                let v = (self.value == 0) as i32;
                self.set(cx, v);
            }
        }
        false
    }
}

macro_rules! radio_check {
    ($name:ident, $skin:literal, $tn:literal) => {
        /// TB: TBCheckBox / TBRadioButton
        pub struct $name(RadioCheckBox);
        impl Default for $name {
            fn default() -> Self {
                $name(RadioCheckBox { value: 0, skin: crate::tbid!($skin), name: $tn })
            }
        }
        impl Widget for $name {
            fn type_name(&self) -> &'static str {
                self.0.name
            }
            fn on_create(&mut self, cx: &mut Ctx) {
                self.0.create(cx);
            }
            fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
                self.0.event(cx, ev)
            }
            fn value(&self) -> Value {
                Value::Int(self.0.value)
            }
            fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
                self.0.set(cx, v.as_int());
            }
            /// TB: TBRadioCheckBox::OnCalculatePreferredSize (fixed size)
            fn after_measure(&self, mut ps: PreferredSize) -> PreferredSize {
                ps.min = ps.pref;
                ps.max = ps.pref;
                ps
            }
        }
    };
}

radio_check!(CheckBox, "TBCheckBox", "CheckBox");
radio_check!(RadioButton, "TBRadioButton", "RadioButton");

/// Drag to move the parent (a window's title bar). TB: TBMover
#[derive(Default)]
pub struct Mover;

impl Widget for Mover {
    fn type_name(&self) -> &'static str {
        "Mover"
    }
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBMover"));
    }
    /// TB: TBMover::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let me = cx.this();
        let Some(target) = cx.parent(me) else { return false };
        if ev.kind == EventKind::PointerMove && cx.captured() == Some(me) {
            let down = cx.pointer_down_pos();
            let d = ev.pos - down;
            let mut r = cx.rect(target).offset_by(d);
            if let Some(gp) = cx.parent(target) {
                let g = cx.rect(gp);
                r.x = r.x.max(-down.x).min(g.w - down.x).max(-down.x);
                r.y = r.y.max(-down.y).min(g.h - down.y).max(-down.y);
            }
            cx.set_rect(target, r);
            return true;
        }
        false
    }
}

/// Drag to resize the parent (a window's corner). TB: TBResizer
#[derive(Default)]
pub struct Resizer;

impl Widget for Resizer {
    fn type_name(&self) -> &'static str {
        "Resizer"
    }
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBResizer"));
    }
    /// Only the lower right triangle hits. TB: TBResizer::GetHitStatus
    fn hit_test(&self, this: NodeRef, p: Point) -> HitStatus {
        const EXTRA: i32 = 3;
        if p.x < this.rect().w - p.y - EXTRA {
            return HitStatus::NoHit;
        }
        HitStatus::default_for(this, p)
    }
    /// TB: TBResizer::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let me = cx.this();
        let Some(target) = cx.parent(me) else { return false };
        if ev.kind == EventKind::PointerMove && cx.captured() == Some(me) {
            let d = ev.pos - cx.pointer_down_pos();
            let mut r = cx.rect(target);
            r.w = (r.w + d.x).max(50);
            r.h = (r.h + d.y).max(50);
            cx.set_rect(target, r);
            return true;
        }
        false
    }
}

/// Dims everything behind it (under modal windows); fades in and out.
/// TB: TBDimmer (+ its fades from TBWidgetsAnimationManager)
#[derive(Default)]
pub struct Dimmer;

impl Widget for Dimmer {
    fn type_name(&self) -> &'static str {
        "Dimmer"
    }
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBDimmer"));
        cx.set_gravity(me, Gravity::ALL);
    }
    fn on_added(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        if let Some(p) = cx.parent(me) {
            let s = cx.rect(p).size();
            cx.set_rect(me, Rect::from_size(s));
        }
        cx.animate(AnimSpec::opacity(me, ALMOST_ZERO_OPACITY, 1.0));
    }
    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        let me = cx.this();
        cx.animate(AnimSpec::opacity(me, 1.0, ALMOST_ZERO_OPACITY).on_done(OnDone::Remove));
        DieAction::KeepAlive
    }
}
