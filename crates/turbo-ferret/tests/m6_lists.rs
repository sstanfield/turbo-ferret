//! M6: item sources, select lists, dropdowns, popup, menu and message windows, and
//! the inline select (headless).

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::widgets::*;
use turbo_ferret::*;

fn ui() -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(800, 600));
    ui.set_time(1000);
    ui.lang_mut().set("TBList.header", "Showing %d of %d");
    ui.lang_mut().set("TBMessageWindow.ok", "Ok");
    ui.lang_mut().set("TBMessageWindow.cancel", "Cancel");
    ui.lang_mut().set("TBMessageWindow.yes", "Yes");
    ui.lang_mut().set("TBMessageWindow.no", "No");
    ui
}

fn click(ui: &mut Ui, id: WidgetId) {
    let r = ui.rect(id);
    let c = ui.to_root(id, Point::new(r.w / 2, r.h / 2));
    ui.pointer_down(c, 1, Modifiers::NONE);
    ui.pointer_up(c, Modifiers::NONE);
}

/// Let fades finish and removed windows go away.
fn settle(ui: &mut Ui) {
    for _ in 0..40 {
        ui.set_time(ui.now() + 16);
        ui.update();
    }
}

type Log = Rc<RefCell<Vec<(EventKind, TbId, TbId, Option<i32>)>>>;

/// Record (kind, target id, ref_id, value) of the events that reach `id`'s own
/// listener or bubble to it.
struct Recorder(Log);
impl Widget for Recorder {
    fn type_name(&self) -> &'static str {
        "Recorder"
    }
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if matches!(ev.kind, EventKind::Click | EventKind::Changed) {
            let tid = cx.node(ev.target).map_or(TbId::NONE, |n| n.id());
            self.0.borrow_mut().push((ev.kind, tid, ev.ref_id, ev.value.as_ref().map(|v| v.as_int())));
        }
        false
    }
}

fn recorder(ui: &mut Ui) -> (WidgetId, Log) {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let root = ui.root();
    let r = ui.add(root, Recorder(log.clone()));
    ui.set_rect(r, Rect::new(0, 0, 400, 400));
    (r, log)
}

fn names() -> Rc<RefCell<StringItemSource>> {
    let src = StringItemSource::shared();
    for (name, id) in [("Cherry", "c"), ("apple", "a"), ("Banana", "b"), ("date", "d")] {
        src.borrow_mut().add(StringItem::with_id(name, TbId::new(id)));
    }
    src
}

/// A 150x100 list at (10, 10) inside a recorder, showing `src`.
fn list(ui: &mut Ui, src: &Rc<RefCell<StringItemSource>>) -> (WidgetId, Log) {
    let (rec, log) = recorder(ui);
    let l = ui.add(rec, SelectList::default());
    ui.with_widget::<SelectList, _>(l, |w, cx| w.set_source(cx, Some(src.clone())));
    ui.set_rect(l, Rect::new(10, 10, 150, 100));
    ui.update();
    (l, log)
}

fn item_texts(ui: &Ui, l: WidgetId) -> Vec<String> {
    let layout = ui.get::<SelectList>(l).unwrap().item_layout().unwrap();
    ui.children(layout).iter().map(|&c| ui.text(c).unwrap_or("<none>").to_string()).collect()
}

fn item(ui: &Ui, l: WidgetId, index: i32) -> WidgetId {
    ui.get::<SelectList>(l).unwrap().item_widget(ui, index).expect("item widget")
}

#[test]
fn string_source_basics() {
    let src = names();
    let mut s = src.borrow_mut();
    assert_eq!(s.num_items(), 4);
    assert_eq!((s.item_string(1), s.item_id(1)), ("apple", tbid!("a")));
    assert_eq!((s.item_string(9), s.item_id(9)), ("", TbId::NONE));
    assert!(s.filter(2, "NAN") && !s.filter(2, "x"));
    let v = s.changes().version();
    s.insert(0, StringItem::new("first").image(tbid!("Icon16")));
    assert_eq!((s.item_string(0), s.item_image(0)), ("first", tbid!("Icon16")));
    assert_eq!(s.remove(0).unwrap().text, "first");
    assert!(s.remove(99).is_none());
    s.update(0, |i| i.text = "Cherries".into());
    assert_eq!(s.item_string(0), "Cherries");
    assert_eq!(s.changes().version(), v + 3);
    s.clear();
    s.clear();
    assert_eq!((s.num_items(), s.changes().version()), (0, v + 4));
}

