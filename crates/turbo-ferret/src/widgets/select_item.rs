//! Item sources for list widgets, and the default item widgets.
//! TB: tb_select_item.cpp (TBSelectItemSource, TBSelectItemSourceList,
//! TBGenericStringItem, TBSimpleLayoutItemWidget)
//!
//! TB sources kept a list of their viewers and called them on every change. Here a
//! source is shared (`Rc<RefCell<..>>`) and carries a change counter
//! ([`SourceChanges`]); viewers compare it with the last one they saw when they are
//! processed. Nothing has to unregister, and a source can never call into a viewer
//! that is gone.

use alloc::rc::{Rc, Weak};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::arena::WidgetId;
use crate::event::{Event, EventKind};
use crate::geom::Size;
use crate::id::TbId;
use crate::layout::size::Gravity;
use crate::layout::stack::LayoutDistribution;
use crate::reader::id_from_value;
use crate::res::ResNode;
use crate::ui::{Ctx, Ui};
use crate::value::Value;
use crate::widget::{Align, Widget, WidgetState, ZRel};

use super::popup::PopupAlignment;
use super::text::{TextAlign, TextField};
use super::{Layout, Separator, SkinImage};

/// TB: TB_SORT
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Sort {
    /// Items appear in list order.
    #[default]
    None,
    Ascending,
    Descending,
}

/// A shared item source, as list widgets hold it.
pub type SourceRef = Rc<RefCell<dyn SelectItemSource>>;
/// A source reference that does not keep the source alive (sub menus).
pub type WeakSourceRef = Weak<RefCell<dyn SelectItemSource>>;

/// What changed in a source, for its viewers. A source embeds one and calls its
/// methods when items change. TB: InvokeItemChanged / Added / Removed / AllItemsRemoved
#[derive(Clone, Debug, Default)]
pub struct SourceChanges {
    version: u32,
    /// The last change, if it was a change to one item.
    last_item: Option<(usize, Option<WidgetId>)>,
}

impl SourceChanges {
    /// Counts every change.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Items were added or removed (viewers rebuild their lists).
    pub fn items_changed(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.last_item = None;
    }

    /// One item changed in a way that should update viewers. `exclude_viewer` is the
    /// viewer that made the change and shows it already. TB: InvokeItemChanged
    pub fn item_changed(&mut self, index: usize, exclude_viewer: Option<WidgetId>) {
        self.version = self.version.wrapping_add(1);
        self.last_item = Some((index, exclude_viewer));
    }

    /// What a viewer that last saw `seen` has to do.
    pub fn since(&self, seen: u32, viewer: WidgetId) -> SourceUpdate {
        if seen == self.version {
            return SourceUpdate::None;
        }
        match self.last_item {
            Some((index, exclude)) if seen.wrapping_add(1) == self.version => {
                if exclude == Some(viewer) {
                    SourceUpdate::None
                } else {
                    SourceUpdate::Item(index)
                }
            }
            _ => SourceUpdate::All,
        }
    }
}

/// See [`SourceChanges::since`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SourceUpdate {
    None,
    /// One item changed.
    Item(usize),
    /// Rebuild everything.
    All,
}

/// Provides the items of list widgets (`SelectList`, `SelectDropdown`, menus). The
/// widgets ask for items when they need them and may show only a filtered subset.
///
/// This trait has no storage; [`StringItemSource`] is a ready-made list of
/// [`StringItem`]s. An implementation embeds a [`SourceChanges`] and reports changes
/// to it. TB: TBSelectItemSource
#[allow(unused_variables)]
pub trait SelectItemSource {
    fn num_items(&self) -> usize;

    /// The string of an item. For items with more than one string, the one to sort
    /// by. TB: GetItemString
    fn item_string(&self, index: usize) -> &str;

    /// TB: GetItemID
    fn item_id(&self, index: usize) -> TbId {
        TbId::NONE
    }

    /// The skin image painted before the text. TB: GetItemImage
    fn item_image(&self, index: usize) -> TbId {
        TbId::NONE
    }

