//! Widget flows found sound in review (2026-11), with the default skin and real geometry.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::widgets::*;
use turbo_ferret::*;

fn skinned() -> Ui {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    ui.add_font("Segoe", "default_font/segoe_white_with_shadow.tb.txt");
    assert!(ui.set_default_font(FontDesc::new(tbid!("Segoe"), 14)));
    ui.set_viewport(Size::new(640, 480));
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

fn settle(ui: &mut Ui) {
    for _ in 0..60 {
        ui.set_time(ui.now() + 16);
        ui.update();
    }
}

fn center(ui: &Ui, id: WidgetId) -> Point {
    let r = ui.rect(id);
    ui.to_root(id, Point::new(r.w / 2, r.h / 2))
}

#[test]
fn s01_window_close_button_and_resizer() {
    let mut ui = skinned();
    let root = ui.root();
    let w = ui.add(root, Window::new("Title"));
    ui.set_rect(w, Rect::new(50, 50, 200, 150));
    let c = ui.add(w, Button::new("content"));
    ui.set_gravity(c, Gravity::ALL);
    ui.place_child(w, c);
    settle(&mut ui);
    let win = ui.get::<Window>(w).unwrap();
    let close = win.close_button().unwrap();
    let kids = ui.children(w).to_vec();
    // Resizer: last child, bottom right.
    let resizer = *kids.iter().find(|&&k| ui.get::<Resizer>(k).is_some()).unwrap();
    assert_eq!(*kids.last().unwrap(), resizer, "resizer on top");
    let rr = ui.rect(resizer);
    assert_eq!((rr.x + rr.w, rr.y + rr.h), (200, 150));
    let grab = ui.to_root(resizer, Point::new(rr.w - 3, rr.h - 3));
    assert_eq!(ui.widget_at(root, grab), Some(resizer));
    ui.pointer_down(grab, 1, Modifiers::NONE);
    ui.pointer_move(grab + Point::new(40, 30), Modifiers::NONE);
    ui.pointer_move(grab + Point::new(50, 40), Modifiers::NONE);
    ui.pointer_up(grab + Point::new(50, 40), Modifiers::NONE);
    assert_eq!(ui.rect(w), Rect::new(50, 50, 250, 190));
    let cr = ui.rect(c);
    let pr = ui.padding_rect(w);
    assert_eq!(cr, pr, "content follows");
    // Shrink below the minimum of 50.
    let rr = ui.rect(resizer);
    let grab = ui.to_root(resizer, Point::new(rr.w - 3, rr.h - 3));
    ui.pointer_down(grab, 1, Modifiers::NONE);
    ui.pointer_move(grab + Point::new(-400, -400), Modifiers::NONE);
    ui.pointer_up(grab + Point::new(-400, -400), Modifiers::NONE);
    assert_eq!(ui.rect(w).size(), Size::new(50, 50));
    ui.set_rect(w, Rect::new(50, 50, 200, 150));
    ui.update();
    // Close by the real button.
    let p = center(&ui, close);
    assert_eq!(ui.widget_at(root, p), Some(close), "close rect {:?}", ui.rect(close));
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert!(ui.is_dying(w));
    settle(&mut ui);
    assert!(!ui.exists(w));
}

#[test]
fn s02_scroll_container_bar_drag_and_page() {
    let mut ui = skinned();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Y));
    let content = ui.add(ui.content_root(sc), Plain);
    ui.set_layout_params(content, Some(LayoutParams::fixed(100, 1000)));
    ui.set_rect(sc, Rect::new(0, 0, 200, 200));
    ui.update();
    let w = ui.get::<ScrollContainer>(sc).unwrap();
    let sby = w.scrollbar_y().unwrap();
    let handle = ui.get::<ScrollBar>(sby).unwrap().handle().unwrap();
    assert_eq!(ui.scroll_info(sc).unwrap().max.y, 800);
    // Drag the handle 40 px down: 200 px track, handle 40 px => 160 px for 800 => 5 per px.
    let grab = center(&ui, handle);
    ui.pointer_down(grab, 1, Modifiers::NONE);
    assert_eq!(ui.captured(), Some(handle));
    for i in 1..=40 {
        ui.pointer_move(grab + Point::new(0, i), Modifiers::NONE);
    }
    ui.pointer_up(grab + Point::new(0, 40), Modifiers::NONE);
    let pos = ui.scroll_info(sc).unwrap().pos.y;
    assert!((pos - 200).abs() <= 6, "pos after drag {pos}");
    assert_eq!(ui.rect(handle).y, 40);
    // Drag far past the end and back: the handle should follow the pointer again.
    let grab = center(&ui, handle);
    ui.pointer_down(grab, 1, Modifiers::NONE);
    ui.pointer_move(grab + Point::new(0, 500), Modifiers::NONE);
    assert_eq!(ui.scroll_info(sc).unwrap().pos.y, 800);
    ui.pointer_move(grab + Point::new(0, 0), Modifiers::NONE);
    let pos2 = ui.scroll_info(sc).unwrap().pos.y;
    ui.pointer_up(grab, Modifiers::NONE);
    assert!((pos2 - pos).abs() <= 6, "back at the grab point: {pos2} vs {pos}");
    // Click in the track below the handle: one page down.
    let before = ui.scroll_info(sc).unwrap().pos.y;
    let br = ui.rect(sby);
    let p = ui.to_root(sby, Point::new(br.w / 2, br.h - 2));
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    let after = ui.scroll_info(sc).unwrap().pos.y;
    assert_eq!(after - before, 200, "page down {before} -> {after}");
    // The content really moved.
    assert_eq!(ui.to_root(content, Point::ZERO).y, -after);
}

