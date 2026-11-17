//! Changing the tree: create, attach, z-order, remove, die, flush (section 8), and
//! layout invalidation and measurement (section 12).
//!
//! TB: tb_widgets.cpp (AddChildRelative, RemoveChild, SetZ, Die, InvalidateLayout,
//! GetPreferredSize).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::arena::WidgetId;
use crate::layout::size::{PreferredSize, SizeConstraints, Visibility};
use crate::widget::{DieAction, Widget, Z, ZRel};

use super::Ui;
use super::ctx::call_hook;
use super::node::{Node, NodeFlags};

impl Ui {
    // ------------------------------------------------------------------
    // Creating and attaching (immediate)
    // ------------------------------------------------------------------

    /// Create an unattached node (runs `on_create`). Attach it later, or `remove` it.
    pub fn create<W: Widget>(&mut self, w: W) -> WidgetId {
        self.create_boxed(Box::new(w))
    }

    pub fn create_boxed(&mut self, b: Box<dyn Widget>) -> WidgetId {
        let id = self.tree.nodes.insert(Node::new(b));
        call_hook(self, id, |w, cx| w.on_create(cx));
        id
    }

    /// Create and attach on top of `parent`'s content root. The common case.
    pub fn add<W: Widget>(&mut self, parent: WidgetId, w: W) -> WidgetId {
        self.add_at(parent, w, Z::Top)
    }

    /// If `parent` is gone (a stale id, or a widget that was removed), the new
    /// widget is removed again at once: the id returned is then not live.
    pub fn add_at<W: Widget>(&mut self, parent: WidgetId, w: W, z: Z) -> WidgetId {
        let id = self.create(w);
        if !self.attach(parent, id, z) {
            self.remove(id);
        }
        id
    }

    pub fn add_boxed(&mut self, parent: WidgetId, b: Box<dyn Widget>, z: Z) -> WidgetId {
        let id = self.create_boxed(b);
        if !self.attach(parent, id, z) {
            self.remove(id);
        }
        id
    }

    /// Create and attach before or after `reference` (same parent).
    pub fn add_relative<W: Widget>(&mut self, reference: WidgetId, w: W, rel: ZRel) -> WidgetId {
        let id = self.create(w);
        if !self.attach_relative(reference, id, rel) {
            self.remove(id);
        }
        id
    }

    /// Attach an unattached node to `parent`'s content root. TB: AddChild
    pub fn attach(&mut self, parent: WidgetId, child: WidgetId, z: Z) -> bool {
        let parent = self.content_root(parent);
        let Some(p) = self.tree.live(parent) else { return false };
        let index = match z {
            Z::Top => p.children.len(),
            Z::Bottom => 0,
        };
        self.attach_at(parent, child, index)
    }

    /// TB: AddChildRelative
    pub fn attach_relative(&mut self, reference: WidgetId, child: WidgetId, rel: ZRel) -> bool {
        let Some(parent) = self.tree.parent(reference) else { return false };
        let Some(i) = self.tree.index_in_parent(reference) else { return false };
        let index = match rel {
            ZRel::Before => i,
            ZRel::After => i + 1,
        };
        self.attach_at(parent, child, index)
    }

    fn attach_at(&mut self, parent: WidgetId, child: WidgetId, index: usize) -> bool {
        let ok = self.tree.is_live(parent)
            && self.tree.live(child).is_some_and(|c| c.parent.is_none())
            && child != self.root()
            && !self.tree.is_ancestor_of(child, parent);
        if !ok {
            return false;
        }
        self.top(|ui| {
            let p = ui.tree.live_mut(parent).unwrap();
            let index = index.min(p.children.len());
            p.children.insert(index, child);
            ui.tree.live_mut(child).unwrap().parent = Some(parent);
            call_hook(ui, parent, |w, cx| w.on_child_added(cx, child));
            call_hook(ui, child, |w, cx| w.on_added(cx));
            let ids = ui.listener_ids(Some(parent));
            if !ids.is_empty() {
                ui.call_listeners(&ids, |l, ui| {
                    l.on_added(ui, parent, child);
                    false
                });
            }
            // TB: TBWindow::OnAdded activates a window added on top.
            if ui.has_flag(child, NodeFlags::WINDOW)
                && ui.tree.live(parent).and_then(|p| p.children.last()) == Some(&child)
            {
                ui.activate_window(child);
            }
            ui.invalidate_layout(parent);
            ui.repaint = true;
            ui.skin_dirty = true;
        });
        true
    }