#[test]
fn list_shows_items_and_selects_by_click() {
    let mut ui = ui();
    let src = names();
    let (l, log) = list(&mut ui, &src);
    assert_eq!(item_texts(&ui, l), ["Cherry", "apple", "Banana", "date"]);
    assert_eq!(ui.value(l), Value::Int(-1));
    assert!(ui.node(l).unwrap().is_focusable());

    let banana = item(&ui, l, 2);
    assert_eq!(ui.node(banana).unwrap().id(), tbid!("b"));
    click(&mut ui, banana);
    assert_eq!(ui.value(l), Value::Int(2));
    assert!(ui.has_state(banana, WidgetState::SELECTED));
    assert_eq!(ui.get::<SelectList>(l).unwrap().selected_item_id(), tbid!("b"));
    assert_eq!(ui.focused(), Some(l));
    // First Changed, then Click, both from the list with the item's id and index.
    assert_eq!(
        *log.borrow(),
        [(EventKind::Changed, TbId::NONE, tbid!("b"), Some(2)), (EventKind::Click, TbId::NONE, tbid!("b"), Some(2)),]
    );

    // Another item: the old one is deselected. The same item again: only a click.
    let apple = item(&ui, l, 1);
    click(&mut ui, apple);
    assert!(!ui.has_state(banana, WidgetState::SELECTED) && ui.has_state(apple, WidgetState::SELECTED));
    log.borrow_mut().clear();
    click(&mut ui, apple);
    assert_eq!(log.borrow().len(), 1);
    ui.set_value(l, -1);
    assert!(!ui.has_state(apple, WidgetState::SELECTED));
}

#[test]
fn list_sorts_and_filters() {
    let mut ui = ui();
    let src = names();
    let (l, _log) = list(&mut ui, &src);
    ui.set_value(l, 0); // Cherry
    src.borrow_mut().set_sort(Sort::Ascending);
    ui.update();
    // Byte order, as TB's strcmp: capitals first.
    assert_eq!(item_texts(&ui, l), ["Banana", "Cherry", "apple", "date"]);
    // The value is the index in the source, not the position in the list.
    let cherry = item(&ui, l, 0);
    assert!(ui.has_state(cherry, WidgetState::SELECTED));
    assert_eq!(ui.text(cherry), Some("Cherry"));
    src.borrow_mut().set_sort(Sort::Descending);
    ui.update();
    assert_eq!(item_texts(&ui, l), ["date", "apple", "Cherry", "Banana"]);

    // A filter shows matching items under a header with the counts.
    ui.with_widget::<SelectList, _>(l, |w, cx| w.set_filter(cx, "A"));
    ui.update();
    assert_eq!(item_texts(&ui, l), ["Showing 3 of 4", "date", "apple", "Banana"]);
    let layout = ui.get::<SelectList>(l).unwrap().item_layout().unwrap();
    let header = ui.children(layout)[0];
    assert!(ui.has_state(header, WidgetState::DISABLED));
    // The header is not an item: a click on it selects nothing new.
    ui.set_value(l, 3);
    ui.set_state(header, WidgetState::DISABLED, false);
    click(&mut ui, header);
    assert_eq!(ui.value(l), Value::Int(-1), "the header carries index -1");
    ui.with_widget::<SelectList, _>(l, |w, cx| w.set_filter(cx, ""));
    ui.update();
    assert_eq!(item_texts(&ui, l).len(), 4);
}

#[test]
fn list_follows_source_changes() {
    let mut ui = ui();
    let src = names();
    let (l, _log) = list(&mut ui, &src);
    let (l2, _log2) = list(&mut ui, &src);
    src.borrow_mut().add(StringItem::new("elderberry"));
    assert_eq!(item_texts(&ui, l).len(), 4, "nothing changes before the next update");
    ui.update();
    assert_eq!(item_texts(&ui, l).last().unwrap(), "elderberry");
    assert_eq!(item_texts(&ui, l2).len(), 5, "every viewer follows");

    // One changed item is replaced in place and keeps its state.
    ui.set_value(l, 1);
    let old = item(&ui, l, 1);
    let untouched = item(&ui, l, 0);
    src.borrow_mut().update(1, |i| i.text = "apricot".into());
    ui.update();
    let new = item(&ui, l, 1);
    assert!(new != old && !ui.exists(old));
    assert_eq!(item(&ui, l, 0), untouched);
    assert_eq!(item_texts(&ui, l), ["Cherry", "apricot", "Banana", "date", "elderberry"]);
    assert!(ui.has_state(new, WidgetState::SELECTED));

    // Removing everything empties the list and clears the value.
    src.borrow_mut().clear();
    ui.update();
    assert!(item_texts(&ui, l).is_empty());
    assert_eq!(ui.value(l), Value::Int(-1));

    // A source that is borrowed mutably during an update is read at the next one.
    src.borrow_mut().add(StringItem::new("fig"));
    {
        let _held = src.borrow_mut();
        ui.update();
    }
    assert!(item_texts(&ui, l).is_empty());
    ui.update();
    assert_eq!(item_texts(&ui, l), ["fig"]);
}

