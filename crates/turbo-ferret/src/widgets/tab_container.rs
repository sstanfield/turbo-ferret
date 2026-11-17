//! `TabContainer`: pages with a row (or column) of tab buttons.
//! TB: tb_tab_container.cpp (TBTabContainer, TBTabLayout)
//!
//! TB's TBTabLayout subclass is `Layout::tab_layout()` (a `Layout` in the tab role).

use crate::arena::WidgetId;
use crate::event::{Event, EventKind};
use crate::layout::size::{Gravity, Visibility};
use crate::layout::stack::{LayoutDistribution, LayoutDistributionPosition, LayoutPosition};
use crate::res::ResNode;
use crate::ui::Ctx;
use crate::value::Value;
use crate::widget::{Align, Axis, Widget};

use super::{Layout, Plain};

/// A container with tabs for multiple pages. Buttons inflated under `tabs` become the
/// tabs; the other children are the pages, in the same order. TB: TBTabContainer
pub struct TabContainer {
    root_layout: Option<WidgetId>,
    tab_layout: Option<WidgetId>,
    content_root: Option<WidgetId>,
    need_page_update: bool,
    current_page: i32,
    align: Align,
}

impl Default for TabContainer {
    fn default() -> Self {
        TabContainer {
            root_layout: None,
            tab_layout: None,
            content_root: None,
            need_page_update: true,
            current_page: 0,
            align: Align::Top,
        }
    }
}

impl TabContainer {
    /// The layout that holds the tab buttons. TB: GetTabLayout
    pub fn tab_layout(&self) -> Option<WidgetId> {
        self.tab_layout
    }

    /// The layout that holds the tab layout and the pages.
    pub fn root_layout(&self) -> Option<WidgetId> {
        self.root_layout
    }

    /// The parent of the pages.
    pub fn pages_root(&self) -> Option<WidgetId> {
        self.content_root
    }

    pub fn alignment(&self) -> Align {
        self.align
    }

    pub fn current_page(&self) -> i32 {
        self.current_page
    }

    /// TB: TBTabContainer::GetNumPages
    pub fn num_pages(&self, cx: &Ctx) -> usize {
        self.tab_layout.map_or(0, |t| cx.children(t).len())
    }

    /// The widget of the current page. TB: GetCurrentPageWidget
    pub fn current_page_widget(&self, cx: &Ctx) -> Option<WidgetId> {
        let root = self.content_root?;
        usize::try_from(self.current_page).ok().and_then(|i| cx.children(root).get(i).copied())
    }

    /// Put the tabs on a side of the pages. TB: TBTabContainer::SetAlignment
    pub fn set_alignment(&mut self, cx: &mut Ctx, align: Align) {
        let horizontal = matches!(align, Align::Top | Align::Bottom);
        let reverse = matches!(align, Align::Top | Align::Left);
        self.set_axis(cx, if horizontal { Axis::Y } else { Axis::X });
        if let Some(l) = self.root_layout {
            cx.with_widget::<Layout, _>(l, |w, cx| w.configure(cx, |w| w.reverse = reverse));
        }
        if let Some(l) = self.tab_layout {
            let pos = if reverse { LayoutPosition::RightBottom } else { LayoutPosition::LeftTop };
            cx.with_widget::<Layout, _>(l, |w, cx| w.configure(cx, |w| w.position = pos));
        }
        self.align = align;
        // The tabs' skin depends on the alignment (also for tabs that are inflated
        // and measured before this hook returns).
        cx.publish(self);
        cx.invalidate_skin_states();
    }

    /// Show page `index` and press its tab. TB: TBTabContainer::SetValue
    pub fn set_current_page(&mut self, cx: &mut Ctx, index: i32) {
        if index == self.current_page {
            return;
        }
        self.current_page = index;
        let (Some(pages), Some(tabs)) = (self.content_root, self.tab_layout) else { return };
        let pages = cx.children(pages).to_vec();
        let tabs = cx.children(tabs).to_vec();
        for (i, (page, tab)) in pages.into_iter().zip(tabs).enumerate() {
            let active = i as i32 == index;
            cx.set_visibility(page, if active { Visibility::Visible } else { Visibility::Invisible });
            cx.set_value(tab, active as i32);
        }
    }
}

