//! `Button` and `ClickLabel`. TB: tb_widgets_common.cpp (TBButton, TBClickLabel)
//!
//! Former by-value members (the button's layout and text field) are child nodes made
//! in `on_create` and kept as ids (H-11).

use alloc::string::String;

use crate::arena::WidgetId;
use crate::event::{Event, EventKind};
use crate::geom::{Point, Size};
use crate::id::TbId;
use crate::layout::size::{Gravity, Visibility};
use crate::layout::stack::LayoutDistributionPosition;
use crate::layout::{PreferredSize, SizeConstraints};
use crate::msg::Message;
use crate::res::ResNode;
use crate::ui::{Ctx, FocusReason, NodeRef};
use crate::value::Value;
use crate::widget::{Axis, HitStatus, Widget, WidgetState};

use super::Layout;
use super::text::TextField;

const AUTO_CLICK_FIRST_DELAY: u32 = 500;
const AUTO_CLICK_REPEAT_DELAY: u32 = 100;
const AUTO_CLICK: TbId = crate::tbid!("auto_click");

/// A clickable button with a text and optional children (icons). TB: TBButton
#[derive(Default)]
pub struct Button {
    text: String,
    pub toggle_mode: bool,
    pub auto_repeat: bool,
    /// The value (TB keeps it as the PRESSED state; the port mirrors it here so
    /// `value()` needs no node access).
    on: bool,
    layout: Option<WidgetId>,
    textfield: Option<WidgetId>,
}

impl Button {
    pub fn new(text: &str) -> Button {
        Button { text: String::from(text), ..Default::default() }
    }

    pub fn layout(&self) -> Option<WidgetId> {
        self.layout
    }

    /// Let the text shrink below its preferred width (it is then cut with an
    /// ellipsis). TB: TBButton::SetSqueezable
    pub fn set_squeezable(&mut self, cx: &mut Ctx, squeezable: bool) {
        let Some(t) = self.textfield else { return };
        if let Some(tf) = cx.get_mut::<TextField>(t) {
            if tf.squeezable != squeezable {
                tf.squeezable = squeezable;
                cx.invalidate_layout(t);
            }
        }
    }

    /// Toggle mode, or a member of a radio-like group. TB: CanToggle
    fn can_toggle(&self, cx: &Ctx) -> bool {
        self.toggle_mode || cx.node(cx.this()).is_some_and(|n| n.group_id().is_some())
    }

    /// Collapse the text field when it is empty and there are other children.
    /// TB: UpdateTextFieldVisibility
    fn update_textfield_visibility(&self, cx: &mut Ctx) {
        let (Some(l), Some(t)) = (self.layout, self.textfield) else { return };
        let collapse = self.text.is_empty() && cx.children(l).len() > 1;
        let v = if collapse { Visibility::Gone } else { Visibility::Visible };
        if cx.visibility(t) != v {
            cx.set_visibility(t, v);
        }
    }
}

