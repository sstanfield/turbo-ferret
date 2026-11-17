//! Input state and the pointer, wheel, key and touch entry points (section 9).
//!
//! TB: tb_widgets.cpp (InvokePointerDown/Up/Move, InvokeWheel, InvokeKey,
//! InvokeTouch*, SetHoveredWidget, SetCapturedWidget, GetWidgetAt, ConvertFromRoot,
//! GetAutoState). TB's statics live here (H-04).

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::arena::WidgetId;
use crate::event::{Event, EventKind, Modifiers, SpecialKey};
use crate::geom::Point;
use crate::id::TbId;
use crate::msg::Millis;
use crate::widget::{HitStatus, WidgetState};

use super::Ui;
use super::ctx::call_hook;
use super::focus::FocusReason;
use super::node::NodeFlags;

#[derive(Copy, Clone, Debug)]
pub(crate) struct LongClick {
    pub widget: WidgetId,
    pub at: Millis,
    pub touch: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct TouchInfo {
    pub captured: Option<WidgetId>,
    pub hovered: Option<WidgetId>,
    pub down_pos: Point,
    pub move_pos: Point,
}

/// Keyboard click tracking (TB: function-local statics in InvokeKey).
#[derive(Copy, Clone, Debug)]
pub(crate) struct KeyClick {
    pub check_pressed: bool,
    pub had_pressed: bool,
}

impl Default for KeyClick {
    fn default() -> Self {
        KeyClick { check_pressed: true, had_pressed: false }
    }
}

/// Hovered, captured and focused widgets, touches, long click and key click state.
#[derive(Default)]
pub struct InputState {
    pub(crate) hovered: Option<WidgetId>,
    pub(crate) captured: Option<WidgetId>,
    pub(crate) focused: Option<WidgetId>,
    /// Hovered by touch: no automatic hover state. TB: no_automatic_hover_state
    pub(crate) hover_by_touch: bool,
    pub(crate) down_pos: Point,
    pub(crate) move_pos: Point,
    pub(crate) cancel_click: bool,
    pub(crate) panning: bool,
    pub(crate) show_focus: bool,
    pub(crate) long_click: Option<LongClick>,
    pub(crate) touches: BTreeMap<u32, TouchInfo>,
    pub(crate) key_click: KeyClick,
}

impl InputState {
    /// Forget every reference to widgets matching `gone` (removal fix-up).
    pub(crate) fn forget(&mut self, gone: impl Fn(WidgetId) -> bool) {
        let clear = |w: &mut Option<WidgetId>| {
            if w.is_some_and(&gone) {
                *w = None;
            }
        };
        clear(&mut self.hovered);
        if self.captured.is_some_and(&gone) {
            self.captured = None;
            self.panning = false;
        }
        clear(&mut self.focused);
        if self.long_click.is_some_and(|l| gone(l.widget)) {
            self.long_click = None;
        }
        for t in self.touches.values_mut() {
            clear(&mut t.captured);
            clear(&mut t.hovered);
        }
    }
}

impl Ui {
    pub fn hovered(&self) -> Option<WidgetId> {
        self.input.hovered
    }

    pub fn captured(&self) -> Option<WidgetId> {
        self.input.captured
    }

    /// Explicit state plus the automatic ones (pressed, hovered, focused).
    /// TB: GetAutoState
    pub fn auto_state(&self, id: WidgetId) -> WidgetState {
        let mut s = self.state(id);
        let i = &self.input;
        let pressed = !i.cancel_click && i.captured == Some(id) && i.hovered == Some(id);
        if pressed {
            s |= WidgetState::PRESSED;
        }
        if i.hovered == Some(id) && (!i.hover_by_touch || pressed) {
            s |= WidgetState::HOVERED;
        }
        if i.focused == Some(id) && i.show_focus {
            s |= WidgetState::FOCUSED;
        }
        s
    }