impl Widget for TabContainer {
    fn type_name(&self) -> &'static str {
        "TabContainer"
    }

    /// TB: TBTabContainer::TBTabContainer
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        // The tab layout is above the pages in z order, so the tabs' skin can overlap
        // the border of the page container. The layout order puts it first.
        let mut root = Layout::new(Axis::Y);
        root.distribution = LayoutDistribution::Available;
        root.reverse = true;
        let root = cx.add(me, root);
        cx.set_gravity(root, Gravity::ALL);
        cx.set_skin(root, crate::tbid!("TBTabContainer.rootlayout"));
        let content = cx.add(root, Plain);
        cx.set_gravity(content, Gravity::ALL);
        cx.set_skin(content, crate::tbid!("TBTabContainer.container"));
        let mut tabs = Layout::tab_layout();
        tabs.distribution_position = LayoutDistributionPosition::Center;
        tabs.position = LayoutPosition::RightBottom;
        let tabs = cx.add(root, tabs);
        cx.set_skin(tabs, crate::tbid!("TBTabContainer.tablayout_x"));
        self.root_layout = Some(root);
        self.content_root = Some(content);
        self.tab_layout = Some(tabs);
    }

    /// TB: TBTabContainer::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        let align = res.get("align").map(|n| n.value.as_str());
        match align.as_deref() {
            Some("left") => self.set_alignment(cx, Align::Left),
            Some("top") => self.set_alignment(cx, Align::Top),
            Some("right") => self.set_alignment(cx, Align::Right),
            Some("bottom") => self.set_alignment(cx, Align::Bottom),
            _ => {}
        }
        // "tabs", "content" and "root" may also carry properties for the three parts.
        if let (Some(tabs), Some(l)) = (res.get("tabs"), self.tab_layout) {
            if let Err(e) = cx.inflate_node(l, tabs) {
                cx.warn(format_args!("TabContainer tabs: {e}"));
            }
            cx.inflate_properties(l, tabs);
        }
        if let (Some(n), Some(w)) = (res.get("content"), self.content_root) {
            cx.inflate_properties(w, n);
        }
        if let (Some(n), Some(w)) = (res.get("root"), self.root_layout) {
            cx.inflate_properties(w, n);
        }
    }

    fn content_root(&self) -> Option<WidgetId> {
        self.content_root
    }

    /// The axis the tabs and the pages are laid out along. Use `set_alignment` to
    /// choose the side; this does not change the alignment. TB: SetAxis
    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {
        if let Some(l) = self.root_layout {
            cx.with_widget::<Layout, _>(l, |w, cx| w.set_axis(cx, axis));
        }
        if let Some(l) = self.tab_layout {
            cx.with_widget::<Layout, _>(l, |w, cx| w.set_axis(cx, axis.other()));
            let skin = match axis {
                Axis::X => crate::tbid!("TBTabContainer.tablayout_y"),
                Axis::Y => crate::tbid!("TBTabContainer.tablayout_x"),
            };
            cx.set_skin(l, skin);
        }
    }

    fn axis(&self) -> Axis {
        match self.align {
            Align::Left | Align::Right => Axis::X,
            _ => Axis::Y,
        }
    }

    fn align(&self) -> Option<Align> {
        Some(self.align)
    }

    fn value(&self) -> Value {
        Value::Int(self.current_page)
    }

    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        self.set_current_page(cx, v.as_int());
    }

    /// TB: TBTabContainer::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if !matches!(ev.kind, EventKind::Click | EventKind::PointerDown) {
            return false;
        }
        let Some(tabs) = self.tab_layout else { return false };
        let is_tab = cx.node(ev.target).is_some_and(|n| n.id() == crate::tbid!("tab") && n.parent() == Some(tabs));
        if !is_tab {
            return false;
        }
        if let Some(i) = cx.children(tabs).iter().position(|&c| c == ev.target) {
            self.set_current_page(cx, i as i32);
        }
        true
    }

    /// Once, on the first frame: give the inflated pages and tabs their state.
    /// TB: TBTabContainer::OnProcess
    fn on_process(&mut self, cx: &mut Ctx) {
        if self.need_page_update {
            self.need_page_update = false;
            let page = core::mem::replace(&mut self.current_page, -1);
            self.set_current_page(cx, page);
        }
    }
}
