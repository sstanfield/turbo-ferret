//! `ScrollBar` and `Slider`. TB: tb_widgets_common.cpp (TBScrollBar, TBSlider)

use crate::arena::WidgetId;
use crate::event::{Event, EventKind, SpecialKey};
use crate::geom::{Rect, Size};
use crate::layout::size::{Gravity, SizeConstraints};
use crate::res::ResNode;
use crate::ui::Ctx;
use crate::value::Value;
use crate::widget::{Axis, Widget};

use super::Plain;

/// A scroll bar with a draggable handle. TB: TBScrollBar
pub struct ScrollBar {
    axis: Axis,
    value: f64,
    min: f64,
    max: f64,
    visible: f64,
    to_pixel: f64,
    handle: Option<WidgetId>,
}

impl Default for ScrollBar {
    fn default() -> Self {
        ScrollBar { axis: Axis::X, value: 0.0, min: 0.0, max: 1.0, visible: 1.0, to_pixel: 0.0, handle: None }
    }
}

impl ScrollBar {
    pub fn handle(&self) -> Option<WidgetId> {
        self.handle
    }

    pub fn limits(&self) -> (f64, f64, f64) {
        (self.min, self.max, self.visible)
    }

    /// TB: TBScrollBar::SetLimits
    pub fn set_limits(&mut self, cx: &mut Ctx, min: f64, max: f64, visible: f64) {
        self.set_limits_impl(cx, min, max, visible, true);
    }

    /// `set_limits` without the `Changed` event, for an owner that tracks the value
    /// itself (its own hook is running, so the event could not reach it anyway).
    pub fn set_limits_quiet(&mut self, cx: &mut Ctx, min: f64, max: f64, visible: f64) {
        self.set_limits_impl(cx, min, max, visible, false);
    }

    fn set_limits_impl(&mut self, cx: &mut Ctx, min: f64, max: f64, visible: f64, emit: bool) {
        let max = max.max(min);
        let visible = visible.max(0.0);
        if (min, max, visible) == (self.min, self.max, self.visible) {
            return;
        }
        self.min = min;
        self.max = max;
        self.visible = visible;
        let v = self.value;
        self.set_value_impl(cx, v, emit);
        // While the handle is dragged, keep the pointer's grip on it: the point
        // where it went down is in the handle's coordinates, and the handle moves
        // (and resizes) with the new limits. Without this the content jumps on the
        // next pointer move. TB: TBScrollBar::SetLimits (ConvertToRoot/ConvertFromRoot)
        let dragged = self.handle.filter(|&h| cx.captured() == Some(h));
        let before = dragged.map(|h| cx.rect(h).origin());
        self.update_handle(cx);
        if let (Some(h), Some(before)) = (dragged, before) {
            let moved = cx.rect(h).origin() - before;
            cx.input.down_pos -= moved;
        }
    }

    /// TB: TBScrollBar::SetValueDouble
    pub fn set_value_f(&mut self, cx: &mut Ctx, v: f64) {
        self.set_value_impl(cx, v, true);
    }

    /// `set_value_f` without the `Changed` event (see `set_limits_quiet`).
    pub fn set_value_quiet(&mut self, cx: &mut Ctx, v: f64) {
        self.set_value_impl(cx, v, false);
    }

    fn set_value_impl(&mut self, cx: &mut Ctx, v: f64, emit: bool) {
        let v = v.clamp(self.min, self.max);
        if v == self.value {
            return;
        }
        self.value = v;
        self.update_handle(cx);
        if emit {
            cx.emit_changed(Value::Float(v as f32));
        } else {
            cx.invalidate();
        }
    }

    /// TB: TBScrollBar::CanScrollNegative
    pub fn can_scroll_negative(&self) -> bool {
        self.value > self.min
    }

    /// TB: TBScrollBar::CanScrollPositive
    pub fn can_scroll_positive(&self) -> bool {
        self.value < self.max
    }

    pub fn value_f(&self) -> f64 {
        self.value
    }

    fn apply_axis_skin(&self, cx: &mut Ctx) {
        let me = cx.this();
        let (bg, fg) = match self.axis {
            Axis::X => (crate::tbid!("TBScrollBarBgX"), crate::tbid!("TBScrollBarFgX")),
            Axis::Y => (crate::tbid!("TBScrollBarBgY"), crate::tbid!("TBScrollBarFgY")),
        };
        cx.set_skin_quiet(me, bg);
        if let Some(h) = self.handle {
            cx.set_skin_quiet(h, fg);
        }
    }

