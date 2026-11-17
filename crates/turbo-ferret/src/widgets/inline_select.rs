//! `InlineSelect`: a number with two arrow buttons that step it.
//! TB: tb_inline_select.cpp (TBInlineSelect)
//!
//! TB shows the number in an edit field. The edit field comes with a later milestone;
//! until then the number is a text field (it cannot be typed into).

use alloc::format;

use crate::arena::WidgetId;
use crate::event::{Event, EventKind, SpecialKey};
use crate::geom::Size;
use crate::layout::size::Gravity;
use crate::res::ResNode;
use crate::ui::Ctx;
use crate::value::Value;
use crate::widget::{Axis, Widget};

use super::text::TextField;
use super::{Button, Layout, SkinImage};

/// A select widget without a popup: two arrow buttons step a number between limits.
/// TB: TBInlineSelect
pub struct InlineSelect {
    value: i32,
    min: i32,
    max: i32,
    layout: Option<WidgetId>,
    text: Option<WidgetId>,
}

impl Default for InlineSelect {
    fn default() -> Self {
        InlineSelect { value: 0, min: 0, max: 100, layout: None, text: None }
    }
}

impl InlineSelect {
    pub fn limits(&self) -> (i32, i32) {
        (self.min, self.max)
    }

    /// TB: SetLimits
    pub fn set_limits(&mut self, cx: &mut Ctx, min: i32, max: i32) {
        self.min = min;
        self.max = max.max(min);
        let v = self.value;
        self.set_value_internal(cx, v, true);
    }

    /// TB: SetValueInternal
    fn set_value_internal(&mut self, cx: &mut Ctx, value: i32, update_text: bool) {
        let value = value.clamp(self.min, self.max);
        if value == self.value {
            return;
        }
        self.value = value;
        if let (true, Some(t)) = (update_text, self.text) {
            cx.set_text(t, &format!("{value}"));
        }
        cx.emit_changed(Value::Int(value));
    }
}

impl Widget for InlineSelect {
    fn type_name(&self) -> &'static str {
        "InlineSelect"
    }

    /// TB: TBInlineSelect::TBInlineSelect
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin(me, crate::tbid!("TBInlineSelect"));
        let mut layout = Layout::default();
        layout.spacing = Some(0);
        let layout = cx.add(me, layout);
        let pr = cx.padding_rect(me);
        cx.set_rect(layout, pr);
        cx.set_gravity(layout, Gravity::ALL);
        let step_button = |cx: &mut Ctx, id, arrow| {
            let mut b = Button::default();
            b.auto_repeat = true;
            let b = cx.add(layout, b);
            cx.set_skin(b, crate::tbid!("TBButton.flat"));
            let content = cx.content_root(b);
            let img = cx.add(content, SkinImage);
            cx.set_skin(img, arrow);
            cx.set_focusable(b, false);
            cx.set_id(b, id);
            b
        };
        let dec = step_button(cx, crate::tbid!("dec"), crate::tbid!("arrow.left"));
        let text = cx.add(layout, TextField::new("0"));
        cx.set_gravity(text, Gravity::ALL);
        let inc = step_button(cx, crate::tbid!("inc"), crate::tbid!("arrow.right"));
        let _ = (dec, inc);
        self.layout = Some(layout);
        self.text = Some(text);
    }

    /// TB: TBInlineSelect::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        let min = res.int("min", self.min);
        let max = res.int("max", self.max);
        self.set_limits(cx, min, max);
    }

    fn arrange(&mut self, cx: &mut Ctx, _old: Size, _new: Size) {
        if let Some(l) = self.layout {
            let pr = cx.padding_rect(cx.this());
            cx.set_rect(l, pr);
        }
    }

    /// TB: TBInlineSelect::OnSkinChanged
    fn on_skin_changed(&mut self, cx: &mut Ctx) {
        if let Some(l) = self.layout {
            let pr = cx.padding_rect(cx.this());
            cx.set_rect(l, pr);
        }
    }

    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {
        if let Some(l) = self.layout {
            cx.with_widget::<Layout, _>(l, |w, cx| w.set_axis(cx, axis));
        }
    }

    fn axis(&self) -> Axis {
        Axis::X
    }

    fn value(&self) -> Value {
        Value::Int(self.value)
    }

    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        self.set_value_internal(cx, v.as_int(), true);
    }

    /// TB: TBInlineSelect::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let step = match ev.kind {
            EventKind::KeyDown if ev.special == SpecialKey::Up => 1,
            EventKind::KeyDown if ev.special == SpecialKey::Down => -1,
            EventKind::Click => match cx.node(ev.target).map(|n| n.id()) {
                Some(id) if id == crate::tbid!("inc") => 1,
                Some(id) if id == crate::tbid!("dec") => -1,
                _ => return false,
            },
            _ => return false,
        };
        let v = self.value.saturating_add(step);
        self.set_value_internal(cx, v, true);
        true
    }
}
