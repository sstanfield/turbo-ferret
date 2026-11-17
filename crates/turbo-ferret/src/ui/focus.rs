//! Focus, windows and activation, Tab navigation, scrolling into view.
//!
//! TB: tb_widgets.cpp (SetFocus, SetFocusRecursive, MoveFocus, ScrollIntoView*),
//! tb_window.cpp (Activate, EnsureFocus, DeActivate, GetTopMostOtherWindow).

use crate::arena::WidgetId;
use crate::geom::{Point, Rect};
use crate::widget::{WidgetState, Z};

use super::Ui;
use super::ctx::call_hook;
use super::node::NodeFlags;

/// Why focus is being set. TB: WIDGET_FOCUS_REASON
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FocusReason {
    /// Keyboard navigation: scrolls the widget into view.
    Navigation,
    Pointer,
    Unknown,
}

impl Ui {
    pub fn focused(&self) -> Option<WidgetId> {
        self.input.focused.filter(|&f| self.tree.is_live(f))
    }

    /// Focus a widget. Refused if it is not focusable, disabled, invisible or dying.
    /// In an inactive window, focus is recorded and applied when the window activates.
    pub fn set_focus(&mut self, id: WidgetId) -> bool {
        self.top(|ui| ui.set_focus_reason(id, FocusReason::Unknown))
    }

    pub fn set_focus_with(&mut self, id: WidgetId, reason: FocusReason) -> bool {
        self.top(|ui| ui.set_focus_reason(id, reason))
    }

    /// TB: TBWidget::SetFocus
    pub(crate) fn set_focus_reason(&mut self, id: WidgetId, reason: FocusReason) -> bool {
        if self.input.focused == Some(id) {
            return true;
        }
        let Some(n) = self.node(id) else { return false };
        if n.is_disabled() || !n.is_focusable() || !n.is_visible_combined() || n.is_dying() {
            return false;
        }
        if let Some(win) = self.tree.parent_window(id) {
            self.tree.last_focus.insert(win, id);
            let wn = self.tree.live(win).unwrap();
            if !wn.flags.contains(NodeFlags::ACTIVE_WINDOW) && wn.flags.contains(NodeFlags::CAN_ACTIVATE) {
                return true;
            }
        }
        let old = self.input.focused.replace(id);
        self.repaint = true;
        self.skin_dirty = true;
        if reason == FocusReason::Navigation {
            self.scroll_into_view_recursive(id);
        }
        if let Some(o) = old.filter(|&o| self.tree.is_live(o)) {
            if self.has_flag(o, NodeFlags::KEY_PRESSED) {
                self.set_state(o, WidgetState::PRESSED, false);
                if let Some(n) = self.tree.live_mut(o) {
                    n.flags.remove(NodeFlags::KEY_PRESSED);
                }
            }
            call_hook(self, o, |w, cx| w.on_focus_changed(cx, false));
            self.notify_focus(o, false);
        }
        if self.input.focused == Some(id) {
            call_hook(self, id, |w, cx| w.on_focus_changed(cx, true));
        }
        if self.input.focused == Some(id) {
            self.notify_focus(id, true);
        }
        true
    }

    fn notify_focus(&mut self, id: WidgetId, focused: bool) {
        let ids = self.listener_ids(Some(id));
        if !ids.is_empty() {
            self.call_listeners(&ids, |l, ui| {
                l.on_focus_changed(ui, id, focused);
                false
            });
        }
    }

    /// Focus the first focusable descendant. TB: SetFocusRecursive
    pub fn set_focus_recursive(&mut self, id: WidgetId) -> bool {
        let mut cur = self.tree.live(id).and_then(|n| n.children.first().copied());
        while let Some(c) = cur {
            if self.set_focus_reason(c, FocusReason::Unknown) {
                return true;
            }
            cur = self.tree.next_deep(c, Some(id));
        }
        false
    }

    /// Move focus to the next (or previous) focusable widget in the focused window.
    /// TB: MoveFocus
    pub fn move_focus(&mut self, forward: bool) -> bool {
        self.top(|ui| ui.move_focus_excluding(forward, None))
    }

    pub(crate) fn move_focus_excluding(&mut self, forward: bool, exclude: Option<WidgetId>) -> bool {
        let origin = self.focused().unwrap_or(self.root());
        let root = self.tree.parent_window(origin).unwrap_or_else(|| self.tree.parent_root(origin));
        let mut cur = origin;
        let limit = self.tree.nodes.len() * 2 + 4;
        for _ in 0..limit {
            let next = if forward { self.tree.next_deep(cur, Some(root)) } else { self.tree.prev_deep(cur) };
            cur = match next {
                Some(n) if self.tree.is_ancestor_of(root, n) => n,
                _ if forward => root,
                _ => self.tree.last_leaf(root).unwrap_or(root),
            };
            if cur == origin {
                break;
            }
            if exclude.is_some_and(|e| self.tree.is_ancestor_of(e, cur)) {
                continue;
            }
            if self.set_focus_reason(cur, FocusReason::Navigation) {
                return true;
            }
        }
        false
    }