#[test]
fn list_keys_move_the_selection_and_scroll() {
    let mut ui = ui();
    let src = StringItemSource::shared();
    for i in 0..30 {
        src.borrow_mut().add(StringItem::new(&format!("Item {i}")));
    }
    src.borrow_mut().insert(1, StringItem::new("-"));
    let (l, _log) = list(&mut ui, &src);
    // Give the items a height (no skin or font in this test).
    let layout = ui.get::<SelectList>(l).unwrap().item_layout().unwrap();
    for c in ui.children(layout).to_vec() {
        ui.set_layout_params(c, Some(LayoutParams::fixed(100, 20)));
    }
    ui.update();
    ui.set_focus(l);
    let key = |ui: &mut Ui, k| ui.key(None, k, Modifiers::NONE, true);
    assert!(key(&mut ui, SpecialKey::Down));
    assert_eq!(ui.value(l), Value::Int(0));
    // The separator is not skipped by itself, but a disabled item is.
    let sep = item(&ui, l, 1);
    assert!(ui.get::<Separator>(sep).is_some());
    ui.set_state(sep, WidgetState::DISABLED, true);
    key(&mut ui, SpecialKey::Down);
    assert_eq!(ui.value(l), Value::Int(2));
    key(&mut ui, SpecialKey::Up);
    assert_eq!(ui.value(l), Value::Int(0));
    assert!(!key(&mut ui, SpecialKey::Up), "nothing above the first item");

    key(&mut ui, SpecialKey::End);
    assert_eq!(ui.value(l), Value::Int(30));
    ui.update();
    let sc = ui.get::<SelectList>(l).unwrap().scroll_container().unwrap();
    let info = ui.scroll_info(sc).unwrap();
    assert_eq!(info.pos.y, info.max.y, "scrolled to the selected last item");
    assert!(info.max.y > 400);
    key(&mut ui, SpecialKey::Home);
    ui.update();
    assert_eq!(ui.value(l), Value::Int(0));
    assert_eq!(ui.scroll_info(sc).unwrap().pos.y, 0);
    // Other keys scroll the container (PageDown here).
    assert!(key(&mut ui, SpecialKey::PageDown));
    assert!(ui.is_scrolling(sc));
}

#[test]
fn inflated_list_and_dropdown_read_items() {
    let mut ui = ui();
    let (rec, log) = recorder(&mut ui);
    let src = r#"
TBSelectList: id: list
	items
		item: text: "One", id: one
		item: text: "Two", id: 2
		ignored: text: "not an item"
TBSelectDropdown: id: drop
	items
		item: text: "Red", id: red
		item: text: "Green", id: green
TBSelectDropdown: id: drop2, value: 1
	items
		item: text: "Red"
		item: text: "Green"
"#;
    let ids = ui.inflate(rec, src).unwrap();
    ui.update();
    let (l, d, d2) = (ids[0], ids[1], ids[2]);
    assert_eq!(item_texts(&ui, l), ["One", "Two"]);
    // As in TB, the first item is selected when no value is given.
    assert_eq!(ui.value(l), Value::Int(0));
    assert_eq!(ui.get::<SelectList>(l).unwrap().selected_item_id(), tbid!("one"));
    let two = item(&ui, l, 1);
    assert_eq!(ui.node(two).unwrap().id(), TbId(2));
    assert_eq!((ui.value(d), ui.text(d)), (Value::Int(0), Some("Red")));
    assert_eq!(ui.get::<SelectDropdown>(d).unwrap().selected_item_id(), tbid!("red"));
    assert_eq!((ui.value(d2), ui.text(d2)), (Value::Int(1), Some("Green")));
    // The widgets are not attached yet when their value is set, so the application
    // sees no events from inflation.
    assert!(log.borrow().is_empty());
}

