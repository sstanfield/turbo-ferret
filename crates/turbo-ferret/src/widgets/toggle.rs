//! `ToggleContainer`, `SectionHeader` and `Section`.
//! TB: tb_toggle_container.cpp (TBToggleContainer, TBSectionHeader, TBSection)

use alloc::string::String;

use crate::arena::WidgetId;
use crate::event::{Event, EventKind};
use crate::geom::{Point, Size};
use crate::layout::size::{Gravity, PreferredSize, SizeConstraints, Visibility};
use crate::layout::stack::LayoutSize;
use crate::msg::Message;
use crate::res::ResNode;
use crate::ui::{Ctx, NodeRef};
use crate::value::Value;
use crate::widget::{Axis, HitStatus, Widget, WidgetState};

use super::{Button, Layout};

/// What a `ToggleContainer` toggles when its value changes.
/// TB: TBToggleContainer::TOGGLE
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Toggle {
    /// Nothing happens (the default).
    #[default]
    Nothing,
    /// Enabled or disabled state.
    Enabled,
    /// Opacity 1 or 0.
    Opacity,
    /// Expanded or collapsed (visible or gone).
    Expanded,
}

impl Toggle {
    /// TB: TBToggleContainer::OnInflate ("toggle", matched anywhere, ignoring case)
    pub fn parse(s: &str) -> Option<Toggle> {
        let s = s.to_ascii_lowercase();
        if s.contains("enabled") {
            Some(Toggle::Enabled)
        } else if s.contains("opacity") {
            Some(Toggle::Opacity)
        } else if s.contains("expanded") {
            Some(Toggle::Expanded)
        } else {
            None
        }
    }
}

/// A container that toggles a property of itself when its value changes between 0
/// and 1, for example to show a group of widgets depending on a check box (connect
/// both to the same value). TB: TBToggleContainer
#[derive(Default)]
pub struct ToggleContainer {
    toggle: Toggle,
    invert: bool,
    value: i32,
}

impl ToggleContainer {
    pub fn new(toggle: Toggle) -> ToggleContainer {
        ToggleContainer { toggle, ..Default::default() }
    }

    pub fn toggle(&self) -> Toggle {
        self.toggle
    }

    /// TB: TBToggleContainer::SetToggle
    pub fn set_toggle(&mut self, cx: &mut Ctx, toggle: Toggle) {
        if toggle == self.toggle {
            return;
        }
        if self.toggle == Toggle::Expanded {
            let me = cx.this();
            cx.invalidate_layout(me);
        }
        self.toggle = toggle;
        self.update(cx);
    }

    pub fn invert(&self) -> bool {
        self.invert
    }

    /// TB: TBToggleContainer::SetInvert
    pub fn set_invert(&mut self, cx: &mut Ctx, invert: bool) {
        if invert != self.invert {
            self.invert = invert;
            self.update(cx);
        }
    }

    /// The value after the invert setting. TB: GetIsOn
    pub fn is_on(&self) -> bool {
        (self.value != 0) != self.invert
    }

    /// TB: TBToggleContainer::UpdateInternal
    fn update(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        let on = self.is_on();
        match self.toggle {
            Toggle::Nothing => {}
            Toggle::Enabled => cx.set_state(me, WidgetState::DISABLED, !on),
            Toggle::Opacity => cx.set_opacity(me, if on { 1.0 } else { 0.0 }),
            Toggle::Expanded => {
                cx.set_visibility(me, if on { Visibility::Visible } else { Visibility::Gone });
                // Also disable when collapsed, so Tab focus skips the children.
                cx.set_state(me, WidgetState::DISABLED, !on);
            }
        }
    }
}

impl Widget for ToggleContainer {
    fn type_name(&self) -> &'static str {
        "ToggleContainer"
    }

    /// TB: TBToggleContainer::TBToggleContainer
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBToggleContainer"));
        self.update(cx);
    }

    /// TB: TBToggleContainer::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        if let Some(t) = res.get("toggle").and_then(|n| Toggle::parse(&n.value.as_str())) {
            self.set_toggle(cx, t);
        }
        let invert = res.int("invert", self.invert as i32) != 0;
        self.set_invert(cx, invert);
    }

    fn value(&self) -> Value {
        Value::Int(self.value)
    }

    /// TB: TBToggleContainer::SetValue
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        let v = v.as_int();
        if v != self.value {
            self.value = v;
            self.update(cx);
            cx.invalidate_skin_states();
        }
    }
}

/// The clickable header of a `Section`: a toggle button with the "TBSectionHeader"
/// skin. TB: TBSectionHeader (a TBButton subclass; here it wraps a `Button`)
///
/// TB's header told its section about a change from its own OnEvent. A widget does
/// not see events it fires from inside its own hook here, so `Section` listens for
/// the header's `Changed` event instead.
#[derive(Default)]
pub struct SectionHeader(Button);

impl SectionHeader {
    pub fn new(text: &str) -> SectionHeader {
        SectionHeader(Button::new(text))
    }

    pub fn button(&self) -> &Button {
        &self.0
    }
}