    /// Move to the top or bottom of its siblings (no callbacks). TB: SetZ
    pub fn set_z(&mut self, id: WidgetId, z: Z) {
        let Some(parent) = self.tree.parent(id) else { return };
        let p = self.tree.live_mut(parent).unwrap();
        let Some(i) = p.children.iter().position(|&c| c == id) else { return };
        p.children.remove(i);
        match z {
            Z::Top => p.children.push(id),
            Z::Bottom => p.children.insert(0, id),
        }
        self.invalidate_layout(parent);
        self.repaint = true;
        self.skin_dirty = true;
    }

    // ------------------------------------------------------------------
    // Removing (detach now, free later)
    // ------------------------------------------------------------------

    /// Remove a widget and its subtree. It is detached immediately (lookups of its id
    /// fail from now on) and freed at the end of the current top-level call.
    pub fn remove(&mut self, id: WidgetId) {
        self.top(|ui| ui.remove_inner(id));
    }

    pub(crate) fn remove_inner(&mut self, id: WidgetId) {
        if id == self.root() || !self.tree.is_live(id) {
            return;
        }
        let parent = self.tree.parent(id);

        // 1. Notifications (listeners, own hook, parent hook).
        let ids = self.listener_ids(parent);
        if !ids.is_empty() {
            self.call_listeners(&ids, |l, ui| {
                l.on_removing(ui, parent, id);
                false
            });
        }
        if !self.tree.is_live(id) {
            return;
        }
        call_hook(self, id, |w, cx| w.on_removed(cx));
        if let Some(p) = parent {
            call_hook(self, p, |w, cx| w.on_child_removed(cx, id));
        }
        if !self.tree.is_live(id) {
            return;
        }
        // The widgets below it go too. TB: ~TBWidget removed each child in turn,
        // which called its OnRemove.
        let mut below = Vec::new();
        self.tree.collect_subtree(id, &mut below);
        for w in below.into_iter().filter(|&w| w != id) {
            if self.tree.is_live(w) {
                call_hook(self, w, |b, cx| b.on_removed(cx));
            }
        }
        if !self.tree.is_live(id) {
            return;
        }

        // Window bookkeeping. TB: TBWindow::OnRemove
        if self.has_flag(id, NodeFlags::WINDOW) {
            self.set_window_active(id, false);
            if let Some(other) = self.topmost_other_window(id, true) {
                self.activate_window(other);
            }
        }

        // Focus moves before detaching, unless the parent is dying. TB: RemoveChild
        if self.input.focused.is_some_and(|f| self.tree.is_ancestor_of(id, f)) {
            let parent_dying = self.tree.parent(id).and_then(|p| self.node(p)).is_some_and(|p| p.is_dying());
            if !parent_dying {
                self.move_focus_excluding(true, Some(id));
            }
            if self.input.focused.is_some_and(|f| self.tree.is_ancestor_of(id, f)) {
                self.input.focused = None;
            }
        }

        // A pan in progress ends with its captured widget, and scrolls of removed
        // widgets stop.
        if let Some(cap) = self.input.captured.filter(|&c| self.tree.is_ancestor_of(id, c)) {
            if let Some(s) = self.find_started_scroller(cap) {
                self.stop_scroller(s);
            }
        }
        if !self.active_scrollers.is_empty() {
            let tree = &self.tree;
            self.active_scrollers.retain(|&w| !tree.is_ancestor_of(id, w));
        }

        // 2. Detach.
        if let Some(p) = self.tree.parent(id) {
            if let Some(pn) = self.tree.live_mut(p) {
                pn.children.retain(|&c| c != id);
            }
            self.tree.live_mut(id).unwrap().parent = None;
            self.invalidate_layout(p);
        }

        // 3-4. Input, messages, listeners, values, animations.
        let mut sub = Vec::new();
        self.tree.collect_subtree(id, &mut sub);
        let tree = &self.tree;
        self.input.forget(|w| tree.is_ancestor_of(id, w));
        self.msgs.cancel_where(|t| tree.is_ancestor_of(id, t));
        self.forget_animations(id);
        for &w in &sub {
            if let Some(ls) = self.tree.widget_listeners.remove(w) {
                for l in ls {
                    self.listeners.arena.remove(l);
                }
            }
            self.disconnect_value(w);
        }

        // 5. Mark detached; free at flush.
        for &w in &sub {
            if let Some(n) = self.tree.nodes.get_mut(w) {
                n.flags.insert(NodeFlags::DETACHED);
            }
        }
        self.pending.free.push(id);
        self.repaint = true;
        self.skin_dirty = true;
    }