    /// Topmost widget under `p` (in `root`'s coordinates), not counting `root` itself.
    /// TB: GetWidgetAt(x, y, include_children = true)
    pub fn widget_at(&self, root: WidgetId, p: Point) -> Option<WidgetId> {
        let n = self.node(root)?;
        let p = p - n.behaviour().map(|b| b.child_translation()).unwrap_or_default();
        let mut last = None;
        for c in n.child_refs() {
            let cp = p - c.rect().origin();
            let status = match c.behaviour() {
                Some(b) => b.hit_test(c, cp),
                None => HitStatus::default_for(c, cp),
            };
            match status {
                HitStatus::NoHit => {}
                HitStatus::Hit => last = Some(self.widget_at(c.widget_id(), cp).unwrap_or(c.widget_id())),
                HitStatus::HitNoChildren => last = Some(c.widget_id()),
            }
        }
        last
    }

    fn hits(&self, id: WidgetId, p: Point) -> bool {
        let Some(n) = self.node(id) else { return false };
        let s = match n.behaviour() {
            Some(b) => b.hit_test(n, p),
            None => HitStatus::default_for(n, p),
        };
        s != HitStatus::NoHit
    }

    /// Root coordinates to `id`'s local coordinates. TB: ConvertFromRoot
    pub fn from_root(&self, id: WidgetId, mut p: Point) -> Point {
        let mut cur = id;
        while let Some(n) = self.node(cur) {
            let Some(parent) = n.parent() else { break };
            p -= n.rect().origin();
            if let Some(pb) = self.node(parent).and_then(|pn| pn.behaviour()) {
                p -= pb.child_translation();
            }
            cur = parent;
        }
        p
    }

    /// `id`'s local coordinates to root coordinates. TB: ConvertToRoot
    pub fn to_root(&self, id: WidgetId, mut p: Point) -> Point {
        let mut cur = id;
        while let Some(n) = self.node(cur) {
            let Some(parent) = n.parent() else { break };
            p += n.rect().origin();
            if let Some(pb) = self.node(parent).and_then(|pn| pn.behaviour()) {
                p += pb.child_translation();
            }
            cur = parent;
        }
        p
    }

    /// TB: SetHoveredWidget
    pub(crate) fn set_hovered(&mut self, w: Option<WidgetId>, touch: bool) {
        if self.input.hovered == w {
            return;
        }
        if w.is_some_and(|w| self.has_state(w, WidgetState::DISABLED)) {
            return;
        }
        self.input.hovered = w;
        self.input.hover_by_touch = touch;
        self.repaint = true;
        self.skin_dirty = true;
    }

    /// TB: SetCapturedWidget
    pub(crate) fn set_captured(&mut self, w: Option<WidgetId>) {
        if self.input.captured == w {
            return;
        }
        if w.is_some_and(|w| self.has_state(w, WidgetState::DISABLED)) {
            return;
        }
        let old = self.input.captured;
        if old.is_some() {
            self.input.panning = false;
            self.input.long_click = None;
            self.repaint = true;
            self.skin_dirty = true;
        }
        self.input.cancel_click = false;
        self.input.captured = w;
        if let Some(o) = old {
            call_hook(self, o, |b, cx| b.on_capture_changed(cx, false));
        }
        if let Some(n) = w {
            self.repaint = true;
            self.skin_dirty = true;
            call_hook(self, n, |b, cx| b.on_capture_changed(cx, true));
        }
    }

    /// Release the pointer capture if `id` holds it. TB: ReleaseCapture
    pub fn release_capture(&mut self, id: WidgetId) {
        if self.input.captured == Some(id) {
            self.top(|ui| ui.set_captured(None));
        }
    }

    /// Suppress the click that would follow the current pointer down.
    pub fn cancel_click(&mut self) {
        self.input.cancel_click = true;
        self.input.long_click = None;
    }

    // ------------------------------------------------------------------
    // Pointer
    // ------------------------------------------------------------------

    /// Pointer (mouse) button pressed at `p` (root coordinates). Returns true if a widget
    /// got it.
    pub fn pointer_down(&mut self, p: Point, clicks: u32, mods: Modifiers) -> bool {
        self.top(|ui| ui.pointer_down_inner(p, clicks, mods, false))
    }

    pub fn pointer_up(&mut self, p: Point, mods: Modifiers) -> bool {
        self.top(|ui| ui.pointer_up_inner(p, mods, false))
    }

    pub fn pointer_move(&mut self, p: Point, mods: Modifiers) {
        self.top(|ui| ui.pointer_move_inner(p, mods, false))
    }

