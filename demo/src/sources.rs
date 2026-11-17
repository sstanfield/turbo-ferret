//! The demo's item sources. TB: Demo01.cpp (name_source, advanced_source,
//! popup_menu_source) and ListWindow.cpp (AdvancedItem, AdvancedItemSource)

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::layout::stack::{LayoutDistribution, LayoutDistributionPosition};
use turbo_ferret::widgets::{Layout, SelectItemSource, Sort, SourceChanges, SourceRef, StringItem, StringItemSource};
use turbo_ferret::*;

const GIRL_NAMES: &[&str] = &[
    "Maja", "Alice", "Julia", "Linnéa", "Wilma", "Ella", "Elsa", "Emma", "Alva", "Olivia", "Molly", "Ebba", "Klara",
    "Nellie", "Agnes", "Isabelle", "Ida", "Elin", "Ellen", "Moa", "Emilia", "Nova", "Alma", "Saga", "Amanda",
    "Isabella", "Lilly", "Alicia", "Astrid", "Matilda", "Tuva", "Tilde", "Stella", "Felicia", "Elvira", "Tyra",
    "Hanna", "Sara", "Vera", "Thea", "Freja", "Lova", "Selma", "Meja", "Signe", "Ester", "Lovisa", "Ellie", "Lea",
    "Tilda", "Tindra", "Sofia", "Nora", "Nathalie", "Leia", "Filippa", "Siri", "Emelie", "Inez", "Edith", "Stina",
    "Liv", "Lisa", "Linn", "Tove", "Emmy", "Livia", "Jasmine", "Evelina", "Cornelia", "Märta", "Svea", "Ingrid", "My",
    "Rebecca", "Joline", "Mira", "Ronja", "Hilda", "Melissa", "Anna", "Frida", "Maria", "Iris", "Josefine", "Elise",
    "Elina", "Greta", "Vilda", "Minna", "Lina", "Hedda", "Nicole", "Kajsa", "Majken", "Sofie", "Annie", "Juni",
    "Novalie", "Hedvig",
];

const BOY_NAMES: &[&str] = &[
    "Oscar",
    "William",
    "Lucas",
    "Elias",
    "Alexander",
    "Hugo",
    "Oliver",
    "Theo",
    "Liam",
    "Leo",
    "Viktor",
    "Erik",
    "Emil",
    "Isak",
    "Axel",
    "Filip",
    "Anton",
    "Gustav",
    "Edvin",
    "Vincent",
    "Arvid",
    "Albin",
    "Ludvig",
    "Melvin",
    "Noah",
    "Charlie",
    "Max",
    "Elliot",
    "Viggo",
    "Alvin",
    "Alfred",
    "Theodor",
    "Adam",
    "Olle",
    "Wilmer",
    "Benjamin",
    "Simon",
    "Nils",
    "Noel",
    "Jacob",
    "Leon",
    "Rasmus",
    "Kevin",
    "Linus",
    "Casper",
    "Gabriel",
    "Jonathan",
    "Milo",
    "Melker",
    "Felix",
    "Love",
    "Ville",
    "Sebastian",
    "Sixten",
    "Carl",
    "Malte",
    "Neo",
    "David",
    "Joel",
    "Adrian",
    "Valter",
    "Josef",
    "Jack",
    "Hampus",
    "Samuel",
    "Mohammed",
    "Alex",
    "Tim",
    "Daniel",
    "Vilgot",
    "Wilhelm",
    "Harry",
    "Milton",
    "Maximilian",
    "Robin",
    "Sigge",
    "Måns",
    "Eddie",
    "Elton",
    "Vidar",
    "Hjalmar",
    "Loke",
    "Elis",
    "August",
    "John",
    "Hannes",
    "Sam",
    "Frank",
    "Svante",
    "Marcus",
    "Mio",
    "Otto",
    "Ali",
    "Johannes",
    "Fabian",
    "Ebbe",
    "Aron",
    "Julian",
    "Elvin",
    "Ivar",
];

/// A name with extra data. TB: AdvancedItem
pub struct AdvancedItem {
    pub name: String,
    pub id: TbId,
    pub male: bool,
    pub checked: bool,
}

/// A source with its own item type, filter and item widgets. TB: AdvancedItemSource
#[derive(Default)]
pub struct AdvancedSource {
    items: Vec<AdvancedItem>,
    changes: SourceChanges,
}

impl AdvancedSource {
    pub fn add(&mut self, name: &str, id: TbId, male: bool) {
        self.items.push(AdvancedItem { name: name.to_string(), id, male, checked: false });
        self.changes.items_changed();
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
            self.changes.items_changed();
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.changes.items_changed();
    }