    /// Graceful removal: lets the widget (or a listener) animate it out first.
    /// TB: Die
    pub fn die(&mut self, id: WidgetId) {
        self.top(|ui| ui.die_inner(id));
    }

    fn die_inner(&mut self, id: WidgetId) {
        // The root cannot go (and must not be left dying, which would take every
        // widget out of hit testing and focus).
        if id == self.root() {
            return;
        }
        let Some(n) = self.tree.live_mut(id) else { return };
        if n.flags.contains(NodeFlags::DYING) {
            return;
        }
        n.flags.insert(NodeFlags::DYING);
        self.repaint = true;
        // A dying subtree gets no input and cannot keep focus.
        if self.input.focused.is_some_and(|f| self.tree.is_ancestor_of(id, f)) {
            self.move_focus_excluding(true, Some(id));
            if self.input.focused.is_some_and(|f| self.tree.is_ancestor_of(id, f)) {
                self.input.focused = None;
            }
        }
        if self.input.captured.is_some_and(|c| self.tree.is_ancestor_of(id, c)) {
            self.set_captured(None);
        }
        // Dying from inside its own hook (a window closing itself from on_event):
        // finish when that hook returns, so on_die still runs.
        if self.tree.live(id).is_some_and(|n| n.behaviour.is_none()) {
            self.pending.die_when_free.push(id);
            return;
        }
        self.die_finish(id);
    }

    /// on_die, dying listeners, then remove unless kept alive.
    pub(crate) fn die_finish(&mut self, id: WidgetId) {
        if !self.tree.is_live(id) {
            return;
        }
        let action = call_hook(self, id, |w, cx| w.on_die(cx)).unwrap_or(DieAction::Remove);
        let mut keep = action == DieAction::KeepAlive;
        let ids = self.listener_ids(Some(id));
        if !ids.is_empty() {
            keep |= self.call_listeners(&ids, |l, ui| l.on_dying(ui, id));
        }
        if !keep {
            self.remove_inner(id);
        }
    }

    pub fn is_dying(&self, id: WidgetId) -> bool {
        self.node(id).is_some_and(|n| n.is_dying())
    }

    /// Free detached subtrees. A no-op inside hooks (only top level frees).
    pub(crate) fn flush(&mut self) {
        if self.depth != 0 {
            return;
        }
        let mut sub = Vec::new();
        while let Some(id) = self.pending.free.pop() {
            sub.clear();
            self.tree.collect_subtree(id, &mut sub);
            for &w in sub.iter().rev() {
                // The destructor. Whatever it removes is freed by this loop too.
                if let Some(mut b) = self.tree.nodes.get_mut(w).and_then(|n| n.behaviour.take()) {
                    self.depth += 1;
                    b.on_free(self);
                    self.depth -= 1;
                }
                self.tree.layout_params.remove(w);
                self.tree.scrollers.remove(w);
                self.tree.connections.remove(w);
                self.tree.widget_listeners.remove(w);
                self.tree.last_focus.remove(w);
                self.tree.nodes.remove(w);
            }
        }
    }

    /// Number of nodes in the arena (including detached ones not yet freed).
    pub fn node_count(&self) -> usize {
        self.tree.nodes.len()
    }

    // ------------------------------------------------------------------
    // Layout invalidation and measure
    // ------------------------------------------------------------------

    /// Clear the measure cache of `id` and its ancestors and mark them for arrange.
    /// Stops above a layout boundary (see `set_layout_boundary`).
    /// TB: InvalidateLayout(INVALIDATE_LAYOUT_RECURSIVE)
    pub fn invalidate_layout(&mut self, id: WidgetId) {
        let mut cur = Some(id);
        while let Some(c) = cur {
            let Some(n) = self.tree.live_mut(c) else { break };
            n.measure_cache = None;
            n.flags.insert(NodeFlags::NEEDS_ARRANGE);
            self.layout_pending = true;
            if n.visibility == Visibility::Gone || n.flags.contains(NodeFlags::LAYOUT_BOUNDARY) {
                break;
            }
            cur = n.parent;
        }
        self.repaint = true;
    }