impl Widget for Button {
    fn type_name(&self) -> &'static str {
        "Button"
    }

    /// TB: TBButton::TBButton
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_focusable(me, true);
        cx.set_click_by_key(me, true);
        cx.set_skin_quiet(me, crate::tbid!("TBButton"));
        let mut lay = Layout::new(Axis::X);
        lay.paint_overflow_fadeout = false;
        let l = cx.add(me, lay);
        cx.set_gravity(l, Gravity::ALL);
        let t = cx.add(l, TextField::new(&self.text));
        cx.set_gravity(t, Gravity::ALL);
        self.layout = Some(l);
        self.textfield = Some(t);
        self.update_textfield_visibility(cx);
    }

    /// TB: TBButton::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        self.auto_repeat = res.int("auto-repeat", self.auto_repeat as i32) != 0;
        self.toggle_mode = res.int("toggle-mode", self.toggle_mode as i32) != 0;
        if let Some(n) = res.get("squeezable") {
            self.set_squeezable(cx, n.value.as_int() != 0);
        }
    }

    fn content_root(&self) -> Option<WidgetId> {
        self.layout
    }

    fn arrange(&mut self, cx: &mut Ctx, _old: Size, _new: Size) {
        if let Some(l) = self.layout {
            let pr = cx.padding_rect(cx.this());
            cx.set_rect(l, pr);
        }
    }

    fn on_skin_changed(&mut self, cx: &mut Ctx) {
        if let Some(l) = self.layout {
            let pr = cx.padding_rect(cx.this());
            cx.set_rect(l, pr);
        }
    }

    fn on_process(&mut self, cx: &mut Ctx) {
        self.update_textfield_visibility(cx);
    }

    /// The empty text field of an image-only button must not count, also when the
    /// button is measured before its first update (TB hid it as soon as a child was
    /// added: ButtonLayout::OnChildAdded).
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        self.update_textfield_visibility(cx);
        cx.default_measure_content(sc)
    }

    /// Children do not get pointer input. TB: TBButton::GetHitStatus
    fn hit_test(&self, this: NodeRef, p: Point) -> HitStatus {
        if HitStatus::default_for(this, p) == HitStatus::NoHit { HitStatus::NoHit } else { HitStatus::HitNoChildren }
    }

    /// TB: TBButton::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if self.can_toggle(cx) && ev.kind == EventKind::Click && ev.target == cx.this() {
            let has_group = cx.node(cx.this()).is_some_and(|n| n.group_id().is_some());
            let v = self.value().as_int();
            if !(has_group && v != 0) {
                self.set_value(cx, &Value::Int((v == 0) as i32));
            }
        }
        false
    }

    /// Auto repeat. TB: TBButton::OnCaptureChanged
    fn on_capture_changed(&mut self, cx: &mut Ctx, captured: bool) {
        let me = cx.this();
        if captured && self.auto_repeat {
            cx.post_message_delayed(me, AUTO_CLICK, Value::Null, AUTO_CLICK_FIRST_DELAY);
        } else if !captured {
            cx.cancel_messages(me, Some(AUTO_CLICK));
        }
    }

    /// TB: TBButton::OnMessageReceived
    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {
        if msg.id != AUTO_CLICK {
            return;
        }
        let me = cx.this();
        let pos = cx.pointer_move_pos();
        let local = cx.local_rect();
        if cx.captured() == Some(me) && !cx.is_click_cancelled() && local.contains(pos) {
            let mut ev = Event::new(EventKind::Click, me).at(pos);
            ev.touch = true;
            cx.emit(ev);
        }
        // Repeat while held. (The release may not have been able to cancel a
        // message that was already on its way.)
        if cx.captured() == Some(me) {
            cx.post_message_delayed(me, AUTO_CLICK, Value::Null, AUTO_CLICK_REPEAT_DELAY);
        }
    }

    /// Pressed state as value. TB: TBButton::GetValue
    fn value(&self) -> Value {
        Value::Int(self.on as i32)
    }

    /// TB: TBButton::SetValue
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        let me = cx.this();
        let on = v.as_int() != 0;
        if self.on == on {
            return;
        }
        self.on = on;
        cx.set_state(me, WidgetState::PRESSED, on);
        if self.can_toggle(cx) {
            cx.emit_changed(Value::Int(on as i32));
        }
        if on && cx.node(me).is_some_and(|n| n.group_id().is_some()) {
            cx.update_group_widgets(me);
        }
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }

    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        self.text = String::from(text);
        if let Some(t) = self.textfield {
            cx.set_text(t, text);
        }
        self.update_textfield_visibility(cx);
    }

    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {
        if let Some(l) = self.layout {
            cx.with_widget::<Layout, _>(l, |w, cx| w.set_axis(cx, axis));
        }
    }

    fn axis(&self) -> Axis {
        Axis::X
    }
}

/// A label that forwards clicks to the widget next to it (a check box or radio
/// button). TB: TBClickLabel
#[derive(Default)]
pub struct ClickLabel {
    text: String,
    layout: Option<WidgetId>,
    textfield: Option<WidgetId>,
}

impl ClickLabel {
    pub fn new(text: &str) -> ClickLabel {
        ClickLabel { text: String::from(text), ..Default::default() }
    }
}

impl Widget for ClickLabel {
    fn type_name(&self) -> &'static str {
        "ClickLabel"
    }

    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        let mut lay = Layout::new(Axis::X);
        lay.distribution_position = LayoutDistributionPosition::LeftTop;
        let l = cx.add(me, lay);
        cx.set_gravity(l, Gravity::ALL);
        let t = cx.add(l, TextField::new(&self.text));
        self.layout = Some(l);
        self.textfield = Some(t);
    }

    fn content_root(&self) -> Option<WidgetId> {
        self.layout
    }

    fn arrange(&mut self, cx: &mut Ctx, _old: Size, _new: Size) {
        if let Some(l) = self.layout {
            let pr = cx.padding_rect(cx.this());
            cx.set_rect(l, pr);
        }
    }

    /// TB: TBClickLabel::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let Some(l) = self.layout else { return false };
        let kids = cx.children(l).to_vec();
        if kids.len() < 2 {
            return false;
        }
        let target = if Some(kids[0]) == self.textfield { kids[kids.len() - 1] } else { kids[0] };
        if ev.target == target || cx.is_ancestor_of(target, ev.target) {
            return false;
        }
        if !matches!(ev.kind, EventKind::Click | EventKind::PointerDown | EventKind::PointerUp | EventKind::PointerMove)
        {
            return false;
        }
        if ev.kind == EventKind::Click {
            cx.set_focus_with(target, FocusReason::Pointer);
        }
        let mut pressed = cx.auto_state(ev.target).contains(WidgetState::PRESSED);
        if matches!(ev.kind, EventKind::PointerUp | EventKind::Click) {
            pressed = false;
        }
        cx.set_state(target, WidgetState::PRESSED, pressed);
        let r = cx.rect(target);
        let mut fwd = Event::new(ev.kind, target).at(ev.pos - r.origin());
        fwd.touch = ev.touch;
        fwd.mods = ev.mods;
        cx.invoke_event(fwd)
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }

    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        self.text = String::from(text);
        if let Some(t) = self.textfield {
            cx.set_text(t, text);
        }
    }

    fn value(&self) -> Value {
        Value::str(&self.text)
    }
}