    /// Cancel the current press (release capture without a click). TB: InvokePointerCancel
    pub fn pointer_cancel(&mut self) {
        self.top(|ui| ui.set_captured(None));
    }

    fn pointer_down_inner(&mut self, p: Point, clicks: u32, mods: Modifiers, touch: bool) -> bool {
        if self.input.captured.is_none() {
            let hit = self.widget_at(self.root(), p);
            self.set_captured(hit);
            self.set_hovered(self.input.captured, touch);
            if self.input.focused != self.input.captured {
                self.set_auto_focus_state(false);
            }
            if let Some(c) = self.input.captured {
                if touch && self.has_flag(c, NodeFlags::WANT_LONG_CLICK) {
                    let at = self.now + self.config.long_click_delay_ms as Millis;
                    self.input.long_click = Some(super::input::LongClick { widget: c, at, touch });
                }
                if let Some(win) = self.tree.parent_window(c) {
                    self.activate_window(win);
                }
            }
        }
        let Some(cap) = self.input.captured else { return false };
        if self.stop_scrollers_above(cap) {
            self.input.cancel_click = true;
        }
        let mut t = Some(cap);
        while let Some(x) = t {
            if self.set_focus_reason(x, FocusReason::Pointer) {
                break;
            }
            t = self.parent(x);
        }
        let Some(cap) = self.input.captured else { return true };
        let lp = self.from_root(cap, p);
        self.input.down_pos = lp;
        self.input.move_pos = lp;
        let mut ev = Event::new(EventKind::PointerDown, cap).at(lp);
        ev.count = clicks;
        ev.mods = mods;
        ev.touch = touch;
        self.invoke_event(ev);
        true
    }

    fn pointer_up_inner(&mut self, p: Point, mods: Modifiers, touch: bool) -> bool {
        let Some(cap) = self.input.captured else { return false };
        let lp = self.from_root(cap, p);
        let mut ev = Event::new(EventKind::PointerUp, cap).at(lp);
        ev.mods = mods;
        ev.touch = touch;
        self.invoke_event(ev);
        if !self.input.cancel_click && self.input.captured == Some(cap) && self.hits(cap, lp) {
            let mut ev = Event::new(EventKind::Click, cap).at(lp);
            ev.mods = mods;
            ev.touch = touch;
            self.invoke_event(ev);
        }
        if let Some(c) = self.input.captured {
            self.release_capture(c);
        }
        true
    }

    fn pointer_move_inner(&mut self, p: Point, mods: Modifiers, touch: bool) {
        let hit = self.widget_at(self.root(), p);
        self.set_hovered(hit, touch);
        let Some(target) = self.input.captured.or(self.input.hovered) else { return };
        let lp = self.from_root(target, p);
        self.input.move_pos = lp;
        let mut ev = Event::new(EventKind::PointerMove, target).at(lp);
        ev.mods = mods;
        ev.touch = touch;
        if self.invoke_event(ev) {
            return;
        }
        self.handle_panning(lp);
    }

    /// TB: HandlePanningOnMove (without the kinetic scroller, which comes with M5).
    fn handle_panning(&mut self, lp: Point) {
        let Some(cap) = self.input.captured else { return };
        let d = self.input.down_pos - lp;
        let th = self.config.pan_threshold;
        let start_x = d.x.abs() >= th;
        let start_y = d.y.abs() >= th;
        if !(self.input.panning || start_x || start_y) {
            return;
        }
        self.input.long_click = None;
        let mut comp = Point::ZERO;
        if !self.input.panning {
            let extra = th - 1;
            if start_x {
                comp.x = if d.x < 0 { extra } else { -extra };
            }
            if start_y {
                comp.y = if d.y < 0 { extra } else { -extra };
            }
        }
        let Some(target) = self.find_scrollable(cap, d.x != 0, d.y != 0) else { return };
        let delta = d + comp;
        let before = self.scroll_info(target).map(|i| i.pos);
        let left = self.scroll_by_recursive(target, delta);
        let after = self.scroll_info(target).map(|i| i.pos);
        if left != delta || before != after {
            self.input.panning = true;
            self.input.cancel_click = true;
            // The content moved under the pointer: re-anchor the down position.
            self.input.down_pos = lp;
        }
    }