#[test]
fn dropdown_opens_a_menu_and_takes_the_choice() {
    let mut ui = ui();
    let (rec, log) = recorder(&mut ui);
    let d = ui.add(rec, SelectDropdown::default());
    ui.set_id(d, tbid!("drop"));
    ui.set_rect(d, Rect::new(20, 20, 120, 24));
    let src = names();
    ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.set_source(cx, Some(src.clone())));
    assert_eq!((ui.value(d), ui.text(d)), (Value::Int(0), Some("Cherry")));
    ui.update();
    log.borrow_mut().clear();

    click(&mut ui, d);
    let menu = ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.menu_if_open(cx)).flatten().expect("menu open");
    let root = ui.root();
    assert_eq!(ui.parent(menu), Some(root));
    assert_eq!(ui.node(menu).unwrap().id(), tbid!("TBSelectDropdown.window"));
    assert_eq!(ui.popup_target(menu), Some(d));
    // Below the dropdown, at least as wide.
    let mr = ui.rect(menu);
    assert_eq!((mr.x, mr.y), (20, 44));
    assert!(mr.w >= 120);
    // The dropdown keeps the focus, and the current item is selected in the list.
    assert_eq!(ui.focused(), Some(d));
    let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
    assert_eq!(ui.value(list), Value::Int(0));
    assert!(log.borrow().is_empty(), "opening sends no events to the application");

    // Choose "Banana".
    for c in ui.children(ui.get::<SelectList>(list).unwrap().item_layout().unwrap()).to_vec() {
        ui.set_layout_params(c, Some(LayoutParams::fixed(100, 20)));
    }
    ui.resize_to_preferred(menu);
    ui.update();
    let banana = item(&ui, list, 2);
    click(&mut ui, banana);
    assert_eq!((ui.value(d), ui.text(d)), (Value::Int(2), Some("Banana")));
    assert!(ui.is_dying(menu) || !ui.exists(menu));
    settle(&mut ui);
    assert!(!ui.exists(menu));
    // The application sees the dropdown change (and the menu's own events, which
    // travel through the dropdown).
    assert!(log.borrow().contains(&(EventKind::Changed, tbid!("drop"), TbId::NONE, Some(2))));

    // By keyboard: Enter opens, Down moves in the list, Enter takes it.
    let key = |ui: &mut Ui, k: SpecialKey| {
        ui.key(None, k, Modifiers::NONE, true);
        ui.key(None, k, Modifiers::NONE, false);
    };
    assert_eq!(ui.focused(), Some(d));
    key(&mut ui, SpecialKey::Enter);
    let menu = ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.menu_if_open(cx)).flatten().expect("menu open");
    ui.update();
    key(&mut ui, SpecialKey::Down);
    assert_eq!(ui.value(d), Value::Int(2), "not taken yet");
    key(&mut ui, SpecialKey::Enter);
    assert_eq!((ui.value(d), ui.text(d)), (Value::Int(3), Some("date")));
    settle(&mut ui);
    assert!(!ui.exists(menu));

    // Esc closes without a change.
    key(&mut ui, SpecialKey::Enter);
    let menu = ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.menu_if_open(cx)).flatten().unwrap();
    key(&mut ui, SpecialKey::Up);
    key(&mut ui, SpecialKey::Esc);
    settle(&mut ui);
    assert!(!ui.exists(menu));
    assert_eq!(ui.value(d), Value::Int(3));
}