#[test]
fn s03_dropdown_real_click_and_long_list() {
    let mut ui = skinned();
    let root = ui.root();
    let d = ui.add(root, SelectDropdown::default());
    ui.set_rect(d, Rect::new(20, 20, 120, 24));
    let src = StringItemSource::shared();
    for i in 0..60 {
        src.borrow_mut().add(StringItem::with_id(&format!("Item {i}"), TbId(1000 + i)));
    }
    ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.set_source(cx, Some(src.clone())));
    ui.set_value(d, 45);
    ui.update();
    click(&mut ui, d);
    let menu = ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.menu_if_open(cx)).flatten().expect("menu open");
    settle(&mut ui);
    let mr = ui.rect(menu);
    assert!(mr.y >= 44 && mr.bottom() <= 480, "{mr:?}");
    let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
    let sel = ui.get::<SelectList>(list).unwrap().item_widget(&ui, 45).unwrap();
    // The selected item is scrolled into view.
    let top = ui.to_root(sel, Point::ZERO).y;
    assert!(top >= mr.y && top + ui.rect(sel).h <= mr.bottom(), "selected item at {top}, menu {mr:?}");
    assert!(ui.has_state(sel, WidgetState::SELECTED));
    // Click the item above it.
    let it = ui.get::<SelectList>(list).unwrap().item_widget(&ui, 44).unwrap();
    let p = center(&ui, it);
    assert_eq!(ui.widget_at(root, p), Some(it));
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!((ui.value(d), ui.text(d)), (Value::Int(44), Some("Item 44")));
    settle(&mut ui);
    assert!(!ui.exists(menu));
    // Wheel inside the open menu scrolls the list, and does not close it.
    click(&mut ui, d);
    let menu = ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.menu_if_open(cx)).flatten().expect("menu open");
    settle(&mut ui);
    let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
    let sc = ui.get::<SelectList>(list).unwrap().scroll_container().unwrap();
    let before = ui.scroll_info(sc).unwrap().pos.y;
    let p = center(&ui, menu);
    ui.wheel(p, Point::new(0, -1), Modifiers::NONE);
    settle(&mut ui);
    assert!(ui.exists(menu) && !ui.is_dying(menu));
    assert!(ui.scroll_info(sc).unwrap().pos.y < before, "wheel scrolled {before} -> {:?}", ui.scroll_info(sc));
    // Dragging the menu's scroll bar does not close it or choose anything.
    let sby = ui.get::<ScrollContainer>(sc).unwrap().scrollbar_y().unwrap();
    let handle = ui.get::<ScrollBar>(sby).unwrap().handle().unwrap();
    let g = center(&ui, handle);
    ui.pointer_down(g, 1, Modifiers::NONE);
    ui.pointer_move(g + Point::new(0, -20), Modifiers::NONE);
    ui.pointer_up(g + Point::new(0, -20), Modifiers::NONE);
    settle(&mut ui);
    assert!(ui.exists(menu) && !ui.is_dying(menu), "menu survives a scroll bar drag");
    assert_eq!(ui.value(d), Value::Int(44));
}

