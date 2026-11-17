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

    /// Fire `Changed`. The value is kept in the event only when this node has a value
    /// connection (section 9), so binding never reads back from the busy sender.
    pub fn emit_changed(&mut self, value: Value) -> bool {
        let mut ev = Event::new(EventKind::Changed, self.this);
        if self.ui.tree.connections.contains(self.this) {
            ev.value = Some(value);
        }
        self.ui.invoke_event(ev)
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
    let node = ui.tree.live_mut(id)?;
    let Some(mut b) = node.behaviour.take() else {
        ui.warn_busy(id);
        return None;
    };
    ui.depth += 1;
    let r = f(&mut *b, &mut Ctx { ui, this: id });
    ui.depth -= 1;
    // Slots are only freed at flush (depth 0), so the node is still there.
    if let Some(node) = ui.tree.nodes.get_mut(id) {
        node.behaviour = Some(b);
    }
    if let Some(n) = ui.tree.node_ref(id) {
        let ep = n.behaviour().map(|b| b.extra_padding(n)).unwrap_or_default();
        if let Some(node) = ui.tree.nodes.get_mut(id) {
            node.extra_pad = ep;
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
