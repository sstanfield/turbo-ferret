//! `SelectList` and `SelectDropdown`. TB: tb_select.cpp (TBSelectList, TBSelectDropdown)

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::arena::WidgetId;
use crate::event::{Event, EventKind, SpecialKey};
use crate::geom::{Point, Size};
use crate::id::TbId;
use crate::layout::size::Gravity;
use crate::layout::stack::{LayoutDistributionPosition, LayoutPosition, LayoutSize};
use crate::msg::Message;
use crate::res::ResNode;
use crate::ui::{Ctx, NodeRef, Ui};
use crate::value::Value;
use crate::widget::{Axis, HitStatus, Widget, WidgetState};

use super::popup::{MenuWindow, PopupAlignment};
use super::scroll_container::{ScrollContainer, ScrollMode};
use super::select_item::{Sort, SourceRef, SourceUpdate, StringItemSource, add_item_after};
use super::text::TextField;
use super::{Button, Layout, SkinImage};

/// A scrollable list of the items of a [`SelectItemSource`](super::SelectItemSource).
/// Its value is the index of the selected item (-1 for none). TB: TBSelectList
pub struct SelectList {
    container: Option<WidgetId>,
    layout: Option<WidgetId>,
    default_source: Rc<RefCell<StringItemSource>>,
    source: Option<SourceRef>,
    /// The source's change counter when the list last looked.
    seen: u32,
    value: i32,
    filter: String,
    list_is_invalid: bool,
    scroll_to_current: bool,
    header_string: TbId,
}

impl Default for SelectList {
    fn default() -> Self {
        let default_source = StringItemSource::shared();
        SelectList {
            container: None,
            layout: None,
            source: Some(default_source.clone()),
            default_source,
            seen: 0,
            value: -1,
            filter: String::new(),
            // Nothing is built yet.
            list_is_invalid: true,
            scroll_to_current: false,
            header_string: crate::tbid!("TBList.header"),
        }
    }
}

impl SelectList {
    /// The source the list starts with, and that resource files fill (`items`). To
    /// share items between widgets or to use other item types, `set_source` another
    /// one. TB: GetDefaultSource
    pub fn default_source(&self) -> Rc<RefCell<StringItemSource>> {
        self.default_source.clone()
    }

    pub fn source(&self) -> Option<SourceRef> {
        self.source.clone()
    }

    /// TB: TBSelectItemViewer::SetSource
    pub fn set_source(&mut self, cx: &mut Ctx, source: Option<SourceRef>) {
        let same = match (&self.source, &source) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.source = source;
        self.seen = self.source_version().unwrap_or(0);
        self.invalidate_list(cx);
    }

    fn source_version(&self) -> Option<u32> {
        Some(self.source.as_ref()?.try_borrow().ok()?.changes().version())
    }

    /// The scroll container the items are in. TB: GetScrollContainer
    pub fn scroll_container(&self) -> Option<WidgetId> {
        self.container
    }