#[test]
fn popup_closes_on_outside_press_focus_and_target_removal() {
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Button::new("target"));
    ui.set_rect(a, Rect::new(10, 10, 80, 20));
    let b = ui.add(root, Button::new("other"));
    ui.set_rect(b, Rect::new(300, 300, 80, 20));
    let open = |ui: &mut Ui| {
        let p = ui.create(PopupWindow::new(a));
        let inner = ui.add(p, Button::new("inside"));
        ui.set_layout_params(inner, Some(LayoutParams::fixed(60, 20)));
        assert!(ui.show_popup(p, &PopupAlignment::default()));
        ui.set_rect(inner, Rect::new(0, 0, 60, 20));
        ui.update();
        (p, inner)
    };
    let closed = |ui: &mut Ui, p: WidgetId| {
        let dying = ui.is_dying(p);
        settle(ui);
        dying && !ui.exists(p)
    };

    // A press inside does not close it; its events go on through the target.
    let (p, inner) = open(&mut ui);
    assert_eq!(ui.rect(p).origin(), Point::new(10, 30));
    assert_eq!(ui.event_destination(p), Some(a));
    assert!(ui.is_event_destination_for(a, inner));
    click(&mut ui, inner);
    assert!(!ui.is_dying(p));
    // A press outside closes it.
    click(&mut ui, b);
    assert!(closed(&mut ui, p));

    // Focus moving outside closes it.
    ui.set_focus(a);
    let (p, _) = open(&mut ui);
    ui.set_focus(b);
    assert!(closed(&mut ui, p));

    // Esc inside closes it.
    let (p, inner) = open(&mut ui);
    let mut ev = Event::new(EventKind::KeyDown, inner);
    ev.special = SpecialKey::Esc;
    assert!(ui.invoke_event(ev));
    assert!(closed(&mut ui, p));

    // The target dying or being removed closes it.
    let (p, _) = open(&mut ui);
    ui.die(a);
    assert!(closed(&mut ui, p));
    let (p2, _) = {
        let p = ui.create(PopupWindow::new(b));
        ui.show_popup(p, &PopupAlignment::default());
        (p, ())
    };
    ui.remove(b);
    assert!(closed(&mut ui, p2));
    // A popup for a target that is gone cannot be shown.
    let p3 = ui.create(PopupWindow::new(b));
    assert!(!ui.show_popup(p3, &PopupAlignment::default()));
}

#[test]
fn popup_alignment_keeps_the_popup_in_the_root() {
    let mut ui = ui();
    let root = ui.root();
    let t = ui.add(root, Plain);
    let popup = ui.create(Plain);
    ui.set_layout_params(popup, Some(LayoutParams::fixed(100, 50)));
    let at = |ui: &mut Ui, target: Rect, a: PopupAlignment| {
        ui.set_rect(t, target);
        a.aligned_rect(ui, popup, t)
    };
    // Below, and as wide as the target.
    assert_eq!(at(&mut ui, Rect::new(10, 10, 200, 20), PopupAlignment::default()), Rect::new(10, 30, 200, 50));
    // No room below: above.
    assert_eq!(at(&mut ui, Rect::new(10, 570, 60, 20), PopupAlignment::default()), Rect::new(10, 520, 100, 50));
    // Top, and no room above: below.
    assert_eq!(at(&mut ui, Rect::new(10, 100, 60, 20), PopupAlignment::new(Align::Top)), Rect::new(10, 50, 100, 50));
    assert_eq!(at(&mut ui, Rect::new(10, 10, 60, 20), PopupAlignment::new(Align::Top)), Rect::new(10, 30, 100, 50));
    // Right of the target, or left when there is no room; kept inside vertically.
    assert_eq!(at(&mut ui, Rect::new(10, 10, 60, 20), PopupAlignment::new(Align::Right)), Rect::new(70, 10, 100, 50));
    assert_eq!(
        at(&mut ui, Rect::new(720, 580, 60, 20), PopupAlignment::new(Align::Right)),
        Rect::new(620, 550, 100, 50)
    );
    assert_eq!(at(&mut ui, Rect::new(200, 10, 60, 20), PopupAlignment::new(Align::Left)), Rect::new(100, 10, 100, 50));
    // At a point: moved into view horizontally.
    let a = PopupAlignment::at(Point::new(780, 100), Align::Bottom);
    assert_eq!(at(&mut ui, Rect::ZERO, a), Rect::new(700, 100, 100, 50));
    let a = PopupAlignment::at_with_offset(Point::new(50, 580), Point::new(0, 10));
    assert_eq!(at(&mut ui, Rect::ZERO, a), Rect::new(50, 530, 100, 50));
}

