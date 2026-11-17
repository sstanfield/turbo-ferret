//! `Ctx` and the take-out / put-back hook call (port-design.md section 7, M-01).

use core::ops::{Deref, DerefMut};

use crate::arena::WidgetId;
use crate::event::{Event, EventKind};
use crate::geom::{Rect, Size};
use crate::layout::size::{PreferredSize, SizeConstraints, Visibility};
use crate::value::Value;
use crate::widget::Widget;

use super::Ui;
use super::node::NodeRef;
use super::tree::BusyState;

/// What a hook gets besides `&mut self`: all of `Ui` (through `Deref`), plus the id of
/// the node whose hook is running.
///
/// Inside its own hook a widget cannot reach its own behaviour through
/// `cx.get::<Self>(cx.this())` (it is taken out); it uses `self`.
pub struct Ctx<'a> {
    pub(crate) ui: &'a mut Ui,
    pub(crate) this: WidgetId,
}

impl Deref for Ctx<'_> {
    type Target = Ui;
    fn deref(&self) -> &Ui {
        self.ui
    }
}

impl DerefMut for Ctx<'_> {
    fn deref_mut(&mut self) -> &mut Ui {
        self.ui
    }
}

impl Ctx<'_> {
    /// The node whose hook is running.
    pub fn this(&self) -> WidgetId {
        self.this
    }

    /// Read-only view of this node (`None` once it has been removed).
    pub fn this_node(&self) -> Option<NodeRef<'_>> {
        self.ui.node(self.this)
    }

    pub fn ui(&mut self) -> &mut Ui {
        self.ui
    }

    /// Fire an event at this widget (bubbles from here). The widget's own `on_event`
    /// is skipped since it is busy; it already knows. Returns true if handled.
    pub fn emit(&mut self, mut ev: Event) -> bool {
        ev.target = self.this;
        self.ui.invoke_event(ev)
    }

    pub fn emit_kind(&mut self, kind: EventKind) -> bool {
        let ev = Event::new(kind, self.this);
        self.ui.invoke_event(ev)
    }

    /// Fire `Changed` with the new value in `ev.value`. Value binding and the
    /// handlers up the chain take it from the event: they cannot read it back from
    /// this widget, which is busy while its hook runs (section 9).
    pub fn emit_changed(&mut self, value: Value) -> bool {
        let mut ev = Event::new(EventKind::Changed, self.this);
        ev.value = Some(value);
        self.ui.invoke_event(ev)
    }

    /// Make this widget's current answers to the read-only queries (`axis`, `align`,
    /// `extra_padding`, `scroll_info`, `child_translation`, `scroll_root`,
    /// `content_root`, `event_destination`) visible to
    /// the rest of the UI now. While a hook runs the widget is busy and others see
    /// the answers from when the hook started; call this after changing one of them
    /// if the hook goes on to do something that depends on it (measure children whose
    /// skin has a condition on this widget, start a scroll ...).
    pub fn publish(&mut self, w: &dyn Widget) {
        let id = self.this;
        let Some(n) = self.ui.tree.node_ref(id) else { return };
        let (ep, axis, align) = (w.extra_padding(n), w.axis(), w.align());
        if let Some(node) = self.ui.tree.nodes.get_mut(id) {
            node.extra_pad = ep;
            node.axis = axis;
            node.align = align;
        }
        let state = self.ui.tree.node_ref(id).and_then(|n| BusyState::of(n, w));
        match state {
            Some(s) => {
                self.ui.tree.busy.insert(id, s);
            }
            None => {
                self.ui.tree.busy.remove(id);
            }
        }
    }

    /// The default content measure: the union of children (inside padding).
    /// TB: TBWidget::OnCalculatePreferredContentSize
    pub fn default_measure_content(&mut self, sc: SizeConstraints) -> PreferredSize {
        let this = self.this;
        let g = self.ui.gravity(this);
        let apply_max_w = !g.fills_x();
        let apply_max_h = !g.fills_y();
        let mut ps = PreferredSize::default();
        let mut has_children = false;
        let kids = self.ui.children(this).to_vec();
        for c in kids {
            if self.ui.visibility(c) == Visibility::Gone {
                continue;
            }
            if !has_children {
                has_children = true;
                if apply_max_w {
                    ps.max.w = 0;
                }
                if apply_max_h {
                    ps.max.h = 0;
                }
            }
            let cps = self.ui.preferred_size(c, sc);
            ps.pref = ps.pref.max(cps.pref);
            ps.min = ps.min.max(cps.min);
            if apply_max_w {
                ps.max.w = ps.max.w.max(cps.max.w);
            }
            if apply_max_h {
                ps.max.h = ps.max.h.max(cps.max.h);
            }
            ps.dependency |= cps.dependency;
        }
        ps
    }

    /// The default arrange: children follow the edges their gravity names.
    /// TB: TBWidget::OnResized
    pub fn default_arrange(&mut self, old: Size, new: Size) {
        let dw = new.w - old.w;
        let dh = new.h - old.h;
        let kids = self.ui.children(self.this).to_vec();
        for c in kids {
            let Some(n) = self.ui.tree.live(c) else { continue };
            if n.visibility == Visibility::Gone {
                continue;
            }
            let g = n.gravity;
            let mut r = n.rect;
            if g.fills_x() {
                r.w += dw;
            } else if g.contains(crate::layout::size::Gravity::RIGHT) {
                r.x += dw;
            }
            if g.fills_y() {
                r.h += dh;
            } else if g.contains(crate::layout::size::Gravity::BOTTOM) {
                r.y += dh;
            }
            self.ui.set_rect(c, r);
        }
    }

    /// Size a child to its preferred size inside this node's padding rect, following
    /// its gravity. TB: TBWidget::OnInflateChild
    pub fn place_child_default(&mut self, child: WidgetId) {
        let me = self.this;
        self.ui.place_child(me, child);
    }

    /// This node's local rect (0, 0, w, h).
    pub fn local_rect(&self) -> Rect {
        Rect::from_size(self.ui.rect(self.this).size())
    }
}

