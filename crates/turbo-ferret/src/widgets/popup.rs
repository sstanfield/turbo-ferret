//! `PopupWindow`, `MenuWindow` and `PopupAlignment`.
//! TB: tb_popup_window.cpp (TBPopupAlignment, TBPopupWindow), tb_menu_window.cpp
//!
//! TB's popup was a TBWindow subclass and a global widget listener. Here it holds a
//! `Window` and forwards to it, and registers a small listener (`TargetWatcher`) that
//! closes it.

use crate::arena::WidgetId;
use crate::event::{Event, EventKind, SpecialKey};
use crate::geom::{Point, Rect};
use crate::id::TbId;
use crate::layout::size::{Gravity, SizeConstraints};
use crate::ui::{Ctx, ListenerId, NodeRef, Ui, UiListener};
use crate::widget::{Align, DieAction, Widget, Z};

use super::scroll_container::ScrollContainer;
use super::select::SelectList;
use super::select_item::SourceRef;
use super::window::{Window, WindowSettings, forward_to_window};

/// Where a popup wants to be, relative to a target widget or a point.
/// TB: TBPopupAlignment
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PopupAlignment {
    /// A position in root coordinates, instead of the target widget's rect.
    pub pos_in_root: Option<Point>,
    /// With `pos_in_root`: the size around the position to keep clear.
    pub pos_offset: Point,
    pub align: Align,
    /// Make the popup at least as wide as the target, when aligned top or bottom.
    pub expand_to_target_width: bool,
}

impl Default for PopupAlignment {
    /// Below the target widget.
    fn default() -> Self {
        PopupAlignment::new(Align::Bottom)
    }
}

impl PopupAlignment {
    /// Aligned to a side of the target widget.
    pub fn new(align: Align) -> PopupAlignment {
        PopupAlignment { pos_in_root: None, pos_offset: Point::ZERO, align, expand_to_target_width: true }
    }

    /// Aligned to a position (root coordinates).
    pub fn at(pos_in_root: Point, align: Align) -> PopupAlignment {
        PopupAlignment { pos_in_root: Some(pos_in_root), ..PopupAlignment::new(align) }
    }

    /// Below a position (root coordinates), keeping `pos_offset` around it clear.
    pub fn at_with_offset(pos_in_root: Point, pos_offset: Point) -> PopupAlignment {
        PopupAlignment { pos_in_root: Some(pos_in_root), pos_offset, ..PopupAlignment::new(Align::Bottom) }
    }

    /// A good rect for `popup` from its preferred size and this alignment.
    /// TB: TBPopupAlignment::GetAlignedRect
    pub fn aligned_rect(&self, ui: &mut Ui, popup: WidgetId, target: WidgetId) -> Rect {
        let root = ui.parent_root(target);
        let rs = ui.rect(root).size();
        let ps = ui.preferred_size(popup, SizeConstraints::new(rs.w, rs.h));
        let ts = ui.rect(target).size();
        let vertical = matches!(self.align, Align::Top | Align::Bottom);
        let mut w = ps.pref.w.min(rs.w);
        let mut h = ps.pref.h.min(rs.h);
        // The size that is avoided if the popup has to go to the other side.
        let (mut x, mut y, avoid_w, avoid_h);
        match self.pos_in_root {
            Some(p) => {
                (x, y) = (p.x, p.y);
                (avoid_w, avoid_h) = (self.pos_offset.x, self.pos_offset.y);
                // Make sure it is in view horizontally.
                if vertical {
                    x = x.clamp(0, (rs.w - w).max(0));
                }
            }
            None => {
                let p = ui.to_root(target, Point::ZERO);
                (x, y) = (p.x, p.y);
                if vertical {
                    if self.expand_to_target_width {
                        w = w.max(ts.w);
                    }
                    // Limit the height to what is free in the worst case: the target
                    // in the middle of the root.
                    h = h.min(rs.h / 2 - ts.h);
                }
                (avoid_w, avoid_h) = (ts.w, ts.h);
            }
        }
        match self.align {
            Align::Bottom => y = if y + avoid_h + h > rs.h { y - h } else { y + avoid_h },
            Align::Top => y = if y - h < 0 { y + avoid_h } else { y - h },
            Align::Right => {
                x = if x + avoid_w + w > rs.w { x - w } else { x + avoid_w };
                y = y.min(rs.h - h);
            }
            _ => {
                x = if x - w < 0 { x + avoid_w } else { x - w };
                y = y.min(rs.h - h);
            }
        }
        Rect::new(x, y, w, h)
    }
}

/// Closes a window when its target goes away, and (for popups) on a pointer press or
/// a focus change outside it. TB: the TBWidgetListener side of TBPopupWindow and
/// TBMessageWindow
pub(crate) struct TargetWatcher {
    pub window: WidgetId,
    pub target: WidgetId,
    pub outside_closes: bool,
}