    /// The layout whose children are the item widgets.
    pub fn item_layout(&self) -> Option<WidgetId> {
        self.layout
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Show only the items that match `filter` ("" shows all). TB: SetFilter
    pub fn set_filter(&mut self, cx: &mut Ctx, filter: &str) {
        if self.filter != filter {
            self.filter = String::from(filter);
            self.invalidate_list(cx);
        }
    }

    /// The language string of the header that is shown above a filtered list (it
    /// gets the shown and the total count). TB: SetHeaderString
    pub fn set_header_string(&mut self, cx: &mut Ctx, id: TbId) {
        if self.header_string != id {
            self.header_string = id;
            self.invalidate_list(cx);
        }
    }

    /// Rebuild the item widgets at the next validation. TB: InvalidateList
    pub fn invalidate_list(&mut self, cx: &mut Ctx) {
        if !self.list_is_invalid {
            self.list_is_invalid = true;
            cx.invalidate();
        }
    }

    /// Make the item widgets match the source now. TB: ValidateList
    pub fn validate_list(&mut self, cx: &mut Ctx) {
        if !self.list_is_invalid {
            return;
        }
        self.list_is_invalid = false;
        let (Some(layout), me) = (self.layout, cx.this()) else { return };
        for c in cx.children(layout).to_vec() {
            cx.remove(c);
        }
        let Some(source) = self.source.clone() else { return };
        // The source is only read here; if the application holds it mutably right
        // now, try again at the next process.
        let Ok(src) = source.try_borrow() else {
            self.list_is_invalid = true;
            return;
        };
        self.seen = src.changes().version();
        let total = src.num_items();
        if total == 0 {
            return;
        }
        // The items to show with the current filter, sorted.
        let mut shown: Vec<usize> =
            (0..total).filter(|&i| self.filter.is_empty() || src.filter(i, &self.filter)).collect();
        match src.sort() {
            Sort::None => {}
            Sort::Ascending => shown.sort_by(|&a, &b| src.item_string(a).cmp(src.item_string(b))),
            Sort::Descending => shown.sort_by(|&a, &b| src.item_string(b).cmp(src.item_string(a))),
        }
        // A header tells that only a part of the items is shown.
        if !self.filter.is_empty() {
            let text = format_counts(&cx.lang().get(self.header_string), shown.len(), total);
            let header = cx.add(layout, TextField::new(&text));
            cx.set_skin(header, crate::tbid!("TBList.header"));
            cx.set_state(header, WidgetState::DISABLED, true);
            cx.set_gravity(header, Gravity::ALL);
            cx.set_data(header, Value::Int(-1));
        }
        for i in shown {
            if let Some(w) = src.create_item_widget(cx, i, me) {
                // The item's index is kept in the widget's data.
                cx.set_data(w, Value::Int(i as i32));
                add_item_after(cx, layout, w, None);
            }
        }
        drop(src);
        self.select_item(cx, self.value, true);
        self.scroll_to_current = true;
    }

    /// Replace the widget of one item, keeping its state. TB: OnItemChanged
    fn item_changed(&mut self, cx: &mut Ctx, index: usize) {
        if self.list_is_invalid {
            return;
        }
        let (Some(old), Some(layout), Some(source)) =
            (self.item_widget(cx, index as i32), self.layout, self.source.clone())
        else {
            return;
        };
        let state = cx.state(old);
        let me = cx.this();
        if let Ok(src) = source.try_borrow() {
            if let Some(w) = src.create_item_widget(cx, index, me) {
                cx.set_data(w, Value::Int(index as i32));
                add_item_after(cx, layout, w, Some(old));
                cx.set_state_raw(w, state);
            }
        }
        cx.remove(old);
    }

    /// Select item `index` (-1 for none). TB: TBSelectList::SetValue
    pub fn set_selected(&mut self, cx: &mut Ctx, index: i32) {
        if index == self.value {
            return;
        }
        self.set_value_quiet(cx, index);
        let mut ev = Event::new(EventKind::Changed, cx.this());
        ev.ref_id = self.item_widget(cx, index).and_then(|w| cx.node(w)).map_or(TbId::NONE, |n| n.id());
        ev.value = Some(Value::Int(index));
        cx.emit(ev);
    }

    /// Select an item without a `Changed` event.
    pub fn set_value_quiet(&mut self, cx: &mut Ctx, index: i32) {
        if index == self.value {
            return;
        }
        self.select_item(cx, self.value, false);
        self.value = index;
        self.select_item(cx, index, true);
        self.scroll_to_selected_item(cx);
    }

    pub fn selected(&self) -> i32 {
        self.value
    }

    /// The id of the selected item. TB: GetSelectedItemID
    pub fn selected_item_id(&self) -> TbId {
        let (Ok(i), Some(src)) = (usize::try_from(self.value), self.source.as_ref()) else { return TbId::NONE };
        src.try_borrow().ok().filter(|s| i < s.num_items()).map_or(TbId::NONE, |s| s.item_id(i))
    }

    /// Set the selected look of an item's widget (to deselect the previous item, use
    /// `set_selected`). TB: SelectItem
    pub fn select_item(&mut self, cx: &mut Ctx, index: i32, selected: bool) {
        if let Some(w) = self.item_widget(cx, index) {
            cx.set_state(w, WidgetState::SELECTED, selected);
        }
    }

    /// The widget of item `index`, if it is shown. TB: GetItemWidget
    pub fn item_widget(&self, ui: &Ui, index: i32) -> Option<WidgetId> {
        if index == -1 {
            return None;
        }
        let layout = self.layout?;
        ui.children(layout).iter().copied().find(|&c| item_index(ui, c) == Some(index))
    }

    /// Scroll to the selected item (after the next layout if the list is being
    /// rebuilt). TB: ScrollToSelectedItem
    pub fn scroll_to_selected_item(&mut self, cx: &mut Ctx) {
        if self.list_is_invalid {
            self.scroll_to_current = true;
            return;
        }
        self.scroll_to_current = false;
        let Some(container) = self.container else { return };
        match self.item_widget(cx, self.value) {
            Some(w) => {
                let r = cx.rect(w);
                cx.scroll_into_view(container, r);
            }
            None => cx.scroll_to(container, Point::ZERO),
        }
    }

    /// Move the selection to another enabled item with Up, Down, Home or End.
    /// Returns true if it moved. TB: ChangeValue
    pub fn change_value(&mut self, cx: &mut Ctx, key: SpecialKey) -> bool {
        let Some(layout) = self.layout else { return false };
        let items = cx.children(layout).to_vec();
        if self.source.is_none() || items.is_empty() {
            return false;
        }
        let forward = match key {
            SpecialKey::Home | SpecialKey::Down => true,
            SpecialKey::End | SpecialKey::Up => false,
            _ => return false,
        };
        let current = self.item_widget(cx, self.value).and_then(|w| items.iter().position(|&c| c == w));
        let (mut pos, origin) = match (key, current) {
            (SpecialKey::Home, _) | (SpecialKey::Down, None) => (Some(0), None),
            (SpecialKey::End, _) | (SpecialKey::Up, None) => (Some(items.len() - 1), None),
            (_, cur) => (cur, cur),
        };
        while let Some(p) = pos {
            if Some(p) != origin && !cx.has_state(items[p], WidgetState::DISABLED) {
                break;
            }
            pos = if forward { Some(p + 1).filter(|&n| n < items.len()) } else { p.checked_sub(1) };
        }
        match pos.and_then(|p| item_index(cx, items[p])) {
            Some(index) => {
                self.set_selected(cx, index);
                true
            }
            None => false,
        }
    }
}

/// The item index kept in an item widget's data (the header has -1).
fn item_index(ui: &Ui, w: WidgetId) -> Option<i32> {
    ui.node(w).and_then(|n| match n.data() {
        Value::Int(i) => Some(*i),
        _ => None,
    })
}

/// Fill the two `%d` of a header string ("Showing %d of %d").
fn format_counts(fmt: &str, shown: usize, total: usize) -> String {
    let mut out = String::new();
    let mut values = [shown, total].into_iter();
    let mut rest = fmt;
    while let Some(i) = rest.find("%d") {
        out.push_str(&rest[..i]);
        match values.next() {
            Some(v) => out.push_str(&alloc::format!("{v}")),
            None => out.push_str("%d"),
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

impl Widget for SelectList {
    fn type_name(&self) -> &'static str {
        "SelectList"
    }

    /// TB: TBSelectList::TBSelectList
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_focusable(me, true);
        cx.set_skin_quiet(me, crate::tbid!("TBSelectList"));
        let container = cx.add(me, ScrollContainer::new(ScrollMode::YAuto).with_adapt_content_size(true));
        cx.set_gravity(container, Gravity::ALL);
        let pr = cx.padding_rect(me);
        cx.set_rect(container, pr);
        let mut layout = Layout::new(Axis::Y);
        layout.spacing = Some(0);
        layout.position = LayoutPosition::LeftTop;
        layout.distribution_position = LayoutDistributionPosition::LeftTop;
        layout.size = LayoutSize::Available;
        let content = cx.content_root(container);
        let layout = cx.add(content, layout);
        cx.set_gravity(layout, Gravity::ALL);
        self.container = Some(container);
        self.layout = Some(layout);
    }

    /// TB: TBSelectList::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        if let Ok(mut source) = self.default_source.try_borrow_mut() {
            source.read_items(res);
        }
        // TB's generic OnInflate sets the value even without a `value` property, so
        // an inflated list starts with its first item selected.
        if res.get("value").is_none() {
            self.set_selected(cx, 0);
        }
    }