/// Take the behaviour out of `id`, call `f`, put it back (M-01).
///
/// `None` = not called: the id is stale or detached, or the node is busy (its hook is
/// already running further up the stack).
pub(crate) fn call_hook<R>(ui: &mut Ui, id: WidgetId, f: impl FnOnce(&mut dyn Widget, &mut Ctx) -> R) -> Option<R> {
    let n = ui.tree.node_ref(id)?;
    let Some(b) = n.behaviour() else {
        ui.warn_busy(id);
        return None;
    };
    // Queries about this node must still answer while its behaviour is out.
    let busy = BusyState::of(n, b);
    let mut b = ui.tree.live_mut(id)?.behaviour.take()?;
    if let Some(s) = busy {
        ui.tree.busy.insert(id, s);
    }
    ui.depth += 1;
    let r = f(&mut *b, &mut Ctx { ui, this: id });
    ui.depth -= 1;
    // (`Ctx::publish` may have added an entry during the hook.)
    ui.tree.busy.remove(id);
    // Slots are only freed at flush (depth 0), so the node is still there.
    if let Some(node) = ui.tree.nodes.get_mut(id) {
        node.behaviour = Some(b);
    }
    // Keep the answers that others need while this node is busy next time.
    if let Some(n) = ui.tree.node_ref(id) {
        if let Some(b) = n.behaviour() {
            let (ep, axis, align) = (b.extra_padding(n), b.axis(), b.align());
            if let Some(node) = ui.tree.nodes.get_mut(id) {
                node.extra_pad = ep;
                node.axis = axis;
                node.align = align;
            }
        }
    }
    if !ui.pending.die_when_free.is_empty() {
        if let Some(i) = ui.pending.die_when_free.iter().position(|&d| d == id) {
            ui.pending.die_when_free.swap_remove(i);
            ui.depth += 1;
            ui.die_finish(id);
            ui.depth -= 1;
        }
    }
    if ui.depth == 0 {
        ui.flush();
    }
    Some(r)
}