impl TargetWatcher {
    fn target_is_under(&self, ui: &Ui, id: WidgetId) -> bool {
        id == self.target || ui.is_ancestor_of(id, self.target)
    }
}

impl UiListener for TargetWatcher {
    /// TB: OnWidgetFocusChanged
    fn on_focus_changed(&mut self, ui: &mut Ui, id: WidgetId, focused: bool) {
        if self.outside_closes && focused && !ui.is_event_destination_for(self.window, id) {
            ui.close_window(self.window);
        }
    }

    /// TB: OnWidgetInvokeEvent
    fn on_event(&mut self, ui: &mut Ui, _target: WidgetId, ev: &Event) -> bool {
        if self.outside_closes
            && matches!(ev.kind, EventKind::PointerDown | EventKind::ContextMenu)
            && !ui.is_event_destination_for(self.window, ev.target)
        {
            ui.close_window(self.window);
        }
        false
    }

    /// The target or an ancestor of it is dying. TB: OnWidgetDying
    fn on_dying(&mut self, ui: &mut Ui, id: WidgetId) -> bool {
        if self.target_is_under(ui, id) {
            ui.close_window(self.window);
        }
        false
    }

    /// The target is removed. TB: OnWidgetDelete
    fn on_removing(&mut self, ui: &mut Ui, _parent: Option<WidgetId>, child: WidgetId) {
        if self.target_is_under(ui, child) {
            ui.close_window(self.window);
        }
    }
}

/// A window whose events go on through a target widget, and that closes on a click
/// or a focus change outside it. Show it with [`Ui::show_popup`]. TB: TBPopupWindow
pub struct PopupWindow {
    window: Window,
    target: WidgetId,
    watcher: Option<ListenerId>,
}

impl PopupWindow {
    pub fn new(target: WidgetId) -> PopupWindow {
        PopupWindow { window: Window::default().with_settings(WindowSettings::NONE), target, watcher: None }
    }

    pub fn target(&self) -> WidgetId {
        self.target
    }

    fn create(&mut self, cx: &mut Ctx, skin: TbId) {
        self.window.on_create(cx);
        let me = cx.this();
        cx.set_skin_quiet(me, skin);
        let watcher = TargetWatcher { window: me, target: self.target, outside_closes: true };
        self.watcher = Some(cx.add_listener(alloc::boxed::Box::new(watcher)));
    }

    fn removed(&mut self, cx: &mut Ctx) {
        if let Some(w) = self.watcher.take() {
            cx.remove_listener(w);
        }
    }

    /// TB: TBPopupWindow::OnEvent
    fn event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if ev.kind == EventKind::KeyDown && ev.special == SpecialKey::Esc {
            let me = cx.this();
            cx.close_window(me);
            return true;
        }
        self.window.on_event(cx, ev)
    }
}

impl Widget for PopupWindow {
    fn type_name(&self) -> &'static str {
        "PopupWindow"
    }
    /// TB: TBPopupWindow::TBPopupWindow
    fn on_create(&mut self, cx: &mut Ctx) {
        self.create(cx, crate::tbid!("TBPopupWindow"));
    }
    fn on_removed(&mut self, cx: &mut Ctx) {
        self.removed(cx);
    }
    /// TB: TBPopupWindow::GetEventDestination
    fn event_destination(&self, _this: NodeRef<'_>) -> Option<WidgetId> {
        Some(self.target)
    }
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        self.event(cx, ev)
    }
    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        self.window.on_die(cx)
    }
    forward_to_window!(window);
}

/// A popup with a list of items. Choosing one sends a `Click` with the menu's id
/// through the target, with the item's id as `ref_id` and its index as `value`, and
/// closes the menu. Sub menus open as more menu windows. Show one with
/// [`Ui::show_menu`]. TB: TBMenuWindow
pub struct MenuWindow {
    popup: PopupWindow,
    list: Option<WidgetId>,
}

impl MenuWindow {
    pub fn new(target: WidgetId) -> MenuWindow {
        MenuWindow { popup: PopupWindow::new(target), list: None }
    }

    pub fn target(&self) -> WidgetId {
        self.popup.target
    }

    /// The `SelectList` with the items. TB: GetList
    pub fn list(&self) -> Option<WidgetId> {
        self.list
    }
}

