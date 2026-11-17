//! Widget regressions found in review (2026-11), with the default skin.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::widgets::*;
use turbo_ferret::*;

fn skinned() -> Ui {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    ui.add_font("DejaVuShadow", "default_font/dejavu_white_with_shadow.tb.txt");
    assert!(ui.set_default_font(FontDesc::new(tbid!("DejaVuShadow"), 14)));
    ui.set_viewport(Size::new(640, 480));
    ui.set_time(1000);
    ui.lang_mut().set("TBList.header", "Showing %d of %d");
    ui.lang_mut().set("TBMessageWindow.ok", "Ok");
    ui
}

fn click(ui: &mut Ui, id: WidgetId) {
    let r = ui.rect(id);
    let c = ui.to_root(id, Point::new(r.w / 2, r.h / 2));
    ui.pointer_down(c, 1, Modifiers::NONE);
    ui.pointer_up(c, Modifiers::NONE);
}

fn settle(ui: &mut Ui) {
    for _ in 0..60 {
        ui.set_time(ui.now() + 16);
        ui.update();
    }
}

type Log = Rc<RefCell<Vec<(EventKind, TbId, TbId, Option<i32>)>>>;
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

/// TB: the button's layout hides the empty text field as soon as another child is
/// added (ButtonLayout::OnChildAdded), so an image button is measured without it.
#[test]
fn s06_image_button_size_before_first_update() {
    let mut ui = skinned();
    let root = ui.root();
    let w = ui.add(root, Window::new("T"));
    ui.inflate(w, "TBButton: id: b\n\tTBSkinImage: skin: arrow.left\n").unwrap();
    let before = ui.fit_content_rect(w, ResizeFit::Preferred);
    let b = ui.find(w, tbid!("b")).unwrap();
    let rect_before = ui.rect(b);
    settle(&mut ui);
    let after = ui.fit_content_rect(w, ResizeFit::Preferred);
    let pref_after = ui.preferred_size_free(b).pref;
    assert_eq!(
        (before.size(), rect_before.size()),
        (after.size(), pref_after),
        "window fit / button size before the first update vs after"
    );
}

/// A dropdown whose source has a sub menu: choosing in the sub menu.
#[test]
fn s07_dropdown_with_sub_menu() {
    let mut ui = skinned();
    let root = ui.root();
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let rec = ui.add(root, Recorder(log.clone()));
    ui.set_rect(rec, Rect::new(0, 0, 640, 480));
    let d = ui.add(rec, SelectDropdown::default());
    ui.set_id(d, tbid!("drop"));
    ui.set_rect(d, Rect::new(20, 20, 120, 24));
    let sub = StringItemSource::shared();
    for n in ["s0", "s1", "s2", "s3"] {
        sub.borrow_mut().add(StringItem::with_id(n, TbId::new(n)));
    }
    let sub_ref: SourceRef = sub.clone();
    let top = StringItemSource::shared();
    top.borrow_mut().add(StringItem::new("Alpha"));
    top.borrow_mut().add(StringItem::new("Beta"));
    top.borrow_mut().add(StringItem::new("Gamma"));
    top.borrow_mut().add(StringItem::with_sub_source("More", &sub_ref));
    ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.set_source(cx, Some(top.clone())));
    settle(&mut ui);
    assert_eq!((ui.value(d), ui.text(d)), (Value::Int(0), Some("Alpha")));
    click(&mut ui, d);
    let menu = ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.menu_if_open(cx)).flatten().expect("menu open");
    settle(&mut ui);
    let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
    let more = ui.get::<SelectList>(list).unwrap().item_widget(&ui, 3).unwrap();
    click(&mut ui, more);
    settle(&mut ui);
    let submenu = *ui.children(root).last().unwrap();
    assert!(submenu != menu && ui.get::<MenuWindow>(submenu).is_some());
    let sublist = ui.get::<MenuWindow>(submenu).unwrap().list().unwrap();
    let s2 = ui.get::<SelectList>(sublist).unwrap().item_widget(&ui, 2).unwrap();
    log.borrow_mut().clear();
    click(&mut ui, s2);
    settle(&mut ui);
    // TB: the dropdown takes the value of the top menu's list, which did not change.
    assert_eq!(
        (ui.value(d), ui.text(d)),
        (Value::Int(0), Some("Alpha")),
        "a sub menu choice must not select the top level item with the same index"
    );
}

