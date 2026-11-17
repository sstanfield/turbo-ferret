//! External callbacks: global and per-widget listeners (port-design.md section 7).
//!
//! TB: tb_widgets_listener.cpp. Listener boxes live in an arena; calling one takes it
//! out of its slot and puts it back afterwards (M-05), so listeners may add or remove
//! listeners (and widgets) freely.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::arena::{Arena, WidgetId, arena_key};
use crate::event::Event;

use super::Ui;

arena_key!(
    /// Handle of a registered listener.
    ListenerId
);

/// A listener for tree, focus and event notifications. Methods get `&mut Ui` and IDs;
/// reach a behaviour with `ui.with_widget`.
#[allow(unused_variables)]
pub trait UiListener {
    /// `child` was added to `parent` (per-widget listeners: on the parent).
    fn on_added(&mut self, ui: &mut Ui, parent: WidgetId, child: WidgetId) {}
    /// `child` is about to be removed from `parent` (per-widget listeners: on the parent).
    fn on_removing(&mut self, ui: &mut Ui, parent: Option<WidgetId>, child: WidgetId) {}
    /// `id` is dying. Return true to keep it alive (the listener removes it later).
    fn on_dying(&mut self, ui: &mut Ui, id: WidgetId) -> bool {
        false
    }
    fn on_focus_changed(&mut self, ui: &mut Ui, id: WidgetId, focused: bool) {}
    /// An event is about to be dispatched to `target`. Return true to swallow it.
    fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool {
        false
    }
}

/// Closure-based per-widget event handler (`Ui::on_event`).
pub(crate) struct EventFn<F>(pub F);

impl<F: FnMut(&mut Ui, &Event) -> bool> UiListener for EventFn<F> {
    fn on_event(&mut self, ui: &mut Ui, _target: WidgetId, ev: &Event) -> bool {
        (self.0)(ui, ev)
    }
}

#[derive(Default)]
pub(crate) struct Listeners {
    pub arena: Arena<Option<Box<dyn UiListener>>, ListenerId>,
    pub global: Vec<ListenerId>,
}

impl Ui {
    /// Add a global listener.
    pub fn add_listener(&mut self, l: Box<dyn UiListener>) -> ListenerId {
        let id = self.listeners.arena.insert(Some(l));
        self.listeners.global.push(id);
        id
    }

    /// Add a listener for one widget (events targeting it, children added or removed,
    /// its focus changes and dying).
    pub fn add_widget_listener(&mut self, widget: WidgetId, l: Box<dyn UiListener>) -> Option<ListenerId> {
        if !self.tree.is_live(widget) {
            return None;
        }
        let id = self.listeners.arena.insert(Some(l));
        let arena = &self.listeners.arena;
        let list = self.tree.widget_listeners.get_or_insert_with(widget, Vec::new);
        // Listeners removed since are still listed here: drop them now, so a widget
        // whose listeners come and go does not collect them.
        list.retain(|&i| arena.contains(i));
        list.push(id);
        Some(id)
    }

    /// Per-widget event handler closure. Return true to swallow the event.
    pub fn on_event(
        &mut self,
        widget: WidgetId,
        f: impl FnMut(&mut Ui, &Event) -> bool + 'static,
    ) -> Option<ListenerId> {
        self.add_widget_listener(widget, Box::new(EventFn(f)))
    }

    pub fn remove_listener(&mut self, id: ListenerId) {
        self.listeners.arena.remove(id);
        self.listeners.global.retain(|&g| g != id);
        // Per-widget lists drop stale ids when a listener is next added to them
        // (lookups of stale ids fail).
    }

    /// Call `f` on each listener in `ids` (taken out of its slot during the call).
    /// Returns the OR of the results.
    pub(crate) fn call_listeners(
        &mut self,
        ids: &[ListenerId],
        mut f: impl FnMut(&mut dyn UiListener, &mut Ui) -> bool,
    ) -> bool {
        let mut any = false;
        for &lid in ids {
            let Some(slot) = self.listeners.arena.get_mut(lid) else { continue };
            let Some(mut l) = slot.take() else { continue }; // busy (re-entrant)
            self.depth += 1;
            any |= f(&mut *l, self);
            self.depth -= 1;
            if let Some(slot) = self.listeners.arena.get_mut(lid) {
                *slot = Some(l);
            }
        }
        if self.depth == 0 {
            self.flush();
        }
        any
    }

    /// Listener ids for a widget (local first) followed by global ones.
    pub(crate) fn listener_ids(&self, widget: Option<WidgetId>) -> Vec<ListenerId> {
        let mut v = Vec::new();
        if let Some(l) = widget.and_then(|w| self.tree.widget_listeners.get(w)) {
            v.extend(l.iter().copied().filter(|&i| self.listeners.arena.contains(i)));
        }
        v.extend_from_slice(&self.listeners.global);
        v
    }

    pub(crate) fn has_listeners(&self, widget: WidgetId) -> bool {
        !self.listeners.global.is_empty() || self.tree.widget_listeners.get(widget).is_some_and(|l| !l.is_empty())
    }
}