    /// The source of the sub menu this item opens. TB: GetItemSubSource
    fn item_sub_source(&self, index: usize) -> Option<SourceRef> {
        None
    }

    /// Whether an item matches a filter text. By default: its string contains the
    /// filter, ignoring ASCII case. TB: Filter
    fn filter(&self, index: usize, filter: &str) -> bool {
        contains_ignore_case(self.item_string(index), filter)
    }

    /// TB: GetSort
    fn sort(&self) -> Sort {
        Sort::None
    }

    /// Create the widget for an item and return it unattached (`ui.create`); the
    /// viewer adds it. By default a text field, a layout with image, text and sub
    /// menu arrow, or a separator for "-" (see [`create_default_item_widget`]).
    ///
    /// The source is borrowed while this runs, so the widgets created here must not
    /// change it from their `on_create`. TB: CreateItemWidget
    fn create_item_widget(&self, ui: &mut Ui, index: usize, viewer: WidgetId) -> Option<WidgetId> {
        let w = create_default_item_widget(
            ui,
            self.item_string(index),
            self.item_image(index),
            self.item_sub_source(index),
        );
        ui.set_id(w, self.item_id(index));
        Some(w)
    }

    /// The change counter viewers watch.
    fn changes(&self) -> &SourceChanges;
}