/// An item with an image (no sub menu) in a menu is chosen like a plain one, also
/// while a sub menu is open.
#[test]
fn s08_menu_image_item_and_parent_click_with_open_sub_menu() {
    let mut ui = skinned();
    let root = ui.root();
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let rec = ui.add(root, Recorder(log.clone()));
    ui.set_rect(rec, Rect::new(0, 0, 640, 480));
    let b = ui.add(rec, Button::new("menu"));
    ui.set_rect(b, Rect::new(10, 10, 80, 20));
    let top = StringItemSource::shared();
    let top_ref: SourceRef = top.clone();
    top.borrow_mut().add(StringItem::with_id("Option 1", tbid!("opt1")).image(tbid!("arrow.left")));
    top.borrow_mut().add(StringItem::new("-"));
    top.borrow_mut().add(StringItem::with_id("Plain", tbid!("plain")));
    top.borrow_mut().add(StringItem::with_sub_source("Same again", &top_ref));
    let menu = ui.show_menu(b, tbid!("popupmenu"), top.clone(), &PopupAlignment::default(), -1).unwrap();
    settle(&mut ui);
    let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
    let again = ui.get::<SelectList>(list).unwrap().item_widget(&ui, 3).unwrap();
    click(&mut ui, again);
    settle(&mut ui);
    let sub1 = *ui.children(root).last().unwrap();
    assert!(sub1 != menu);
    let list1 = ui.get::<MenuWindow>(sub1).unwrap().list().unwrap();
    let again1 = ui.get::<SelectList>(list1).unwrap().item_widget(&ui, 3).unwrap();
    click(&mut ui, again1);
    settle(&mut ui);
    let sub2 = *ui.children(root).last().unwrap();
    assert!(sub2 != sub1 && ui.exists(menu) && ui.exists(sub1), "three levels open");
    // Click the image item of the first sub menu while the second is open.
    let opt = ui.get::<SelectList>(list1).unwrap().item_widget(&ui, 0).unwrap();
    log.borrow_mut().clear();
    click(&mut ui, opt);
    let clicks: Vec<_> = log.borrow().iter().filter(|e| e.0 == EventKind::Click).cloned().collect();
    assert_eq!(clicks, [(EventKind::Click, tbid!("popupmenu"), tbid!("opt1"), Some(0))]);
    settle(&mut ui);
    assert!(!ui.exists(menu) && !ui.exists(sub1) && !ui.exists(sub2), "all menus closed");
    drop(top_ref);
    assert_eq!(Rc::strong_count(&top), 1);
}

/// A tab container: keyboard click on a focused tab, and tabs inflated with `value`.
#[test]
fn s09_tabs_keyboard_and_value() {
    let mut ui = skinned();
    let root = ui.root();
    let src = "TBTabContainer: id: tc, value: 1\n\ttabs\n\t\tTBButton: text: \"One\"\n\t\tTBButton: text: \"Two\"\n\t\tTBButton: text: \"Three\"\n\tTBButton: id: p1, text: \"page 1\"\n\tTBButton: id: p2, text: \"page 2\"\n\tTBButton: id: p3, text: \"page 3\"\n";
    let tc = ui.inflate(root, src).unwrap()[0];
    ui.set_rect(tc, Rect::new(0, 0, 300, 200));
    settle(&mut ui);
    let w = ui.get::<TabContainer>(tc).unwrap();
    let tabs = ui.children(w.tab_layout().unwrap()).to_vec();
    let pages = ui.children(w.pages_root().unwrap()).to_vec();
    let shown = |ui: &Ui| pages.iter().map(|&p| ui.visibility(p) == Visibility::Visible).collect::<Vec<_>>();
    let pressed = |ui: &Ui| tabs.iter().map(|&t| ui.has_state(t, WidgetState::PRESSED)).collect::<Vec<_>>();
    assert_eq!(shown(&ui), [false, true, false]);
    assert_eq!(pressed(&ui), [false, true, false]);
    // Tab to the third tab and press Space.
    ui.set_focus(tabs[2]);
    ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, true);
    ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, false);
    assert_eq!(shown(&ui), [false, false, true]);
    assert_eq!(pressed(&ui), [false, false, true]);
    assert_eq!(ui.value(tc), Value::Int(2));
    // Space on the active tab keeps it pressed.
    ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, true);
    ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, false);
    assert_eq!(pressed(&ui), [false, false, true]);
    // Pointer: press and hold on tab one switches at once (pointer down).
    let p = ui.to_root(tabs[0], Point::new(5, 5));
    ui.pointer_down(p, 1, Modifiers::NONE);
    assert_eq!(shown(&ui), [true, false, false]);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(pressed(&ui), [true, false, false]);
    // Hidden pages get no input and no focus by Tab.
    ui.set_focus(tabs[0]);
    for _ in 0..8 {
        ui.key(None, SpecialKey::Tab, Modifiers::NONE, true);
        ui.key(None, SpecialKey::Tab, Modifiers::NONE, false);
        let f = ui.focused().unwrap();
        assert!(f != pages[1] && f != pages[2], "focus went to a hidden page");
    }
}