    /// Check or uncheck an item. `viewer` is the list whose check box did it; it
    /// shows the change already, the other viewers update.
    pub fn set_checked(&mut self, index: usize, checked: bool, viewer: Option<WidgetId>) {
        if let Some(item) = self.items.get_mut(index) {
            item.checked = checked;
            self.changes.item_changed(index, viewer);
        }
    }

    pub fn item(&self, index: usize) -> Option<&AdvancedItem> {
        self.items.get(index)
    }
}

impl SelectItemSource for AdvancedSource {
    fn num_items(&self) -> usize {
        self.items.len()
    }
    fn item_string(&self, index: usize) -> &str {
        self.items.get(index).map_or("", |i| &i.name)
    }
    fn item_id(&self, index: usize) -> TbId {
        self.items.get(index).map_or(TbId::NONE, |i| i.id)
    }
    fn sort(&self) -> Sort {
        Sort::Ascending
    }

    /// Hits on the extra data count too. TB: AdvancedItemSource::Filter
    fn filter(&self, index: usize, filter: &str) -> bool {
        let Some(item) = self.items.get(index) else { return false };
        let gender = if item.male { "Male" } else { "Female" };
        widgets::select_item::contains_ignore_case(&item.name, filter)
            || widgets::select_item::contains_ignore_case(gender, filter)
    }

    /// A row made from a resource file: check box, icon, name and info, delete
    /// button. Its clicks are handled by the demo's listener.
    /// TB: AdvancedItemWidget::AdvancedItemWidget
    fn create_item_widget(&self, ui: &mut Ui, index: usize, _viewer: WidgetId) -> Option<WidgetId> {
        let item = self.items.get(index)?;
        let mut layout = Layout::default();
        layout.distribution = LayoutDistribution::Gravity;
        layout.distribution_position = LayoutDistributionPosition::LeftTop;
        layout.paint_overflow_fadeout = false;
        let row = ui.create(layout);
        ui.set_skin(row, tbid!("TBSelectItem"));
        if let Err(e) = ui.inflate_file(row, "demo01/ui_resources/test_list_item.tb.txt") {
            eprintln!("demo: list item: {e}");
        }
        if let Some(check) = ui.find(row, tbid!("check")) {
            ui.set_value(check, item.checked as i32);
        }
        if let Some(name) = ui.find(row, tbid!("name")) {
            ui.set_text(name, &item.name);
        }
        if let Some(info) = ui.find(row, tbid!("info")) {
            ui.set_text(info, if item.male { "Male" } else { "Female" });
        }
        Some(row)
    }

    fn changes(&self) -> &SourceChanges {
        &self.changes
    }
}

/// The sources the demo windows share.
#[derive(Clone)]
pub struct Sources {
    pub names: Rc<RefCell<StringItemSource>>,
    pub advanced: Rc<RefCell<AdvancedSource>>,
    pub popup_menu: Rc<RefCell<StringItemSource>>,
}

impl Sources {
    /// TB: DemoApplication::Init (which takes every other name)
    pub fn new() -> Sources {
        let advanced = Rc::new(RefCell::new(AdvancedSource::default()));
        let names = StringItemSource::shared();
        for n in BOY_NAMES.iter().step_by(2) {
            advanced.borrow_mut().add(n, tbid!("boy_item"), true);
        }
        for n in GIRL_NAMES.iter().step_by(2) {
            advanced.borrow_mut().add(n, tbid!("girl_item"), false);
        }
        for n in GIRL_NAMES.iter().step_by(2) {
            names.borrow_mut().add(StringItem::with_id(n, tbid!("girl_item")));
        }
        for n in BOY_NAMES.iter().step_by(2) {
            names.borrow_mut().add(StringItem::with_id(n, tbid!("boy_item")));
        }
        names.borrow_mut().set_sort(Sort::Ascending);

        // A menu with sub menus, one of them itself, to test sub menu support.
        let popup_menu = StringItemSource::shared();
        let (menu_ref, names_ref): (SourceRef, SourceRef) = (popup_menu.clone(), names.clone());
        let mut m = popup_menu.borrow_mut();
        m.add(StringItem::with_id("Option 1", tbid!("opt 1")).image(tbid!("Icon16")));
        m.add(StringItem::with_id("Option 2", tbid!("opt 2")));
        m.add(StringItem::new("-"));
        m.add(StringItem::with_sub_source("Same submenu", &menu_ref));
        m.add(StringItem::with_sub_source("Long submenu", &names_ref));
        drop(m);
        Sources { names, advanced, popup_menu }
    }
}

impl Default for Sources {
    fn default() -> Self {
        Sources::new()
    }
}