    /// Nearest ancestor (self included) that can scroll on the given axes.
    /// TB: FindScrollableWidget
    pub fn find_scrollable(&self, id: WidgetId, x: bool, y: bool) -> Option<WidgetId> {
        self.tree
            .ancestor_ids_or_self(id)
            .find(|&c| self.scroll_info(c).is_some_and(|i| (x && i.can_scroll_x()) || (y && i.can_scroll_y())))
    }

    /// Stop kinetic scrolling above `id`; true if one was running. (Scrollers: M5.)
    fn stop_scrollers_above(&mut self, _id: WidgetId) -> bool {
        false
    }

    /// Wheel at `p` (root coordinates). Positive `delta.y` scrolls down.
    pub fn wheel(&mut self, p: Point, delta: Point, mods: Modifiers) -> bool {
        self.top(|ui| {
            let hit = ui.widget_at(ui.root(), p);
            ui.set_hovered(hit, true);
            let Some(target) = ui.input.captured.or(ui.input.hovered) else { return false };
            let lp = ui.from_root(target, p);
            ui.input.move_pos = lp;
            let mut ev = Event::new(EventKind::Wheel, target).at(lp);
            ev.delta = delta;
            ev.mods = mods;
            ev.touch = true;
            ui.invoke_event(ev);
            true
        })
    }

    // ------------------------------------------------------------------
    // Keyboard
    // ------------------------------------------------------------------

    /// A key went down or up. `ch` is the character (if any), `special` a non-character
    /// key. Returns true if handled. TB: InvokeKey
    pub fn key(&mut self, ch: Option<char>, special: SpecialKey, mods: Modifiers, down: bool) -> bool {
        self.top(|ui| ui.key_inner(ch, special, mods, down))
    }

    fn key_inner(&mut self, ch: Option<char>, special: SpecialKey, mods: Modifiers, down: bool) -> bool {
        let mut handled = false;
        if let Some(f) = self.focused() {
            let n = self.node(f).unwrap();
            let click_key = special == SpecialKey::Enter || ch == Some(' ');
            if mods.is_empty()
                && n.flags().contains(NodeFlags::CLICK_BY_KEY)
                && !n.is_disabled()
                && !n.is_dying()
                && click_key
            {
                if down && self.input.key_click.check_pressed {
                    self.input.key_click.had_pressed = self.has_state(f, WidgetState::PRESSED);
                    self.input.key_click.check_pressed = false;
                }
                if !down {
                    self.input.key_click.check_pressed = true;
                }
                if !self.input.key_click.had_pressed {
                    self.set_state(f, WidgetState::PRESSED, down);
                    if let Some(n) = self.tree.live_mut(f) {
                        n.flags.set(NodeFlags::KEY_PRESSED, down);
                    }
                }
                if !down {
                    let r = self.rect(f);
                    let mut ev = Event::new(EventKind::Click, f).at(Point::new(r.w / 2, r.h / 2));
                    ev.touch = true;
                    self.invoke_event(ev);
                }
                handled = true;
            } else {
                let mut ev = Event::new(if down { EventKind::KeyDown } else { EventKind::KeyUp }, f);
                ev.key = ch;
                ev.special = special;
                ev.mods = mods;
                handled = self.invoke_event(ev);
            }
        }
        if down && !handled && special == SpecialKey::Tab {
            handled = self.move_focus_excluding(!mods.contains(Modifiers::SHIFT), None);
            if handled {
                self.set_auto_focus_state(true);
            }
        }
        handled
    }

    /// Send a `Shortcut` event (`ref_id` = "copy", "paste", "undo" ...) to the focused
    /// widget. The host decides which key combinations are shortcuts, as in TB.
    pub fn shortcut(&mut self, ref_id: TbId, mods: Modifiers) -> bool {
        self.top(|ui| {
            let Some(f) = ui.focused() else { return false };
            let mut ev = Event::new(EventKind::Shortcut, f);
            ev.ref_id = ref_id;
            ev.mods = mods;
            ui.invoke_event(ev)
        })
    }