    /// Show the focus state on the focused widget (set by Tab navigation, cleared by
    /// pointer down elsewhere). TB: SetAutoFocusState
    pub fn set_auto_focus_state(&mut self, on: bool) {
        if self.input.show_focus != on {
            self.input.show_focus = on;
            self.repaint = true;
            self.skin_dirty = true;
        }
    }

    // ------------------------------------------------------------------
    // Windows
    // ------------------------------------------------------------------

    pub fn is_active_window(&self, id: WidgetId) -> bool {
        self.has_flag(id, NodeFlags::ACTIVE_WINDOW)
    }

    /// Bring a window to the top, make it active and restore its focus. TB: TBWindow::Activate
    pub fn activate_window(&mut self, win: WidgetId) {
        let Some(n) = self.tree.live(win) else { return };
        if !n.flags.contains(NodeFlags::WINDOW) || n.parent.is_none() || !n.flags.contains(NodeFlags::CAN_ACTIVATE) {
            return;
        }
        if n.flags.contains(NodeFlags::ACTIVE_WINDOW) {
            self.ensure_focus(win);
            return;
        }
        if let Some(active) = self.topmost_other_window(win, true) {
            self.set_window_active(active, false);
        }
        self.set_z(win, Z::Top);
        self.set_window_active(win, true);
        self.ensure_focus(win);
    }

    /// TB: TBWindow::SetWindowActiveState (the SELECTED state is what skins see).
    pub(crate) fn set_window_active(&mut self, win: WidgetId, active: bool) {
        let Some(n) = self.tree.live_mut(win) else { return };
        if n.flags.contains(NodeFlags::ACTIVE_WINDOW) == active {
            return;
        }
        n.flags.set(NodeFlags::ACTIVE_WINDOW, active);
        self.set_state(win, WidgetState::SELECTED, active);
        call_hook(self, win, |w, cx| w.on_window_active_changed(cx, active));
    }

    /// TB: TBWindow::EnsureFocus
    pub fn ensure_focus(&mut self, win: WidgetId) -> bool {
        if self.focused().is_some_and(|f| self.tree.is_ancestor_of(win, f)) {
            return true;
        }
        let mut ok = false;
        if let Some(last) = self.tree.last_focus.get(win).copied() {
            ok = self.set_focus_reason(last, FocusReason::Unknown);
        }
        if !ok {
            ok = self.set_focus_recursive(win);
        }
        ok
    }

    /// TB: TBWindow::GetTopMostOtherWindow
    pub(crate) fn topmost_other_window(&self, win: WidgetId, only_activable: bool) -> Option<WidgetId> {
        let p = self.tree.parent(win)?;
        self.tree.live(p)?.children.iter().rev().copied().find(|&s| {
            s != win
                && self.tree.live(s).is_some_and(|n| {
                    n.flags.contains(NodeFlags::WINDOW)
                        && (!only_activable || n.flags.contains(NodeFlags::CAN_ACTIVATE))
                })
        })
    }

    // ------------------------------------------------------------------
    // Scrolling
    // ------------------------------------------------------------------

    /// Scroll a scrollable widget to a position (clamped by the widget).
    pub fn scroll_to(&mut self, id: WidgetId, p: Point) {
        self.with_behaviour(id, |w, cx| w.scroll_to(cx, p));
    }

    /// TB: ScrollBy
    pub fn scroll_by(&mut self, id: WidgetId, d: Point) {
        if let Some(info) = self.node(id).and_then(|n| n.behaviour()).and_then(|b| b.scroll_info()) {
            self.scroll_to(id, info.pos + d);
        }
    }

    /// Scroll `id` and then its ancestors by `d` until it is used up. Returns what is
    /// left. TB: ScrollByRecursive
    pub fn scroll_by_recursive(&mut self, id: WidgetId, mut d: Point) -> Point {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if let Some(old) = self.scroll_info(c) {
                self.scroll_to(c, old.pos + d);
                let new = self.scroll_info(c).unwrap_or(old);
                d -= new.pos - old.pos;
                if d == Point::ZERO {
                    break;
                }
            }
            cur = self.parent(c);
        }
        d
    }

    pub fn scroll_info(&self, id: WidgetId) -> Option<crate::widget::ScrollInfo> {
        self.node(id)?.behaviour()?.scroll_info()
    }

    /// Scroll ancestors so `id` is visible. TB: ScrollIntoViewRecursive
    pub fn scroll_into_view_recursive(&mut self, id: WidgetId) {
        let mut r = self.rect(id);
        let mut cur = id;
        while let Some(p) = self.parent(cur) {
            self.scroll_into_view(p, r);
            let pr = self.rect(p);
            r.x += pr.x;
            r.y += pr.y;
            cur = p;
        }
    }

    /// TB: ScrollIntoView
    pub fn scroll_into_view(&mut self, id: WidgetId, rect: Rect) {
        let Some(info) = self.scroll_info(id) else { return };
        let vis = self.padding_rect(id).offset_by(info.pos);
        let mut np = info.pos;
        if rect.y <= vis.y {
            np.y = rect.y;
        } else if rect.bottom() > vis.bottom() {
            np.y = rect.bottom() - vis.h;
        }
        if rect.x <= vis.x {
            np.x = rect.x;
        } else if rect.right() > vis.right() {
            np.x = rect.right() - vis.w;
        }
        if np != info.pos {
            self.scroll_to(id, np);
        }
    }
}