#[test]
fn menu_with_sub_menus_reports_through_the_top_menu() {
    let mut ui = ui();
    let (rec, log) = recorder(&mut ui);
    let b = ui.add(rec, Button::new("menu"));
    ui.set_id(b, tbid!("button"));
    ui.set_rect(b, Rect::new(10, 10, 80, 20));

    let sub: SourceRef = names();
    let top = StringItemSource::shared();
    top.borrow_mut().add(StringItem::with_id("Option 1", tbid!("opt1")).image(tbid!("Icon16")));
    top.borrow_mut().add(StringItem::new("-"));
    top.borrow_mut().add(StringItem::with_sub_source("More", &sub));
    // A source can refer to itself: the item does not keep it alive.
    let top_ref: SourceRef = top.clone();
    top.borrow_mut().add(StringItem::with_sub_source("Same again", &top_ref));
    assert_eq!(Rc::strong_count(&top), 2);

    let size_items = |ui: &mut Ui, menu: WidgetId| {
        let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
        let layout = ui.get::<SelectList>(list).unwrap().item_layout().unwrap();
        for c in ui.children(layout).to_vec() {
            ui.set_layout_params(c, Some(LayoutParams::fixed(100, 20)));
        }
        ui.resize_to_preferred(menu);
        ui.update();
        list
    };
    let menu = ui.show_menu(b, tbid!("popupmenu"), top.clone(), &PopupAlignment::default(), -1).unwrap();
    let list = size_items(&mut ui, menu);
    assert_eq!(item_texts(&ui, list), ["Option 1", "<none>", "More", "Same again"]);
    let (first, more) = (item(&ui, list, 0), item(&ui, list, 2));
    assert!(ui.get::<SelectItemWidget>(first).is_some());
    assert_eq!(ui.node(first).unwrap().id(), tbid!("opt1"));
    assert!(ui.get::<Separator>(item(&ui, list, 1)).is_some());

    // The item with a sub source opens a sub menu to its right and looks selected.
    log.borrow_mut().clear();
    click(&mut ui, more);
    let root = ui.root();
    let submenu = *ui.children(root).last().unwrap();
    assert!(submenu != menu && ui.get::<MenuWindow>(submenu).is_some());
    assert_eq!(ui.node(submenu).unwrap().id(), tbid!("submenu"));
    assert_eq!(ui.popup_target(submenu), Some(more));
    assert!(ui.rect(submenu).x >= ui.rect(menu).right() - 4);
    assert!(ui.has_state(more, WidgetState::SELECTED));
    assert!(!ui.is_dying(menu), "the sub menu belongs to the menu");

    // Choosing in the sub menu: one click reaches the application, with the top
    // menu's id; both menus close.
    let sublist = size_items(&mut ui, submenu);
    let banana = item(&ui, sublist, 2);
    log.borrow_mut().clear();
    click(&mut ui, banana);
    let clicks: Vec<_> = log.borrow().iter().filter(|e| e.0 == EventKind::Click).cloned().collect();
    assert_eq!(clicks, [(EventKind::Click, tbid!("popupmenu"), tbid!("b"), Some(2))]);
    settle(&mut ui);
    assert!(!ui.exists(menu) && !ui.exists(submenu));

    // A press elsewhere closes an open menu and its sub menu.
    let menu = ui.show_menu(b, tbid!("popupmenu"), top.clone(), &PopupAlignment::default(), -1).unwrap();
    let list = size_items(&mut ui, menu);
    let more = item(&ui, list, 2);
    click(&mut ui, more);
    let submenu = *ui.children(root).last().unwrap();
    ui.pointer_down(Point::new(700, 500), 1, Modifiers::NONE);
    ui.pointer_up(Point::new(700, 500), Modifiers::NONE);
    settle(&mut ui);
    assert!(!ui.exists(menu) && !ui.exists(submenu));
    drop(top_ref);
    assert_eq!(Rc::strong_count(&top), 1, "no menu holds the source any more");
}