    /// Files dropped at `p` (root coordinates). The event's payload is a `Vec<String>`.
    pub fn file_drop(&mut self, p: Point, files: Vec<String>) -> bool {
        self.top(|ui| {
            let Some(target) = ui.widget_at(ui.root(), p).or(ui.focused()) else { return false };
            let mut ev = Event::new(EventKind::FileDrop, target).at(ui.from_root(target, p));
            ev.payload = Some(Rc::new(files));
            ui.invoke_event(ev)
        })
    }

    // ------------------------------------------------------------------
    // Touch
    // ------------------------------------------------------------------

    /// Touch 0 maps to the pointer path; other touches send `Touch*` events.
    pub fn touch_down(&mut self, touch: u32, p: Point, clicks: u32, mods: Modifiers) -> bool {
        if touch == 0 {
            return self.top(|ui| ui.pointer_down_inner(p, clicks, mods, true));
        }
        self.top(|ui| {
            let captured = ui.widget_at(ui.root(), p);
            let mut ti = TouchInfo { captured, ..Default::default() };
            if let Some(c) = captured {
                if !ui.has_state(c, WidgetState::DISABLED) {
                    ti.hovered = Some(c);
                }
                let lp = ui.from_root(c, p);
                ti.down_pos = lp;
                ti.move_pos = lp;
            }
            ui.input.touches.insert(touch, ti);
            let Some(c) = captured else { return false };
            let mut ev = Event::new(EventKind::TouchDown, c).at(ti.down_pos);
            ev.count = clicks;
            ev.mods = mods;
            ev.touch = true;
            ev.ref_id = TbId(touch);
            ui.invoke_event(ev);
            true
        })
    }

    pub fn touch_up(&mut self, touch: u32, p: Point, mods: Modifiers) -> bool {
        if touch == 0 {
            return self.top(|ui| ui.pointer_up_inner(p, mods, true));
        }
        self.top(|ui| {
            let Some(ti) = ui.input.touches.get(&touch).copied() else { return false };
            let Some(c) = ti.captured else { return false };
            let mut ev = Event::new(EventKind::TouchUp, c).at(ui.from_root(c, p));
            ev.mods = mods;
            ev.touch = true;
            ev.ref_id = TbId(touch);
            ui.invoke_event(ev);
            ui.input.touches.remove(&touch);
            true
        })
    }

    pub fn touch_move(&mut self, touch: u32, p: Point, mods: Modifiers) {
        if touch == 0 {
            return self.top(|ui| ui.pointer_move_inner(p, mods, true));
        }
        self.top(|ui| {
            let hovered = ui.widget_at(ui.root(), p);
            let Some(ti) = ui.input.touches.get_mut(&touch) else { return };
            ti.hovered = hovered;
            let Some(c) = ti.captured else { return };
            let lp = ui.from_root(c, p);
            if let Some(ti) = ui.input.touches.get_mut(&touch) {
                ti.move_pos = lp;
            }
            let mut ev = Event::new(EventKind::TouchMove, c).at(lp);
            ev.mods = mods;
            ev.touch = true;
            ev.ref_id = TbId(touch);
            ui.invoke_event(ev);
        })
    }

    pub fn touch_cancel(&mut self, touch: u32) {
        if touch == 0 {
            return self.pointer_cancel();
        }
        self.top(|ui| {
            if let Some(ti) = ui.input.touches.remove(&touch) {
                if let Some(c) = ti.captured {
                    let mut ev = Event::new(EventKind::TouchCancel, c);
                    ev.touch = true;
                    ev.ref_id = TbId(touch);
                    ui.invoke_event(ev);
                }
            }
        })
    }

    /// Check the long-click deadline (called by `update`).
    /// TB: TBLongClickTimer + MaybeInvokeLongClickOrContextMenu
    pub(crate) fn update_long_click(&mut self) {
        let Some(lc) = self.input.long_click else { return };
        if self.now < lc.at {
            return;
        }
        self.input.long_click = None;
        let w = lc.widget;
        if self.input.captured == Some(w) && !self.input.cancel_click && self.hits(w, self.input.move_pos) {
            let mut ev = Event::new(EventKind::LongClick, w).at(self.input.move_pos);
            ev.touch = lc.touch;
            let mut handled = self.invoke_event(ev.clone());
            if !handled {
                ev.kind = EventKind::ContextMenu;
                handled = self.invoke_event(ev);
            }
            if handled {
                self.input.cancel_click = true;
            }
        }
    }
}
