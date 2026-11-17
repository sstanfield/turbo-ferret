//! Event dispatch with bubbling (section 9). TB: TBWidget::InvokeEvent

use crate::arena::WidgetId;
use crate::event::{Event, EventKind};

use super::Ui;
use super::ctx::call_hook;

impl Ui {
    /// Dispatch `ev` to `ev.target`: listeners first, then the target's `on_event`,
    /// bubbling through `event_destination` until handled. Returns true if handled.
    pub fn invoke_event(&mut self, ev: Event) -> bool {
        self.top(|ui| ui.invoke_event_inner(ev))
    }

    /// Convenience: dispatch an event of `kind` at `target`.
    pub fn send(&mut self, target: WidgetId, kind: EventKind) -> bool {
        self.invoke_event(Event::new(kind, target))
    }

    fn invoke_event_inner(&mut self, ev: Event) -> bool {
        let target = ev.target;
        if !self.tree.is_live(target) {
            return false;
        }
        // 1. Listeners (per widget, then global). Any true swallows the event.
        if self.has_listeners(target) {
            let ids = self.listener_ids(Some(target));
            if self.call_listeners(&ids, |l, ui| l.on_event(ui, target, &ev)) {
                return true;
            }
            if !self.tree.is_live(target) {
                return true; // TB: we got removed, so we handled it
            }
        }
        // 2. Value binding.
        if ev.kind == EventKind::Changed {
            self.skin_dirty = true;
            if let Some(v) = &ev.value {
                self.sync_from_widget(target, v.clone());
            }
            if !self.tree.is_live(target) {
                return true;
            }
        }
        // 3. Widget states need processing.
        if matches!(
            ev.kind,
            EventKind::Click | EventKind::LongClick | EventKind::Changed | EventKind::KeyDown | EventKind::KeyUp
        ) {
            self.states_dirty = true;
            self.skin_dirty = true;
        }
        // 4. Bubble. A busy node (its hook is running) is skipped.
        let mut cur = Some(target);
        let mut guard = 0;
        while let Some(c) = cur {
            if call_hook(self, c, |w, cx| w.on_event(cx, &ev)) == Some(true) {
                return true;
            }
            if !self.tree.is_live(c) {
                return true;
            }
            cur = self.event_destination(c);
            guard += 1;
            if guard > 4096 {
                break;
            }
        }
        false
    }

    /// Where events bubble from `id`. TB: GetEventDestination
    pub fn event_destination(&self, id: WidgetId) -> Option<WidgetId> {
        self.tree.event_destination(id)
    }

    /// `a` is `b` or on `b`'s event-destination chain. TB: IsEventDestinationFor
    pub fn is_event_destination_for(&self, a: WidgetId, b: WidgetId) -> bool {
        let mut cur = Some(b);
        let mut guard = 0;
        while let Some(c) = cur {
            if c == a {
                return true;
            }
            cur = self.event_destination(c);
            guard += 1;
            if guard > 4096 {
                break;
            }
        }
        false
    }
}
