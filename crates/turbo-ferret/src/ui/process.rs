//! `update()`: animations, kinetic scrolling, long click, messages, widget states, skin updates, layout
//! and `on_process`, in that order (section 5).
//!
//! TB: TBAnimationManager::Update, TBMessageHandler::ProcessMessages,
//! TBWidget::InvokeProcessStates, TBWidget::InvokeProcess.

use alloc::vec::Vec;

use crate::arena::WidgetId;
use crate::msg::Millis;

use super::Ui;
use super::ctx::call_hook;
use super::node::NodeFlags;

impl Ui {
    /// Advance the UI: run animations, the long-click timer and due messages, then
    /// process widget states, skin updates, pending layout and `on_process`.
    pub fn update(&mut self) {
        self.top(|ui| {
            ui.update_animations();
            ui.update_scrollers();
            ui.update_long_click();
            ui.deliver_messages();
            ui.process_states();
            ui.process_skin_updates();
            let root = ui.root();
            ui.process_node(root);
        });
    }

    /// When the host should call `update` next: `Some(now)` while animations or
    /// scrolls run or immediate messages are queued, else the earliest delayed message or long-click
    /// deadline, else `None` (wait for input). Replaces TBSystem::RescheduleTimer.
    pub fn next_wake_time(&self) -> Option<Millis> {
        let mut t: Option<Millis> = None;
        let mut min = |x: Millis| t = Some(t.map_or(x, |y| y.min(x)));
        if self.has_running_animations() || self.has_running_scrollers() {
            min(self.now);
        }
        if let Some(d) = self.msgs.next_due() {
            min(d.max(self.now));
        }
        if let Some(lc) = self.input.long_click {
            min(lc.at);
        }
        t
    }

    fn deliver_messages(&mut self) {
        let mut due = Vec::new();
        self.msgs.take_due(self.now, &mut due);
        for e in due {
            if self.tree.is_live(e.target) {
                call_hook(self, e.target, |w, cx| w.on_message(cx, &e.msg));
            }
        }
    }

    /// TB: InvokeProcessStates (only when something marked states dirty).
    fn process_states(&mut self) {
        if !core::mem::take(&mut self.states_dirty) {
            return;
        }
        let root = self.root();
        self.walk_pre(root, &mut |ui, id| {
            call_hook(ui, id, |w, cx| w.on_process_states(cx));
        });
    }

    /// Pre-order walk over a snapshot of children at each level (M-02).
    pub(crate) fn walk_pre(&mut self, id: WidgetId, f: &mut dyn FnMut(&mut Ui, WidgetId)) {
        if !self.tree.is_live(id) {
            return;
        }
        f(self, id);
        let start = self.scratch.len();
        if let Some(n) = self.tree.live(id) {
            self.scratch.extend_from_slice(&n.children);
        }
        let end = self.scratch.len();
        for i in start..end {
            let c = self.scratch[i];
            if self.tree.parent(c) == Some(id) {
                self.walk_pre(c, f);
            }
        }
        self.scratch.truncate(start);
    }

    /// Arrange pending layout, then `on_process`, children, `on_process_after_children`.
    /// TB: InvokeProcessInternal
    fn process_node(&mut self, id: WidgetId) {
        let Some(n) = self.tree.live_mut(id) else { return };
        if n.flags.contains(NodeFlags::NEEDS_ARRANGE) {
            n.flags.remove(NodeFlags::NEEDS_ARRANGE);
            let s = n.rect.size();
            call_hook(self, id, |w, cx| w.arrange(cx, s, s));
        }
        call_hook(self, id, |w, cx| w.on_process(cx));
        let start = self.scratch.len();
        if let Some(n) = self.tree.live(id) {
            self.scratch.extend_from_slice(&n.children);
        }
        let end = self.scratch.len();
        for i in start..end {
            let c = self.scratch[i];
            if self.tree.parent(c) == Some(id) {
                self.process_node(c);
            }
        }
        self.scratch.truncate(start);
        call_hook(self, id, |w, cx| w.on_process_after_children(cx));
    }
}