impl Widget for SectionHeader {
    fn type_name(&self) -> &'static str {
        "SectionHeader"
    }

    /// TB: TBSectionHeader::TBSectionHeader
    fn on_create(&mut self, cx: &mut Ctx) {
        self.0.on_create(cx);
        self.0.toggle_mode = true;
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBSectionHeader"));
        cx.set_gravity(me, Gravity::LEFT_RIGHT);
    }
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        self.0.on_inflate(cx, res)
    }
    fn content_root(&self) -> Option<WidgetId> {
        self.0.content_root()
    }
    fn arrange(&mut self, cx: &mut Ctx, old: Size, new: Size) {
        self.0.arrange(cx, old, new)
    }
    fn on_skin_changed(&mut self, cx: &mut Ctx) {
        self.0.on_skin_changed(cx)
    }
    fn on_process(&mut self, cx: &mut Ctx) {
        self.0.on_process(cx)
    }
    fn hit_test(&self, this: NodeRef, p: Point) -> HitStatus {
        self.0.hit_test(this, p)
    }
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        self.0.on_event(cx, ev)
    }
    fn on_capture_changed(&mut self, cx: &mut Ctx, captured: bool) {
        self.0.on_capture_changed(cx, captured)
    }
    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {
        self.0.on_message(cx, msg)
    }
    fn value(&self) -> Value {
        self.0.value()
    }
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        self.0.set_value(cx, v)
    }
    fn text(&self) -> Option<&str> {
        self.0.text()
    }
    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        self.0.set_text(cx, text)
    }
    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {
        self.0.set_axis(cx, axis)
    }
    fn axis(&self) -> Axis {
        self.0.axis()
    }
}

/// A header that expands and collapses the children below it when clicked.
///
/// Skins of the internal widgets: "TBSection" (this widget), "TBSection.layout" (the
/// layout around the header and the container) and "TBSection.container" (the
/// toggle container with the children). TB: TBSection
#[derive(Default)]
pub struct Section {
    title: String,
    value: i32,
    layout: Option<WidgetId>,
    header: Option<WidgetId>,
    container: Option<WidgetId>,
    pending_scroll: bool,
}

impl Section {
    pub fn new(title: &str) -> Section {
        Section { title: String::from(title), ..Default::default() }
    }

    pub fn layout(&self) -> Option<WidgetId> {
        self.layout
    }

    pub fn header(&self) -> Option<WidgetId> {
        self.header
    }

    /// The toggle container that holds the children.
    pub fn container(&self) -> Option<WidgetId> {
        self.container
    }

    pub fn is_expanded(&self) -> bool {
        self.value != 0
    }

    /// Scroll the section into view after the next layout.
    /// TB: SetPendingScrollIntoView
    pub fn set_pending_scroll_into_view(&mut self, pending: bool) {
        self.pending_scroll = pending;
    }
}

impl Widget for Section {
    fn type_name(&self) -> &'static str {
        "Section"
    }

    /// TB: TBSection::TBSection
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_gravity(me, Gravity::LEFT_RIGHT);
        cx.set_skin_quiet(me, crate::tbid!("TBSection"));
        let mut layout = Layout::new(Axis::Y);
        layout.size = LayoutSize::Available;
        let layout = cx.add(me, layout);
        cx.set_skin_quiet(layout, crate::tbid!("TBSection.layout"));
        cx.set_gravity(layout, Gravity::ALL);
        let header = cx.add(layout, SectionHeader::new(&self.title));
        let container = cx.add(layout, ToggleContainer::new(Toggle::Expanded));
        cx.set_skin(container, crate::tbid!("TBSection.container"));
        cx.set_gravity(container, Gravity::ALL);
        self.layout = Some(layout);
        self.header = Some(header);
        self.container = Some(container);
    }

    fn content_root(&self) -> Option<WidgetId> {
        self.container
    }

    fn value(&self) -> Value {
        Value::Int(self.value)
    }

    /// TB: TBSection::SetValue
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        let old = self.value;
        self.value = v.as_int();
        if self.value != old {
            // TB set this from the header's change, which this section does not
            // hear while it is in this hook.
            self.pending_scroll = self.value != 0;
        }
        if let Some(h) = self.header {
            cx.set_value(h, self.value);
        }
        if let Some(c) = self.container {
            cx.set_value(c, self.value);
        }
    }

    fn text(&self) -> Option<&str> {
        Some(&self.title)
    }

    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        self.title = String::from(text);
        if let Some(h) = self.header {
            cx.set_text(h, text);
        }
    }

    /// The header was toggled: expand or collapse, and scroll into view when
    /// expanded. TB: TBSectionHeader::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if ev.kind == EventKind::Changed && Some(ev.target) == self.header {
            // The header is busy firing this event: the value comes with it.
            self.value = ev.value.as_ref().map_or(0, |v| v.as_int());
            if let Some(c) = self.container {
                cx.set_value(c, self.value);
            }
            self.pending_scroll = self.value != 0;
        }
        false
    }

    /// TB: TBSection::OnProcessAfterChildren
    fn on_process_after_children(&mut self, cx: &mut Ctx) {
        if core::mem::take(&mut self.pending_scroll) {
            let me = cx.this();
            cx.scroll_into_view_recursive(me);
        }
    }

    fn measure_adds_skin(&self) -> bool {
        false
    }

    /// TB: TBSection::OnCalculatePreferredSize
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        let mut ps = cx.default_measure_content(sc);
        // Do not grow when there is extra space.
        ps.max.h = ps.pref.h;
        ps
    }
}
