//! Skin integration (M3) and hooks into animation (M4).

use alloc::vec::Vec;

use crate::arena::WidgetId;
use crate::id::TbId;
use crate::layout::size::PreferredSize;
use crate::skin::{Skin, SkinElement, SkinError};

use super::Ui;
use super::ctx::call_hook;
use super::view::View;

impl Ui {
    pub fn view(&self) -> View<'_> {
        View { tree: &self.tree, input: &self.input, skin: &self.skin }
    }

    pub fn skin_data(&self) -> &Skin {
        &self.skin
    }

    pub fn skin_data_mut(&mut self) -> &mut Skin {
        &mut self.skin
    }

    /// Load a skin (and optionally an override skin on top) with its bitmaps. Bitmaps
    /// are uploaded to the renderer on the next paint. TB: TBSkin::Load
    pub fn load_skin(&mut self, path: &str, override_path: Option<&str>) -> Result<(), SkinError> {
        let dpi = self.config.dpi;
        let r = self.skin.load(&mut *self.services.loader, &mut *self.services.images, path, override_path, dpi);
        self.dim = self.skin.dim.clone();
        self.skin_dirty = true;
        self.repaint = true;
        let root = self.root();
        self.invalidate_layout_all(root);
        r
    }

    fn invalidate_layout_all(&mut self, id: WidgetId) {
        let mut all = Vec::new();
        self.tree.collect_subtree(id, &mut all);
        for w in all {
            if let Some(n) = self.tree.live_mut(w) {
                n.measure_cache = None;
                n.flags.insert(super::NodeFlags::NEEDS_ARRANGE);
            }
        }
    }

    /// The node's skin element after strong overrides. TB: GetSkinBgElement
    pub fn skin_element(&self, id: WidgetId) -> Option<&SkinElement> {
        self.view().element(id)
    }

    /// Apply the skin element's padding and size settings to a measured content size.
    /// TB: TBWidget::OnCalculatePreferredSize
    pub(crate) fn apply_skin_sizes(&self, id: WidgetId, ps: &mut PreferredSize) {
        let Some(e) = self.skin_element(id) else { return };
        let ph = e.padding.horizontal();
        let pv = e.padding.vertical();
        if let Some(v) = e.sizes.pref_width {
            ps.pref.w = v;
        } else if let (0, Some(iw)) = (ps.pref.w, e.intrinsic_width()) {
            ps.pref.w = iw;
        } else {
            ps.min.w += ph;
            ps.pref.w += ph;
        }
        if let Some(v) = e.sizes.pref_height {
            ps.pref.h = v;
        } else if let (0, Some(ih)) = (ps.pref.h, e.intrinsic_height()) {
            ps.pref.h = ih;
        } else {
            ps.min.h += pv;
            ps.pref.h += pv;
        }
        ps.min.w = e.sizes.min_width.unwrap_or_else(|| ps.min.w.max(e.intrinsic_min_width()));
        ps.min.h = e.sizes.min_height.unwrap_or_else(|| ps.min.h.max(e.intrinsic_min_height()));
        ps.max.w = e.sizes.max_width.unwrap_or(ps.max.w + ph);
        ps.max.h = e.sizes.max_height.unwrap_or(ps.max.h + pv);
        ps.pref = ps.pref.max(ps.min);
    }

    /// Re-resolve strong overrides; widgets whose element changed get
    /// `on_skin_changed` and a layout invalidation. TB: InvokeSkinUpdatesInternal
    pub(crate) fn process_skin_updates(&mut self) {
        if !core::mem::take(&mut self.skin_dirty) {
            return;
        }
        let mut all = Vec::new();
        self.tree.collect_subtree(self.root(), &mut all);
        let changed: Vec<(WidgetId, TbId)> = {
            let v = self.view();
            all.iter()
                .filter_map(|&w| {
                    let n = self.tree.live(w)?;
                    let e = v.element(w)?;
                    (e.id != n.skin_expected).then_some((w, e.id))
                })
                .collect()
        };
        for (w, eid) in changed {
            if let Some(n) = self.tree.live_mut(w) {
                n.skin_expected = eid;
            }
            call_hook(self, w, |b, cx| b.on_skin_changed(cx));
            self.invalidate_layout(w);
        }
    }

    /// The skin element's `spacing`.
    pub fn skin_spacing(&self, id: WidgetId) -> Option<i32> {
        self.skin_element(id).and_then(|e| e.spacing)
    }

    /// The skin's default spacing. TB: TBSkin::GetDefaultSpacing
    pub fn default_spacing(&self) -> i32 {
        self.skin.defaults.spacing
    }

    /// The renderer lost its context: forget every bitmap; the next paint recreates
    /// them. TB: TBRendererListener::OnContextLost/Restored
    pub fn renderer_lost(&mut self) {
        self.skin.atlas.renderer_lost();
        self.fonts.renderer_lost();
        self.images.renderer_lost();
        self.repaint = true;
    }
}
