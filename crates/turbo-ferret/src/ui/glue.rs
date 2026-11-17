//! Hooks into subsystems that land in later milestones (skin at M3, animation at M4).

use crate::arena::WidgetId;
use crate::geom::Edges;
use crate::layout::size::PreferredSize;

use super::Ui;

impl Ui {
    /// Padding of the node's skin element (M3).
    pub(crate) fn skin_padding(&self, _id: WidgetId) -> Edges {
        Edges::ZERO
    }

    /// Apply the skin element's padding and size settings to a measured size (M3).
    /// TB: TBWidget::OnCalculatePreferredSize
    pub(crate) fn apply_skin_sizes(&self, _id: WidgetId, _ps: &mut PreferredSize) {}

    /// Re-resolve strong overrides (M3). TB: InvokeSkinUpdatesInternal
    pub(crate) fn process_skin_updates(&mut self) {
        self.skin_dirty = false;
    }

    pub(crate) fn update_animations(&mut self) {}

    pub(crate) fn has_running_animations(&self) -> bool {
        false
    }

    pub(crate) fn forget_animations(&mut self, _subtree_root: WidgetId) {}
}