/// TB: stristr(str, filter) != nullptr
pub fn contains_ignore_case(s: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let (s, n) = (s.as_bytes(), needle.as_bytes());
    s.len() >= n.len() && (0..=s.len() - n.len()).any(|i| s[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// An item with a string, and optionally an id, a skin image and a sub menu.
/// TB: TBGenericStringItem
#[derive(Clone, Default)]
pub struct StringItem {
    pub text: String,
    pub id: TbId,
    pub image: TbId,
    /// The source of a sub menu. Not kept alive by the item (as in TB, sources are
    /// owned by the application), so a source may refer to itself.
    pub sub_source: Option<WeakSourceRef>,
    /// Free to use for anything.
    pub tag: Value,
}

impl StringItem {
    pub fn new(text: &str) -> StringItem {
        StringItem { text: String::from(text), ..Default::default() }
    }

    pub fn with_id(text: &str, id: TbId) -> StringItem {
        StringItem { text: String::from(text), id, ..Default::default() }
    }

    pub fn with_sub_source(text: &str, sub_source: &SourceRef) -> StringItem {
        StringItem { text: String::from(text), sub_source: Some(Rc::downgrade(sub_source)), ..Default::default() }
    }

    pub fn image(mut self, image: TbId) -> StringItem {
        self.image = image;
        self
    }
}

/// A list of [`StringItem`]s. TB: TBGenericStringItemSource
#[derive(Default)]
pub struct StringItemSource {
    items: Vec<StringItem>,
    pub sort: Sort,
    changes: SourceChanges,
}

impl StringItemSource {
    pub fn new() -> StringItemSource {
        StringItemSource::default()
    }

    /// A new source, shared and ready to give to list widgets.
    pub fn shared() -> Rc<RefCell<StringItemSource>> {
        Rc::new(RefCell::new(StringItemSource::default()))
    }

    pub fn items(&self) -> &[StringItem] {
        &self.items
    }

    pub fn item(&self, index: usize) -> Option<&StringItem> {
        self.items.get(index)
    }

    /// Add an item last. TB: AddItem
    pub fn add(&mut self, item: StringItem) {
        self.items.push(item);
        self.changes.items_changed();
    }

    /// Add an item at `index` (clamped to the end).
    pub fn insert(&mut self, index: usize, item: StringItem) {
        self.items.insert(index.min(self.items.len()), item);
        self.changes.items_changed();
    }

    /// TB: DeleteItem
    pub fn remove(&mut self, index: usize) -> Option<StringItem> {
        if index >= self.items.len() {
            return None;
        }
        let item = self.items.remove(index);
        self.changes.items_changed();
        Some(item)
    }

    /// TB: DeleteAllItems
    pub fn clear(&mut self) {
        if !self.items.is_empty() {
            self.items.clear();
            self.changes.items_changed();
        }
    }

    /// Change an item in place and tell the viewers. TB: InvokeItemChanged
    pub fn update(&mut self, index: usize, f: impl FnOnce(&mut StringItem)) {
        if let Some(item) = self.items.get_mut(index) {
            f(item);
            self.changes.item_changed(index, None);
        }
    }

    pub fn set_sort(&mut self, sort: Sort) {
        if self.sort != sort {
            self.sort = sort;
            self.changes.items_changed();
        }
    }

    /// Add the items of a resource node's `items` list (`item: text: "..", id: ..`).
    /// TB: tb_widgets_reader.cpp ReadItems
    pub fn read_items(&mut self, node: &ResNode) {
        let Some(items) = node.get("items") else { return };
        for n in items.children().iter().filter(|n| &*n.name == "item") {
            let id = n.get("id").map(|i| id_from_value(&i.value)).unwrap_or(TbId::NONE);
            self.add(StringItem::with_id(&n.str("text", ""), id));
        }
    }
}

impl SelectItemSource for StringItemSource {
    fn num_items(&self) -> usize {
        self.items.len()
    }
    fn item_string(&self, index: usize) -> &str {
        self.items.get(index).map_or("", |i| &i.text)
    }
    fn item_id(&self, index: usize) -> TbId {
        self.items.get(index).map_or(TbId::NONE, |i| i.id)
    }
    fn item_image(&self, index: usize) -> TbId {
        self.items.get(index).map_or(TbId::NONE, |i| i.image)
    }
    fn item_sub_source(&self, index: usize) -> Option<SourceRef> {
        self.items.get(index)?.sub_source.as_ref()?.upgrade()
    }
    fn sort(&self) -> Sort {
        self.sort
    }
    fn changes(&self) -> &SourceChanges {
        &self.changes
    }
}

/// The default widget for an item, unattached: a separator if the string starts with
/// "-", a [`SelectItemWidget`] if there is an image or a sub menu, else a text field.
/// TB: TBSelectItemSource::CreateItemWidget
pub fn create_default_item_widget(ui: &mut Ui, text: &str, image: TbId, sub_source: Option<SourceRef>) -> WidgetId {
    if sub_source.is_some() || image.is_some() {
        return ui.create(SelectItemWidget { text: String::from(text), image, sub_source, ..Default::default() });
    }
    if text.starts_with('-') {
        let w = ui.create(Separator);
        ui.set_gravity(w, Gravity::ALL);
        ui.set_skin(w, crate::tbid!("TBSelectItem.separator"));
        return w;
    }
    let w = ui.create(TextField { text: String::from(text), align: TextAlign::Left, squeezable: false });
    ui.set_skin(w, crate::tbid!("TBSelectItem"));
    w
}

/// An item with an image, a text and an arrow if it opens a sub menu (which it also
/// handles). TB: TBSimpleLayoutItemWidget, a TBLayout subclass. Here it is a widget
/// with a layout inside that takes no input, so the item itself is hovered and
/// clicked.
#[derive(Default)]
pub struct SelectItemWidget {
    text: String,
    image: TbId,
    sub_source: Option<SourceRef>,
    layout: Option<WidgetId>,
    /// The sub menu window, while it is open.
    menu: Option<WidgetId>,
}

impl SelectItemWidget {
    /// TB: TBSimpleLayoutItemWidget::OpenSubMenu
    fn open_sub_menu(&mut self, cx: &mut Ctx) {
        if self.menu.is_some_and(|m| cx.exists(m)) {
            return;
        }
        let Some(source) = self.sub_source.clone() else { return };
        let me = cx.this();
        self.menu = cx.show_menu(me, crate::tbid!("submenu"), source, &PopupAlignment::new(Align::Right), -1);
        if self.menu.is_some() {
            cx.set_state(me, WidgetState::SELECTED, true);
        }
    }

    /// TB: TBSimpleLayoutItemWidget::CloseSubMenu
    fn close_sub_menu(&mut self, cx: &mut Ctx) {
        let Some(menu) = self.menu.take() else { return };
        let me = cx.this();
        cx.set_state(me, WidgetState::SELECTED, false);
        if cx.exists(menu) && !cx.is_dying(menu) {
            cx.close_window(menu);
        }
    }
}

impl Widget for SelectItemWidget {
    fn type_name(&self) -> &'static str {
        "SelectItemWidget"
    }

    /// TB: TBSimpleLayoutItemWidget::TBSimpleLayoutItemWidget
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBSelectItem"));
        let mut layout = Layout::default();
        layout.distribution = LayoutDistribution::Available;
        layout.paint_overflow_fadeout = false;
        let layout = cx.add(me, layout);
        cx.set_gravity(layout, Gravity::ALL);
        cx.set_ignore_input(layout, true);
        if self.image.is_some() {
            let img = cx.add(layout, SkinImage);
            cx.set_skin(img, self.image);
        }
        cx.add(layout, TextField { text: self.text.clone(), align: TextAlign::Left, squeezable: false });
        if self.sub_source.is_some() {
            let arrow = cx.add(layout, SkinImage);
            cx.set_skin(arrow, crate::tbid!("arrow.right"));
        }
        self.layout = Some(layout);
    }

    fn arrange(&mut self, cx: &mut Ctx, _old: Size, _new: Size) {
        if let Some(l) = self.layout {
            let pr = cx.padding_rect(cx.this());
            cx.set_rect(l, pr);
        }
    }

    fn on_skin_changed(&mut self, cx: &mut Ctx) {
        if let Some(l) = self.layout {
            let pr = cx.padding_rect(cx.this());
            cx.set_rect(l, pr);
        }
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }

    /// TB: TBSimpleLayoutItemWidget::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if self.sub_source.is_some() && ev.kind == EventKind::Click && ev.target == cx.this() {
            self.open_sub_menu(cx);
            return true;
        }
        false
    }

    /// The sub menu closed by itself: drop the selected look.
    /// TB: TBSimpleLayoutItemWidget::OnWidgetDelete
    fn on_process(&mut self, cx: &mut Ctx) {
        if self.menu.is_some_and(|m| !cx.exists(m) || cx.is_dying(m)) {
            self.menu = None;
            let me = cx.this();
            cx.set_state(me, WidgetState::SELECTED, false);
        }
    }

    /// TB: TBSimpleLayoutItemWidget::~TBSimpleLayoutItemWidget
    fn on_removed(&mut self, cx: &mut Ctx) {
        self.close_sub_menu(cx);
    }
}

