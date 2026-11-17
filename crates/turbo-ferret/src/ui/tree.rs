//! The node arena, side tables and pure tree queries (no hooks).
//!
//! TB: tb_widgets.cpp (GetNextDeep, GetPrevDeep, GetLastLeaf, IsAncestorOf,
//! GetWidgetByID, GetParentWindow...).

use alloc::vec::Vec;

use crate::arena::{Arena, SideTable, WidgetId};
use crate::id::TbId;
use crate::layout::size::LayoutParams;

use super::listen::ListenerId;
use super::node::{Node, NodeFlags, NodeRef};

pub(crate) struct Tree {
    pub nodes: Arena<Node>,
    pub root: WidgetId,
    pub layout_params: SideTable<LayoutParams>,
    pub connections: SideTable<TbId>,
    pub widget_listeners: SideTable<Vec<ListenerId>>,
    pub last_focus: SideTable<WidgetId>,
}

impl Tree {
    pub fn new(root: Node) -> Tree {
        let mut nodes = Arena::new();
        let root = nodes.insert(root);
        Tree {
            nodes,
            root,
            layout_params: SideTable::new(),
            connections: SideTable::new(),
            widget_listeners: SideTable::new(),
            last_focus: SideTable::new(),
        }
    }

    /// A node that is not detached (public lookup semantics).
    pub fn live(&self, id: WidgetId) -> Option<&Node> {
        self.nodes.get(id).filter(|n| !n.detached())
    }

    pub fn live_mut(&mut self, id: WidgetId) -> Option<&mut Node> {
        self.nodes.get_mut(id).filter(|n| !n.detached())
    }

    pub fn is_live(&self, id: WidgetId) -> bool {
        self.live(id).is_some()
    }

    pub fn node_ref(&self, id: WidgetId) -> Option<NodeRef<'_>> {
        self.live(id).map(|node| NodeRef { tree: self, wid: id, node })
    }

    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.live(id)?.parent
    }

    pub fn ancestors_or_self(&self, id: WidgetId) -> impl Iterator<Item = &Node> {
        let mut cur = self.live(id);
        core::iter::from_fn(move || {
            let n = cur?;
            cur = n.parent.and_then(|p| self.live(p));
            Some(n)
        })
    }

    pub fn ancestor_ids_or_self(&self, id: WidgetId) -> impl Iterator<Item = WidgetId> + '_ {
        let mut cur = self.live(id).map(|_| id);
        core::iter::from_fn(move || {
            let i = cur?;
            cur = self.parent(i);
            Some(i)
        })
    }

    /// `a` is `b` or an ancestor of `b`. TB: IsAncestorOf
    pub fn is_ancestor_of(&self, a: WidgetId, b: WidgetId) -> bool {
        self.ancestor_ids_or_self(b).any(|x| x == a)
    }

    /// TB: GetNextDeep
    pub fn next_deep(&self, id: WidgetId, bound: Option<WidgetId>) -> Option<WidgetId> {
        let n = self.live(id)?;
        if let Some(&c) = n.children.first() {
            return Some(c);
        }
        let mut cur = id;
        while Some(cur) != bound {
            let p = self.parent(cur)?;
            let siblings = &self.live(p)?.children;
            let i = siblings.iter().position(|&s| s == cur)?;
            if let Some(&next) = siblings.get(i + 1) {
                return Some(next);
            }
            cur = p;
        }
        None
    }

    /// TB: GetPrevDeep
    pub fn prev_deep(&self, id: WidgetId) -> Option<WidgetId> {
        let p = self.parent(id)?;
        let siblings = &self.live(p)?.children;
        let i = siblings.iter().position(|&s| s == id)?;
        if i == 0 {
            return Some(p);
        }
        let mut w = siblings[i - 1];
        while let Some(&c) = self.live(w).and_then(|n| n.children.last()) {
            w = c;
        }
        Some(w)
    }

    /// TB: GetLastLeaf
    pub fn last_leaf(&self, id: WidgetId) -> Option<WidgetId> {
        let mut w = *self.live(id)?.children.last()?;
        while let Some(&c) = self.live(w).and_then(|n| n.children.last()) {
            w = c;
        }
        Some(w)
    }

    /// Nearest window, self included. TB: GetParentWindow
    pub fn parent_window(&self, id: WidgetId) -> Option<WidgetId> {
        self.ancestor_ids_or_self(id).find(|&i| self.live(i).is_some_and(|n| n.flags.contains(NodeFlags::WINDOW)))
    }

    /// Topmost node of the chain (the root if attached). TB: GetParentRoot
    pub fn parent_root(&self, id: WidgetId) -> WidgetId {
        self.ancestor_ids_or_self(id).last().unwrap_or(id)
    }

    /// Depth-first search of the subtree for a name. TB: GetWidgetByID
    pub fn find(&self, root: WidgetId, name: TbId) -> Option<WidgetId> {
        let n = self.live(root)?;
        if n.id == name {
            return Some(root);
        }
        n.children.iter().find_map(|&c| self.find(c, name))
    }

    /// All nodes of a subtree in pre-order (including detached ones: used by the
    /// removal cascade and free).
    pub fn collect_subtree(&self, root: WidgetId, out: &mut Vec<WidgetId>) {
        let start = out.len();
        if self.nodes.get(root).is_none() {
            return;
        }
        out.push(root);
        let mut i = start;
        while i < out.len() {
            if let Some(n) = self.nodes.get(out[i]) {
                out.extend_from_slice(&n.children);
            }
            i += 1;
        }
    }

    pub fn index_in_parent(&self, id: WidgetId) -> Option<usize> {
        let p = self.parent(id)?;
        self.live(p)?.children.iter().position(|&c| c == id)
    }
}