    fn on_skin_changed(&mut self, cx: &mut Ctx) {
        if let Some(c) = self.container {
            let pr = cx.padding_rect(cx.this());
            cx.set_rect(c, pr);
        }
    }

    fn value(&self) -> Value {
        Value::Int(self.value)
    }

    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        self.set_selected(cx, v.as_int());
    }

    /// Notice changes of the source, then rebuild if needed.
    /// TB: the TBSelectItemViewer callbacks + TBSelectList::OnProcess
    fn on_process(&mut self, cx: &mut Ctx) {
        let update = match self.source.as_ref().and_then(|s| s.try_borrow().ok()) {
            Some(src) => {
                let u = src.changes().since(self.seen, cx.this());
                let cleared = src.changes().cleared_since(self.seen);
                if !self.list_is_invalid {
                    self.seen = src.changes().version();
                }
                if cleared || (u != SourceUpdate::None && src.num_items() == 0) {
                    // TB: OnAllItemsRemoved
                    self.value = -1;
                }
                u
            }
            None => SourceUpdate::None,
        };
        match update {
            SourceUpdate::None => {}
            SourceUpdate::Item(i) => self.item_changed(cx, i),
            SourceUpdate::All => self.invalidate_list(cx),
        }
        self.validate_list(cx);
    }

    /// TB: TBSelectList::OnProcessAfterChildren
    fn on_process_after_children(&mut self, cx: &mut Ctx) {
        if self.scroll_to_current {
            self.scroll_to_selected_item(cx);
        }
    }

    /// TB: TBSelectList::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let me = cx.this();
        if ev.kind == EventKind::Click && self.layout.is_some() && cx.parent(ev.target) == self.layout {
            let Some(index) = item_index(cx, ev.target) else { return true };
            // The Changed event may make something remove this list (a dropdown
            // closing its menu); only go on if it is still there.
            self.set_selected(cx, index);
            if !cx.exists(me) {
                return true;
            }
            // In a menu window, the click is sent from the list of the topmost menu,
            // so events of sub menus reach the caller as if from the top menu.
            let mut target_list = me;
            let mut window = cx.parent_window(me);
            while let Some((list, target)) =
                window.and_then(|w| cx.get::<MenuWindow>(w)).and_then(|m| Some((m.list()?, m.target())))
            {
                target_list = list;
                window = cx.parent_window(target);
            }
            let mut click = Event::new(EventKind::Click, target_list);
            click.ref_id = self.item_widget(cx, self.value).and_then(|w| cx.node(w)).map_or(TbId::NONE, |n| n.id());
            click.value = Some(Value::Int(self.value));
            cx.invoke_event(click);
            return true;
        }
        if ev.kind == EventKind::KeyDown {
            if self.change_value(cx, ev.special) {
                return true;
            }
            // Let the scroll container scroll by keys when the list itself has the
            // focus (from a focused item the event passed the container already).
            if let Some(c) = self.container {
                if cx.with_behaviour(c, |w, cx| w.on_event(cx, ev)) == Some(true) {
                    return true;
                }
            }
        }
        false
    }
}