/// Add `widget` to `parent` after `reference` (or first, without one).
pub(crate) fn add_item_after(ui: &mut Ui, parent: WidgetId, widget: WidgetId, reference: Option<WidgetId>) {
    match reference {
        Some(r) => {
            ui.attach_relative(r, widget, ZRel::After);
        }
        None => {
            ui.attach(parent, widget, crate::widget::Z::Top);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_ignores_ascii_case() {
        assert!(contains_ignore_case("Hello World", "WORLD"));
        assert!(contains_ignore_case("Hello", ""));
        assert!(!contains_ignore_case("Hello", "help"));
        assert!(!contains_ignore_case("ab", "abc"));
    }

    #[test]
    fn changes_tell_viewers_what_to_do() {
        let mut c = SourceChanges::default();
        use crate::arena::ArenaKey;
        let id = |i| WidgetId::from_parts(i, core::num::NonZeroU32::MIN);
        let (a, b) = (id(1), id(2));
        assert_eq!(c.since(0, a), SourceUpdate::None);
        c.item_changed(3, Some(a));
        assert_eq!(c.since(0, a), SourceUpdate::None, "the viewer that made the change");
        assert_eq!(c.since(0, b), SourceUpdate::Item(3));
        c.item_changed(4, None);
        assert_eq!(c.since(1, a), SourceUpdate::Item(4));
        assert_eq!(c.since(0, a), SourceUpdate::All, "missed more than one change");
        c.items_changed();
        assert_eq!(c.since(2, a), SourceUpdate::All);
        assert_eq!(c.since(3, a), SourceUpdate::None);
    }
}
