//! `View`: a read-only view of the parts of `Ui` that skin conditions, measure and
//! paint read (tree, input state, skin). Paint holds it while the font manager is
//! borrowed mutably (M-06).

use crate::arena::WidgetId;
use crate::geom::Edges;
use crate::skin::{Skin, SkinElement};
use crate::widget::WidgetState;

use super::input::InputState;
use super::node::NodeRef;
use super::tree::Tree;

#[derive(Copy, Clone)]
pub struct View<'a> {
    pub(crate) tree: &'a Tree,
    pub(crate) input: &'a InputState,
    pub(crate) skin: &'a Skin,
}

impl<'a> View<'a> {
    pub fn node(&self, id: WidgetId) -> Option<NodeRef<'a>> {
        self.tree.node_ref(id)
    }

    pub fn skin(&self) -> &'a Skin {
        self.skin
    }

    /// Explicit state plus pressed, hovered and focused. TB: GetAutoState
    pub fn auto_state(&self, id: WidgetId) -> WidgetState {
        let mut s = self.tree.live(id).map(|n| n.state).unwrap_or_default();
        let i = self.input;
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

    /// Previous (`dir < 0`) or next sibling.
    pub fn sibling(&self, id: WidgetId, dir: i32) -> Option<WidgetId> {
        let p = self.tree.parent(id)?;
        let kids = &self.tree.live(p)?.children;
        let i = kids.iter().position(|&c| c == id)? as i64 + dir as i64;
        if i < 0 { None } else { kids.get(i as usize).copied() }
    }

    /// The node's skin element after strong overrides. TB: GetSkinBgElement
    pub fn element(&self, id: WidgetId) -> Option<&'a SkinElement> {
        let n = self.tree.live(id)?;
        let state = self.auto_state(id);
        self.skin.strong_override(n.skin, state, self, id)
    }

    /// Skin padding plus the widget's extra padding.
    pub fn padding(&self, id: WidgetId) -> Edges {
        let mut e = self.element(id).map(|e| e.padding).unwrap_or(Edges::ZERO);
        if let Some(n) = self.node(id) {
            if let Some(b) = n.behaviour() {
                let x = b.extra_padding(n);
                e = Edges::new(e.left + x.left, e.top + x.top, e.right + x.right, e.bottom + x.bottom);
            }
        }
        e
    }
}