#[test]
fn message_window_answers_through_its_target() {
    let mut ui = ui();
    let (rec, log) = recorder(&mut ui);
    let b = ui.add(rec, Button::new("ask"));
    ui.set_rect(b, Rect::new(10, 10, 80, 20));
    let root = ui.root();

    let settings = MessageSettings { dimmer: true, ..MessageSettings::new(MessageButtons::YesNo, TbId::NONE) };
    let win =
        ui.show_message(b, tbid!("confirm"), "Sure?", "Really do it?\n\nIt cannot be undone.", &settings).unwrap();
    ui.update();
    assert_eq!(ui.text(win), Some("Sure?"));
    assert_eq!(ui.parent(win), Some(root));
    // The dimmer is right below the window.
    let kids = ui.children(root).to_vec();
    let dimmer = kids[kids.len() - 2];
    assert!(ui.get::<Dimmer>(dimmer).is_some());
    assert_eq!(*kids.last().unwrap(), win);
    // Centered, active, with the first button focused.
    let wr = ui.rect(win);
    assert!((wr.x + wr.w / 2 - 400).abs() <= 1 && (wr.y + wr.h / 2 - 300).abs() <= 1, "{wr:?}");
    assert!(ui.node(win).unwrap().is_active_window());
    let yes = ui.find(win, tbid!("TBMessageWindow.yes")).unwrap();
    let no = ui.find(win, tbid!("TBMessageWindow.no")).unwrap();
    assert_eq!((ui.text(yes), ui.text(no)), (Some("Yes"), Some("No")));
    assert_eq!(ui.focused(), Some(yes));
    assert!(ui.find(win, tbid!("TBMessageWindow.ok")).is_none());

    // The answer is a click with the window's id and the button's id as ref_id.
    log.borrow_mut().clear();
    ui.set_rect(no, Rect::new(0, 0, 40, 20));
    let mut ev = Event::new(EventKind::Click, no);
    ev.touch = true;
    ui.invoke_event(ev);
    assert_eq!(*log.borrow(), [(EventKind::Click, tbid!("confirm"), tbid!("TBMessageWindow.no"), None)]);
    assert!(ui.is_dying(win) && ui.is_dying(dimmer));
    settle(&mut ui);
    assert!(!ui.exists(win) && !ui.exists(dimmer));

    // Ok / Cancel, closed with Esc: answered as the window's close button.
    let win = ui
        .show_message(b, tbid!("info"), "Info", "Text", &MessageSettings::new(MessageButtons::OkCancel, TbId::NONE))
        .unwrap();
    assert!(ui.find(win, tbid!("TBMessageWindow.cancel")).is_some());
    log.borrow_mut().clear();
    ui.key(None, SpecialKey::Esc, Modifiers::NONE, true);
    assert_eq!(*log.borrow(), [(EventKind::Click, tbid!("info"), tbid!("TBWindow.close"), None)]);
    settle(&mut ui);
    assert!(!ui.exists(win));

    // The window goes away with its target.
    let win = ui.show_message(b, tbid!("info"), "Info", "Text", &MessageSettings::default()).unwrap();
    assert!(ui.find(win, tbid!("TBMessageWindow.ok")).is_some());
    ui.remove(b);
    settle(&mut ui);
    assert!(!ui.exists(win));
    assert!(ui.show_message(b, tbid!("info"), "Info", "Text", &MessageSettings::default()).is_none());
}

#[test]
fn inline_select_steps_between_limits() {
    let mut ui = ui();
    let (rec, log) = recorder(&mut ui);
    let s = ui.inflate(rec, "TBInlineSelect: id: num, min: 5, max: 8, value: 6\n").unwrap()[0];
    ui.set_rect(s, Rect::new(10, 10, 100, 24));
    ui.update();
    assert_eq!(ui.get::<InlineSelect>(s).unwrap().limits(), (5, 8));
    assert_eq!(ui.value(s), Value::Int(6));
    let (dec, inc) = (ui.find(s, tbid!("dec")).unwrap(), ui.find(s, tbid!("inc")).unwrap());
    let shown = |ui: &Ui| {
        let layout = ui.children(s)[0];
        ui.text(ui.children(layout)[1]).unwrap().to_string()
    };
    assert_eq!(shown(&ui), "6");
    log.borrow_mut().clear();
    ui.send(inc, EventKind::Click);
    ui.send(inc, EventKind::Click);
    ui.send(inc, EventKind::Click);
    assert_eq!((ui.value(s), shown(&ui)), (Value::Int(8), "8".to_string()), "clamped to the maximum");
    // (Its edit field reports its own changes too.)
    let changes = log.borrow().iter().filter(|e| e.0 == EventKind::Changed && e.1 == tbid!("num")).count();
    assert_eq!(changes, 2);
    ui.send(dec, EventKind::Click);
    assert_eq!(ui.value(s), Value::Int(7));
    // Keys that bubble to it step it too.
    let mut ev = Event::new(EventKind::KeyDown, dec);
    ev.special = SpecialKey::Down;
    assert!(ui.invoke_event(ev));
    assert_eq!(ui.value(s), Value::Int(6));
    ui.set_value(s, 100);
    assert_eq!(ui.value(s), Value::Int(8));
    ui.with_widget::<InlineSelect, _>(s, |w, cx| w.set_limits(cx, 0, 3));
    assert_eq!((ui.value(s), shown(&ui)), (Value::Int(3), "3".to_string()));
    // The buttons do not take the focus.
    assert!(!ui.node(inc).unwrap().is_focusable());
}

/// A source with its own item type and widgets (like TB's demo AdvancedItemSource).
struct Tasks {
    items: Vec<(String, bool)>,
    changes: SourceChanges,
}