/// A button that opens a menu with the items of a source and shows the selected
/// item's text. Its value is the selected index (-1 for none).
/// TB: TBSelectDropdown (a TBButton subclass; here it wraps a `Button`)
pub struct SelectDropdown {
    button: Button,
    default_source: Rc<RefCell<StringItemSource>>,
    source: Option<SourceRef>,
    value: i32,
    arrow: Option<WidgetId>,
    /// The menu window, while it is open.
    window: Option<WidgetId>,
    /// The open menu's list (kept, because the menu cannot be asked while it is
    /// reporting a choice).
    window_list: Option<WidgetId>,
}

impl Default for SelectDropdown {
    fn default() -> Self {
        let default_source = StringItemSource::shared();
        SelectDropdown {
            button: Button::default(),
            source: Some(default_source.clone()),
            default_source,
            value: -1,
            arrow: None,
            window: None,
            window_list: None,
        }
    }
}

impl SelectDropdown {
    /// See [`SelectList::default_source`]. TB: GetDefaultSource
    pub fn default_source(&self) -> Rc<RefCell<StringItemSource>> {
        self.default_source.clone()
    }

    pub fn source(&self) -> Option<SourceRef> {
        self.source.clone()
    }

    /// Use another source; its first item gets selected.
    /// TB: SetSource + TBSelectDropdown::OnSourceChanged
    pub fn set_source(&mut self, cx: &mut Ctx, source: Option<SourceRef>) {
        self.source = source;
        self.value = -1;
        let has_items = self.source.as_ref().and_then(|s| s.try_borrow().ok()).is_some_and(|s| s.num_items() > 0);
        if has_items {
            self.set_selected(cx, 0);
        }
    }