    /// TB: TBScrollBar::UpdateHandle
    fn update_handle(&mut self, cx: &mut Ctx) {
        let r = cx.rect(cx.this());
        let horizontal = self.axis == Axis::X;
        let available = if horizontal { r.w } else { r.h };
        let min_thickness = r.h.min(r.w);
        let visible_px = if self.max - self.min > 0.0 && self.visible > 0.0 {
            let prop = self.visible / (self.visible + self.max - self.min);
            let px = ((prop * available as f64) as i32).max(min_thickness);
            self.to_pixel = (available - px) as f64 / (self.max - self.min);
            px
        } else {
            self.to_pixel = 0.0;
            0
        };
        let pos = ((self.value - self.min) * self.to_pixel) as i32;
        let hr = if horizontal { Rect::new(pos, 0, visible_px, r.h) } else { Rect::new(0, pos, r.w, visible_px) };
        if let Some(h) = self.handle {
            cx.set_rect(h, hr);
        }
    }
}

impl Widget for ScrollBar {
    fn type_name(&self) -> &'static str {
        "ScrollBar"
    }
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        let h = cx.add(me, Plain);
        self.handle = Some(h);
        self.apply_axis_skin(cx);
    }
    /// TB: TBScrollBar::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        let axis = Axis::parse(&res.str("axis", "x"));
        self.set_axis(cx, axis);
        let me = cx.this();
        cx.set_gravity(me, if axis == Axis::X { Gravity::LEFT_RIGHT } else { Gravity::TOP_BOTTOM });
    }
    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {
        if axis != self.axis {
            self.axis = axis;
            self.apply_axis_skin(cx);
            cx.invalidate();
        }
    }
    fn axis(&self) -> Axis {
        self.axis
    }
    fn value(&self) -> Value {
        Value::Float(self.value as f32)
    }
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        self.set_value_f(cx, v.as_float() as f64);
    }
    fn arrange(&mut self, cx: &mut Ctx, _old: Size, _new: Size) {
        self.update_handle(cx);
    }
    /// The handle is placed by the bar; children are not measured. TB: no content
    fn measure_content(&mut self, _cx: &mut Ctx, _sc: SizeConstraints) -> crate::layout::size::PreferredSize {
        crate::layout::size::PreferredSize::default()
    }
    /// TB: TBScrollBar::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let me = cx.this();
        match ev.kind {
            EventKind::PointerMove if self.handle.is_some() && cx.captured() == self.handle => {
                if self.to_pixel > 0.0 {
                    let d = ev.pos - cx.pointer_down_pos();
                    let dv = if self.axis == Axis::X { d.x } else { d.y } as f64 / self.to_pixel;
                    let v = self.value + dv;
                    self.set_value_f(cx, v);
                }
                true
            }
            EventKind::PointerMove if ev.target == me => true,
            EventKind::PointerDown if ev.target == me => {
                let hr = self.handle.map(|h| cx.rect(h)).unwrap_or_default();
                let after = if self.axis == Axis::X { ev.pos.x > hr.x } else { ev.pos.y > hr.y };
                let v = self.value + if after { self.visible } else { -self.visible };
                self.set_value_f(cx, v);
                true
            }
            EventKind::Wheel => {
                let old = self.value;
                let v = self.value + (ev.delta.y * cx.config().pixels_per_line) as f64;
                self.set_value_f(cx, v);
                self.value != old
            }
            _ => false,
        }
    }
}

/// A value slider. TB: TBSlider
pub struct Slider {
    axis: Axis,
    value: f64,
    min: f64,
    max: f64,
    to_pixel: f64,
    handle: Option<WidgetId>,
}

impl Default for Slider {
    fn default() -> Self {
        Slider { axis: Axis::X, value: 0.0, min: 0.0, max: 1.0, to_pixel: 0.0, handle: None }
    }
}

impl Slider {
    pub fn handle(&self) -> Option<WidgetId> {
        self.handle
    }

    pub fn limits(&self) -> (f64, f64) {
        (self.min, self.max)
    }

    /// TB: TBSlider::GetSmallStep
    pub fn small_step(&self) -> f64 {
        (self.max - self.min) / 100.0
    }

    /// TB: TBSlider::SetLimits
    pub fn set_limits(&mut self, cx: &mut Ctx, min: f64, max: f64) {
        let min = min.min(max);
        if (min, max) == (self.min, self.max) {
            return;
        }
        self.min = min;
        self.max = max;
        let v = self.value;
        self.set_value_f(cx, v);
        self.update_handle(cx);
    }