impl SelectItemSource for Tasks {
    fn num_items(&self) -> usize {
        self.items.len()
    }
    fn item_string(&self, index: usize) -> &str {
        &self.items[index].0
    }
    fn filter(&self, index: usize, filter: &str) -> bool {
        let done = if self.items[index].1 { "done" } else { "open" };
        self.item_string(index).contains(filter) || done == filter
    }
    fn create_item_widget(&self, ui: &mut Ui, index: usize, _viewer: WidgetId) -> Option<WidgetId> {
        let row = ui.create(Layout::default());
        let check = ui.add(row, CheckBox::default());
        ui.set_value(check, self.items[index].1 as i32);
        ui.set_id(check, tbid!("check"));
        ui.add(row, TextField::new(&self.items[index].0));
        Some(row)
    }
    fn changes(&self) -> &SourceChanges {
        &self.changes
    }
}

#[test]
fn custom_source_and_item_widgets() {
    let mut ui = ui();
    let src = Rc::new(RefCell::new(Tasks {
        items: vec![("write".into(), true), ("test".into(), false), ("ship".into(), false)],
        changes: SourceChanges::default(),
    }));
    let (rec, _log) = recorder(&mut ui);
    let mut lists = Vec::new();
    for i in 0..2 {
        let l = ui.add(rec, SelectList::default());
        let s: SourceRef = src.clone();
        ui.with_widget::<SelectList, _>(l, |w, cx| w.set_source(cx, Some(s)));
        ui.set_rect(l, Rect::new(10 + i * 160, 10, 150, 100));
        lists.push(l);
    }
    ui.update();
    let check_of = |ui: &Ui, l: WidgetId, index: i32| {
        let row = item(ui, l, index);
        ui.find(row, tbid!("check")).unwrap()
    };
    assert_eq!(ui.value(check_of(&ui, lists[0], 0)), Value::Int(1));
    assert_eq!(ui.value(check_of(&ui, lists[1], 1)), Value::Int(0));

    // The first list's widget changed the item: that list keeps its widgets, the
    // other one replaces the row.
    let (row_a, row_b) = (item(&ui, lists[0], 1), item(&ui, lists[1], 1));
    {
        let mut s = src.borrow_mut();
        s.items[1].1 = true;
        s.changes.item_changed(1, Some(lists[0]));
    }
    ui.update();
    assert_eq!(item(&ui, lists[0], 1), row_a);
    assert!(item(&ui, lists[1], 1) != row_b);
    assert_eq!(ui.value(check_of(&ui, lists[1], 1)), Value::Int(1));

    // The source's own filter.
    ui.with_widget::<SelectList, _>(lists[0], |w, cx| w.set_filter(cx, "open"));
    ui.update();
    let layout = ui.get::<SelectList>(lists[0]).unwrap().item_layout().unwrap();
    assert_eq!(ui.children(layout).len(), 2, "the header and the one open task");
    assert_eq!(ui.text(ui.children(layout)[0]), Some("Showing 1 of 3"));
}

/// A listener that hears a menu's choice can ask which widget the menu belongs to,
/// although the menu is in the middle of sending the event.
#[test]
fn a_menus_target_is_known_while_it_reports_a_choice() {
    struct Asks(Rc<RefCell<Vec<Option<WidgetId>>>>);
    impl UiListener for Asks {
        fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool {
            if ev.kind == EventKind::Click && ui.node(target).is_some_and(|n| n.id() == tbid!("popupmenu")) {
                self.0.borrow_mut().push(ui.popup_target(target));
            }
            false
        }
    }
    let mut ui = ui();
    let root = ui.root();
    let b = ui.add(root, Button::new("menu"));
    ui.set_rect(b, Rect::new(10, 10, 80, 20));
    let asked = Rc::new(RefCell::new(Vec::new()));
    ui.add_listener(Box::new(Asks(asked.clone())));

    let source: SourceRef = names();
    let menu = ui.show_menu(b, tbid!("popupmenu"), source, &PopupAlignment::default(), -1).unwrap();
    assert_eq!(ui.popup_target(menu), Some(b));
    assert_eq!(ui.popup_target(b), None, "not a popup");
    let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
    let layout = ui.get::<SelectList>(list).unwrap().item_layout().unwrap();
    for c in ui.children(layout).to_vec() {
        ui.set_layout_params(c, Some(LayoutParams::fixed(100, 20)));
    }
    ui.resize_to_preferred(menu);
    ui.update();
    let first = item(&ui, list, 0);
    click(&mut ui, first);
    assert_eq!(*asked.borrow(), [Some(b)]);
}