    pub fn selected(&self) -> i32 {
        self.value
    }

    /// Select item `index` and show its text. TB: TBSelectDropdown::SetValue
    pub fn set_selected(&mut self, cx: &mut Ctx, index: i32) {
        if index == self.value {
            return;
        }
        let Some(source) = self.source.clone() else { return };
        self.value = index;
        let text = match (usize::try_from(index), source.try_borrow()) {
            (Err(_), _) => Some(String::new()),
            (Ok(i), Ok(src)) if i < src.num_items() => Some(String::from(src.item_string(i))),
            _ => None,
        };
        if let Some(text) = text {
            self.button.set_text(cx, &text);
        }
        cx.emit_changed(Value::Int(index));
    }

    /// The id of the selected item. TB: GetSelectedItemID
    pub fn selected_item_id(&self) -> TbId {
        let (Ok(i), Some(src)) = (usize::try_from(self.value), self.source.as_ref()) else { return TbId::NONE };
        src.try_borrow().ok().filter(|s| i < s.num_items()).map_or(TbId::NONE, |s| s.item_id(i))
    }

    /// The menu window, if it is open. TB: GetMenuIfOpen
    pub fn menu_if_open(&self, cx: &Ctx) -> Option<WidgetId> {
        self.window.filter(|&w| cx.exists(w))
    }

    /// Open the menu, if the source has items. TB: OpenWindow
    pub fn open_window(&mut self, cx: &mut Ctx) {
        if self.menu_if_open(cx).is_some() {
            return;
        }
        let Some(source) = self.source.clone() else { return };
        if !source.try_borrow().is_ok_and(|s| s.num_items() > 0) {
            return;
        }
        let me = cx.this();
        let id = crate::tbid!("TBSelectDropdown.window");
        let menu = cx.create_menu(me, id);
        cx.set_skin(menu, id);
        if cx.show_menu_window(menu, source, &PopupAlignment::default(), self.value) {
            self.window = Some(menu);
            self.window_list = cx.get::<MenuWindow>(menu).and_then(|m| m.list());
        } else {
            cx.remove(menu);
        }
    }

    /// TB: CloseWindow
    pub fn close_window(&mut self, cx: &mut Ctx) {
        if let Some(w) = self.menu_if_open(cx) {
            cx.close_window(w);
        }
    }

    fn menu_list(&self, cx: &Ctx) -> Option<WidgetId> {
        self.menu_if_open(cx)?;
        self.window_list.filter(|&l| cx.exists(l))
    }
}