    /// A layout boundary's size does not depend on its content, so invalidating the
    /// layout below it re-arranges it but not its ancestors.
    /// TB: TBScrollContainer::InvalidateLayout (no recursion to the parents)
    pub fn set_layout_boundary(&mut self, id: WidgetId, on: bool) {
        let Some(n) = self.tree.live_mut(id) else { return };
        if n.flags.contains(NodeFlags::LAYOUT_BOUNDARY) == on {
            return;
        }
        // Ancestors have to hear about the change either way.
        n.flags.remove(NodeFlags::LAYOUT_BOUNDARY);
        self.invalidate_layout(id);
        if let Some(n) = self.tree.live_mut(id) {
            n.flags.set(NodeFlags::LAYOUT_BOUNDARY, on);
        }
    }

    /// Preferred size under constraints, cached per node (M-04, H-18).
    /// TB: GetPreferredSize + OnCalculatePreferredSize
    pub fn preferred_size(&mut self, id: WidgetId, sc: SizeConstraints) -> PreferredSize {
        let Some(n) = self.tree.live(id) else { return PreferredSize::ZERO };
        if n.visibility == Visibility::Gone {
            return PreferredSize::ZERO;
        }
        let lp = self.tree.layout_params.get(id).copied();
        let sc = match &lp {
            Some(lp) => sc.constrain_by_layout_params(lp),
            None => sc,
        };
        let cached = n.measure_cache;
        if let Some((csc, cps)) = cached {
            if csc == sc || cps.dependency.is_empty() {
                return cps;
            }
        }
        #[cfg(feature = "debug")]
        self.debug_mark_measured(id);
        // Whether this measure had to use a stale size somewhere below (a busy
        // widget): then it is not cached.
        let outer_incomplete = core::mem::take(&mut self.measure_incomplete);
        let pad = self.padding(id);
        let inner = sc.constrain_by_padding(pad.horizontal(), pad.vertical());
        let r = call_hook(self, id, |w, cx| {
            let adds = w.measure_adds_skin();
            (w.measure_content(cx, inner), adds)
        });
        let Some((mut ps, adds_skin)) = r else {
            // One of its own hooks is running, so it cannot be measured now: answer
            // with its last size, and look again when the hook has returned.
            self.measure_incomplete = true;
            if self.is_busy(id) && !self.pending.measured_busy.contains(&id) {
                self.pending.measured_busy.push(id);
            }
            let last = self.tree.live(id).and_then(|n| n.last_ps);
            return cached.map(|c| c.1).or(last).unwrap_or(PreferredSize::ZERO);
        };
        let incomplete = self.measure_incomplete;
        self.measure_incomplete = outer_incomplete || incomplete;
        if adds_skin {
            self.apply_skin_sizes(id, &mut ps);
        }
        if let Some(b) = self.tree.live(id).and_then(|n| n.behaviour.as_deref()) {
            ps = b.after_measure(ps);
        }
        if let Some(lp) = &lp {
            lp.apply(&mut ps);
        }
        if let Some(n) = self.tree.live_mut(id) {
            if !incomplete {
                n.measure_cache = Some((sc, ps));
            }
            n.last_ps = Some(ps);
        }
        ps
    }

    /// Size `child` to its preferred size inside `parent`'s padding rect, following
    /// its gravity. TB: TBWidget::OnInflateChild
    pub fn place_child(&mut self, parent: WidgetId, child: WidgetId) {
        if self.visibility(child) == Visibility::Gone {
            return;
        }
        let pr = self.padding_rect(parent);
        let mut r = pr;
        let g = self.gravity(child);
        if !g.fills_x() || !g.fills_y() {
            let ps = self.preferred_size(child, SizeConstraints::NONE);
            if !g.fills_x() {
                r.w = ps.pref.w;
                if g.contains(crate::layout::size::Gravity::RIGHT) {
                    r.x = pr.x + pr.w - r.w;
                }
            }
            if !g.fills_y() {
                r.h = ps.pref.h;
                if g.contains(crate::layout::size::Gravity::BOTTOM) {
                    r.y = pr.y + pr.h - r.h;
                }
            }
        }
        self.set_rect(child, r);
    }

    /// Preferred size with no constraints.
    pub fn preferred_size_free(&mut self, id: WidgetId) -> PreferredSize {
        self.preferred_size(id, SizeConstraints::NONE)
    }

    /// Set the rect to the preferred size (keeping the position).
    pub fn resize_to_preferred(&mut self, id: WidgetId) {
        if self.is_busy(id) {
            // From one of its own hooks: when it returns, so it can be measured.
            self.when_free(id, move |ui| ui.resize_to_preferred(id));
            return;
        }
        let ps = self.preferred_size(id, SizeConstraints::NONE);
        let r = self.rect(id);
        self.set_rect(id, crate::geom::Rect::new(r.x, r.y, ps.pref.w, ps.pref.h));
    }
}