#[test]
fn s04_message_window_real_clicks_and_keys() {
    let mut ui = skinned();
    let root = ui.root();
    let log: Rc<RefCell<Vec<(TbId, TbId)>>> = Rc::new(RefCell::new(Vec::new()));
    struct T(Rc<RefCell<Vec<(TbId, TbId)>>>);
    impl Widget for T {
        fn type_name(&self) -> &'static str {
            "T"
        }
        fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
            if ev.kind == EventKind::Click {
                let tid = cx.node(ev.target).map_or(TbId::NONE, |n| n.id());
                self.0.borrow_mut().push((tid, ev.ref_id));
            }
            false
        }
    }
    let t = ui.add(root, T(log.clone()));
    ui.set_rect(t, Rect::new(0, 0, 10, 10));
    let win = ui
        .show_message(
            t,
            tbid!("m"),
            "Title",
            "Some message text",
            &MessageSettings::new(MessageButtons::OkCancel, TbId::NONE),
        )
        .unwrap();
    settle(&mut ui);
    let ok = ui.find(win, tbid!("TBMessageWindow.ok")).unwrap();
    let cancel = ui.find(win, tbid!("TBMessageWindow.cancel")).unwrap();
    assert_eq!(ui.focused(), Some(ok));
    // Tab moves to cancel, Enter answers.
    ui.key(None, SpecialKey::Tab, Modifiers::NONE, true);
    ui.key(None, SpecialKey::Tab, Modifiers::NONE, false);
    assert_eq!(ui.focused(), Some(cancel));
    ui.key(None, SpecialKey::Enter, Modifiers::NONE, true);
    ui.key(None, SpecialKey::Enter, Modifiers::NONE, false);
    assert_eq!(*log.borrow(), [(tbid!("m"), tbid!("TBMessageWindow.cancel"))]);
    settle(&mut ui);
    assert!(!ui.exists(win));
    // Real click on Ok.
    log.borrow_mut().clear();
    let win = ui.show_message(t, tbid!("m2"), "Title", "Text", &MessageSettings::default()).unwrap();
    settle(&mut ui);
    let ok = ui.find(win, tbid!("TBMessageWindow.ok")).unwrap();
    let p = center(&ui, ok);
    assert_eq!(ui.widget_at(root, p), Some(ok));
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(*log.borrow(), [(tbid!("m2"), tbid!("TBMessageWindow.ok"))]);
    settle(&mut ui);
    assert!(!ui.exists(win));
    // The title bar's close button.
    log.borrow_mut().clear();
    let win = ui.show_message(t, tbid!("m3"), "Title", "Text", &MessageSettings::default()).unwrap();
    settle(&mut ui);
    let close = ui.find(win, tbid!("TBWindow.close")).unwrap();
    let p = center(&ui, close);
    assert_eq!(ui.widget_at(root, p), Some(close));
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(*log.borrow(), [(tbid!("m3"), tbid!("TBWindow.close"))]);
    settle(&mut ui);
    assert!(!ui.exists(win));
}

#[test]
fn s05_click_label_and_inline_select() {
    let mut ui = skinned();
    let root = ui.root();
    let src = "TBLayout: axis: y\n\tTBClickLabel: text: \"Check me\"\n\t\tTBCheckBox: id: cb\n\tTBClickLabel: text: \"R1\"\n\t\tTBRadioButton: id: r1, group-id: g\n\tTBClickLabel: text: \"R2\"\n\t\tTBRadioButton: id: r2, group-id: g\n\tTBInlineSelect: id: num, min: 0, max: 3\n\tTBSlider: id: sl, min: 0, max: 10\n\t\tlp: width: 110\n";
    let col = ui.inflate(root, src).unwrap()[0];
    ui.resize_to_preferred(col);
    settle(&mut ui);
    let cb = ui.find(root, tbid!("cb")).unwrap();
    let label = ui.parent(ui.parent(cb).unwrap()).unwrap();
    assert!(ui.get::<ClickLabel>(label).is_some());
    // Click on the label text (right part of the label).
    let lr = ui.rect(label);
    let p = ui.to_root(label, Point::new(lr.w - 5, lr.h / 2));
    assert_ne!(ui.widget_at(root, p), Some(cb));
    ui.pointer_down(p, 1, Modifiers::NONE);
    assert!(ui.has_state(cb, WidgetState::PRESSED), "pressed look while the label is held");
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(ui.value(cb), Value::Int(1));
    assert!(!ui.has_state(cb, WidgetState::PRESSED));
    assert_eq!(ui.focused(), Some(cb));
    // Radios through labels.
    let (r1, r2) = (ui.find(root, tbid!("r1")).unwrap(), ui.find(root, tbid!("r2")).unwrap());
    for r in [r1, r2, r1] {
        let label = ui.parent(ui.parent(r).unwrap()).unwrap();
        let lr = ui.rect(label);
        let p = ui.to_root(label, Point::new(lr.w - 5, lr.h / 2));
        ui.pointer_down(p, 1, Modifiers::NONE);
        ui.pointer_up(p, Modifiers::NONE);
        let other = if r == r1 { r2 } else { r1 };
        assert_eq!((ui.value(r), ui.value(other)), (Value::Int(1), Value::Int(0)));
    }
    // Inline select: real clicks on the arrows.
    let num = ui.find(root, tbid!("num")).unwrap();
    let inc = ui.find(num, tbid!("inc")).unwrap();
    let dec = ui.find(num, tbid!("dec")).unwrap();
    for _ in 0..5 {
        let p = center(&ui, inc);
        ui.pointer_down(p, 1, Modifiers::NONE);
        ui.pointer_up(p, Modifiers::NONE);
    }
    assert_eq!(ui.value(num), Value::Int(3));
    let p = center(&ui, dec);
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(ui.value(num), Value::Int(2));
    // Slider: drag the handle to the far right.
    let sl = ui.find(root, tbid!("sl")).unwrap();
    let handle = ui.get::<Slider>(sl).unwrap().handle().unwrap();
    let g = center(&ui, handle);
    ui.pointer_down(g, 1, Modifiers::NONE);
    for i in 1..=30 {
        ui.pointer_move(g + Point::new(i * 10, 0), Modifiers::NONE);
    }
    assert_eq!(ui.value(sl).as_float(), 10.0);
    for i in (0..=30).rev() {
        ui.pointer_move(g + Point::new(i * 10, 0), Modifiers::NONE);
    }
    ui.pointer_up(g, Modifiers::NONE);
    assert!(ui.value(sl).as_float() < 0.5, "back at the start: {}", ui.value(sl).as_float());
}