impl Widget for MenuWindow {
    fn type_name(&self) -> &'static str {
        "MenuWindow"
    }

    /// TB: TBMenuWindow::TBMenuWindow
    fn on_create(&mut self, cx: &mut Ctx) {
        self.popup.create(cx, crate::tbid!("TBMenuWindow"));
        let me = cx.this();
        let list = cx.add(me, SelectList::default());
        if let Some(sc) = cx.get::<SelectList>(list).and_then(|l| l.scroll_container()) {
            cx.with_widget::<ScrollContainer, _>(sc, |w, cx| w.set_adapt_to_content_size(cx, true));
        }
        // Not focusable, or a click in the list would close its own window.
        cx.set_focusable(list, false);
        cx.set_skin(list, TbId::NONE);
        let pr = cx.padding_rect(me);
        cx.set_rect(list, pr);
        cx.set_gravity(list, Gravity::ALL);
        self.list = Some(list);
    }

    fn on_removed(&mut self, cx: &mut Ctx) {
        self.popup.removed(cx);
    }

    fn event_destination(&self, _this: NodeRef<'_>) -> Option<WidgetId> {
        Some(self.popup.target)
    }

    /// TB: TBMenuWindow::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if ev.kind == EventKind::Click && Some(ev.target) == self.list {
            // Pass the click on through the target, then close.
            let me = cx.this();
            let mut click = Event::new(EventKind::Click, me);
            click.ref_id = ev.ref_id;
            click.value = ev.value.clone();
            cx.emit(click);
            cx.close_window(me);
            return true;
        }
        self.popup.event(cx, ev)
    }

    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        self.popup.window.on_die(cx)
    }

    // The same forwards as a popup, to the window inside it.
    fn extra_padding(&self, this: NodeRef<'_>) -> crate::geom::Edges {
        self.popup.extra_padding(this)
    }
    fn measure_adds_skin(&self) -> bool {
        self.popup.measure_adds_skin()
    }
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> crate::layout::size::PreferredSize {
        self.popup.measure_content(cx, sc)
    }
    fn arrange(&mut self, cx: &mut Ctx, old: crate::geom::Size, new: crate::geom::Size) {
        self.popup.arrange(cx, old, new)
    }
    fn on_child_added(&mut self, cx: &mut Ctx, child: WidgetId) {
        self.popup.on_child_added(cx, child)
    }
    fn on_window_active_changed(&mut self, cx: &mut Ctx, active: bool) {
        self.popup.on_window_active_changed(cx, active)
    }
    fn on_added(&mut self, cx: &mut Ctx) {
        self.popup.on_added(cx)
    }
    fn text(&self) -> Option<&str> {
        self.popup.text()
    }
    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        self.popup.set_text(cx, text)
    }
}

impl Ui {
    /// The target of a popup or menu window.
    pub fn popup_target(&self, popup: WidgetId) -> Option<WidgetId> {
        // A menu is in the middle of a hook when it reports a choice, and cannot be
        // asked then. Its events go to its target, and the tree keeps that answer.
        let name = self.node(popup)?.type_name();
        if name == "PopupWindow" || name == "MenuWindow" { self.event_destination(popup) } else { None }
    }

    /// Size, place and show a popup or menu window made with `create`. Returns false
    /// if it is not one or its target is gone. TB: TBPopupWindow::Show
    pub fn show_popup(&mut self, popup: WidgetId, alignment: &PopupAlignment) -> bool {
        let Some(target) = self.popup_target(popup).filter(|&t| self.exists(t)) else { return false };
        let r = alignment.aligned_rect(self, popup, target);
        self.set_rect(popup, r);
        let root = self.parent_root(target);
        self.attach(root, popup, Z::Top)
    }

    /// Create a menu window for `target` without showing it yet (to change its skin
    /// first, for example); `show_menu_window` shows it.
    pub fn create_menu(&mut self, target: WidgetId, id: TbId) -> WidgetId {
        let menu = self.create(MenuWindow::new(target));
        self.set_id(menu, id);
        menu
    }

    /// Fill a menu window from `source`, select `initial_value` (-1 for none) and show
    /// it. TB: TBMenuWindow::Show
    pub fn show_menu_window(
        &mut self,
        menu: WidgetId,
        source: SourceRef,
        alignment: &PopupAlignment,
        initial_value: i32,
    ) -> bool {
        let Some(list) = self.get::<MenuWindow>(menu).and_then(|m| m.list()) else { return false };
        self.with_widget::<SelectList, _>(list, |l, cx| {
            l.set_value_quiet(cx, initial_value);
            l.set_source(cx, Some(source));
            l.validate_list(cx);
        });
        self.show_popup(menu, alignment)
    }

    /// Show a menu with the items of `source` next to `target`. Choosing an item sends
    /// a `Click` with `id` through `target` (`ref_id` is the item's id, `value` its
    /// index). TB: new TBMenuWindow(target, id) + Show
    pub fn show_menu(
        &mut self,
        target: WidgetId,
        id: TbId,
        source: SourceRef,
        alignment: &PopupAlignment,
        initial_value: i32,
    ) -> Option<WidgetId> {
        if !self.exists(target) {
            return None;
        }
        let menu = self.create_menu(target, id);
        if self.show_menu_window(menu, source, alignment, initial_value) {
            Some(menu)
        } else {
            self.remove(menu);
            None
        }
    }
}