    /// TB: TBSlider::SetValueDouble
    pub fn set_value_f(&mut self, cx: &mut Ctx, v: f64) {
        let v = v.clamp(self.min, self.max);
        if v == self.value {
            return;
        }
        self.value = v;
        self.update_handle(cx);
        cx.emit_changed(Value::Float(v as f32));
    }

    pub fn value_f(&self) -> f64 {
        self.value
    }

    fn apply_axis_skin(&self, cx: &mut Ctx) {
        let me = cx.this();
        let (bg, fg) = match self.axis {
            Axis::X => (crate::tbid!("TBSliderBgX"), crate::tbid!("TBSliderFgX")),
            Axis::Y => (crate::tbid!("TBSliderBgY"), crate::tbid!("TBSliderFgY")),
        };
        cx.set_skin_quiet(me, bg);
        if let Some(h) = self.handle {
            cx.set_skin_quiet(h, fg);
        }
    }

    /// TB: TBSlider::UpdateHandle
    fn update_handle(&mut self, cx: &mut Ctx) {
        let Some(h) = self.handle else { return };
        let r = cx.rect(cx.this());
        let horizontal = self.axis == Axis::X;
        let available = if horizontal { r.w } else { r.h };
        let mut hr = Rect::ZERO;
        if self.max - self.min > 0.0 {
            let ps = cx.preferred_size(h, SizeConstraints::NONE);
            let hp = if horizontal { ps.pref.w } else { ps.pref.h };
            self.to_pixel = (available - hp) as f64 / (self.max - self.min);
            let pos = ((self.value - self.min) * self.to_pixel) as i32;
            hr = if horizontal {
                Rect::new(pos, (r.h - ps.pref.h) / 2, ps.pref.w, ps.pref.h)
            } else {
                Rect::new((r.w - ps.pref.w) / 2, r.h - hp - pos, ps.pref.w, ps.pref.h)
            };
        } else {
            self.to_pixel = 0.0;
        }
        cx.set_rect(h, hr);
    }
}

impl Widget for Slider {
    fn type_name(&self) -> &'static str {
        "Slider"
    }
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_focusable(me, true);
        let h = cx.add(me, Plain);
        self.handle = Some(h);
        self.apply_axis_skin(cx);
    }
    /// TB: TBSlider::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        let axis = Axis::parse(&res.str("axis", "x"));
        self.set_axis(cx, axis);
        let me = cx.this();
        cx.set_gravity(me, if axis == Axis::X { Gravity::LEFT_RIGHT } else { Gravity::TOP_BOTTOM });
        let min = res.float("min", self.min as f32) as f64;
        let max = res.float("max", self.max as f32) as f64;
        self.set_limits(cx, min, max);
    }
    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {
        if axis != self.axis {
            self.axis = axis;
            self.apply_axis_skin(cx);
            cx.invalidate();
        }
    }
    fn axis(&self) -> Axis {
        self.axis
    }
    fn value(&self) -> Value {
        Value::Float(self.value as f32)
    }
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        self.set_value_f(cx, v.as_float() as f64);
    }
    fn arrange(&mut self, cx: &mut Ctx, _old: Size, _new: Size) {
        self.update_handle(cx);
    }
    fn measure_content(&mut self, _cx: &mut Ctx, _sc: SizeConstraints) -> crate::layout::size::PreferredSize {
        crate::layout::size::PreferredSize::default()
    }
    /// TB: TBSlider::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let step = if self.axis == Axis::X { self.small_step() } else { -self.small_step() };
        match ev.kind {
            EventKind::PointerMove if self.handle.is_some() && cx.captured() == self.handle => {
                if self.to_pixel > 0.0 {
                    let d = ev.pos - cx.pointer_down_pos();
                    let dv = if self.axis == Axis::X { d.x } else { -d.y } as f64 / self.to_pixel;
                    let v = self.value + dv;
                    self.set_value_f(cx, v);
                }
                true
            }
            EventKind::Wheel => {
                let old = self.value;
                let v = self.value + step * ev.delta.y as f64;
                self.set_value_f(cx, v);
                self.value != old
            }
            EventKind::KeyDown => {
                let v = match ev.special {
                    SpecialKey::Left | SpecialKey::Up => self.value - step,
                    SpecialKey::Right | SpecialKey::Down => self.value + step,
                    _ => return false,
                };
                self.set_value_f(cx, v);
                true
            }
            EventKind::KeyUp => {
                matches!(ev.special, SpecialKey::Left | SpecialKey::Up | SpecialKey::Right | SpecialKey::Down)
            }
            _ => false,
        }
    }
}