impl Widget for SelectDropdown {
    fn type_name(&self) -> &'static str {
        "SelectDropdown"
    }

    /// TB: TBSelectDropdown::TBSelectDropdown
    fn on_create(&mut self, cx: &mut Ctx) {
        self.button.on_create(cx);
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBSelectDropdown"));
        if let Some(l) = self.button.layout() {
            let arrow = cx.add(l, SkinImage);
            cx.set_skin_quiet(arrow, crate::tbid!("TBSelectDropdown.arrow"));
            self.arrow = Some(arrow);
        }
    }

    /// TB: TBSelectDropdown::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        self.button.on_inflate(cx, res);
        if let Ok(mut source) = self.default_source.try_borrow_mut() {
            source.read_items(res);
        }
        // See SelectList::on_inflate: without `value`, TB selects the first item.
        if res.get("value").is_none() {
            self.set_selected(cx, 0);
        }
    }

    fn on_removed(&mut self, cx: &mut Ctx) {
        self.close_window(cx);
    }

    fn value(&self) -> Value {
        Value::Int(self.value)
    }

    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        self.set_selected(cx, v.as_int());
    }

    /// TB: TBSelectDropdown::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let me = cx.this();
        if ev.target == me && ev.kind == EventKind::Click {
            // Open the menu, or take its value and close it if it is open (a click by
            // keyboard on this button while the menu is up).
            match self.menu_if_open(cx) {
                Some(menu) => {
                    let value = self.menu_list(cx).map(|l| cx.value(l).as_int());
                    cx.die(menu);
                    if let Some(v) = value {
                        self.set_selected(cx, v);
                    }
                }
                None => self.open_window(cx),
            }
            return true;
        }
        if ev.kind == EventKind::Click && self.menu_if_open(cx) == Some(ev.target) {
            // An item was chosen. The menu's own list knows the choice, unless it is
            // the one sending this click (it is busy then): the index comes with
            // the event. A choice in a sub menu leaves the list's value alone, and
            // the index with the event is then one of the sub menu's.
            // TB: TBSelectDropdown::OnEvent (GetMenuIfOpen()->GetList()->GetValue())
            let value = self
                .menu_list(cx)
                .and_then(|l| cx.get::<SelectList>(l).map(|l| l.selected()))
                .or_else(|| ev.value.as_ref().map(|v| v.as_int()));
            if let Some(v) = value {
                self.set_selected(cx, v);
            }
            return true;
        }
        if ev.target == me && self.source.is_some() && matches!(ev.kind, EventKind::KeyDown | EventKind::KeyUp) {
            // Keys go to the list of the open menu.
            if let Some(list) = self.menu_list(cx) {
                let mut redirected = ev.clone();
                redirected.target = list;
                return cx.invoke_event(redirected);
            }
        }
        false
    }

    // The rest is the button's.
    fn content_root(&self) -> Option<WidgetId> {
        self.button.content_root()
    }
    fn arrange(&mut self, cx: &mut Ctx, old: Size, new: Size) {
        self.button.arrange(cx, old, new)
    }
    fn on_skin_changed(&mut self, cx: &mut Ctx) {
        self.button.on_skin_changed(cx)
    }
    fn on_process(&mut self, cx: &mut Ctx) {
        self.button.on_process(cx)
    }
    fn hit_test(&self, this: NodeRef, p: Point) -> HitStatus {
        self.button.hit_test(this, p)
    }
    fn on_capture_changed(&mut self, cx: &mut Ctx, captured: bool) {
        self.button.on_capture_changed(cx, captured)
    }
    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {
        self.button.on_message(cx, msg)
    }
    fn text(&self) -> Option<&str> {
        self.button.text()
    }
    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        self.button.set_text(cx, text)
    }
    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {
        self.button.set_axis(cx, axis)
    }
    fn axis(&self) -> Axis {
        self.button.axis()
    }
}
