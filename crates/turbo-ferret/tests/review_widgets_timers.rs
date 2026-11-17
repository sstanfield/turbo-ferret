//! Widget regressions found in review (2026-11): timers and drags.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::widgets::*;
use turbo_ferret::*;

fn plain() -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(800, 600));
    ui.set_time(1000);
    ui
}

fn skinned() -> Ui {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    ui.add_font("Segoe", "default_font/segoe_white_with_shadow.tb.txt");
    assert!(ui.set_default_font(FontDesc::new(tbid!("Segoe"), 14)));
    ui.set_viewport(Size::new(640, 480));
    ui.set_time(1000);
    ui
}

fn center(ui: &Ui, id: WidgetId) -> Point {
    let r = ui.rect(id);
    ui.to_root(id, Point::new(r.w / 2, r.h / 2))
}

/// Stops a spinner from its own message handler.
struct Stopper(WidgetId);
impl Widget for Stopper {
    fn type_name(&self) -> &'static str {
        "Stopper"
    }
    fn on_message(&mut self, cx: &mut Ctx, _msg: &Message) {
        cx.set_value(self.0, 0);
    }
}

/// The work is done (a message says so) and the spinner is stopped; the spinner's
/// own frame message is due in the same update.
#[test]
fn m01_spinner_stopped_from_a_message_handler() {
    let mut ui = plain();
    let root = ui.root();
    let sp = ui.add(root, ProgressSpinner::default());
    ui.set_value(sp, 1);
    assert_eq!(ui.next_wake_time(), Some(1033));
    let st = ui.add(root, Stopper(sp));
    ui.post_message_delayed(st, tbid!("done"), Value::Null, 20);
    ui.set_time(1040);
    ui.update();
    assert!(!ui.get::<ProgressSpinner>(sp).unwrap().is_running());
    assert_eq!(ui.next_wake_time(), None, "a stopped spinner must not keep a timer running");
}

/// TB: TBScrollBar::SetLimits keeps the grab point when the limits change during a
/// drag of the handle, so the position does not jump.
#[test]
fn m02_scroll_bar_drag_while_content_grows() {
    let mut ui = skinned();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Y));
    let content = ui.add(ui.content_root(sc), Plain);
    ui.set_layout_params(content, Some(LayoutParams::fixed(100, 1000)));
    ui.set_rect(sc, Rect::new(0, 0, 200, 200));
    ui.update();
    let sby = ui.get::<ScrollContainer>(sc).unwrap().scrollbar_y().unwrap();
    let handle = ui.get::<ScrollBar>(sby).unwrap().handle().unwrap();
    let grab = center(&ui, handle);
    ui.pointer_down(grab, 1, Modifiers::NONE);
    for i in 1..=40 {
        ui.pointer_move(grab + Point::new(0, i), Modifiers::NONE);
    }
    let pos = ui.scroll_info(sc).unwrap().pos.y;
    assert!((pos - 200).abs() <= 6, "{pos}");
    // The content doubles while the handle is held (items added to a list, say).
    ui.set_layout_params(content, Some(LayoutParams::fixed(100, 2000)));
    ui.update();
    assert_eq!(ui.scroll_info(sc).unwrap().pos.y, pos, "growing content keeps the position");
    // The pointer moves one more pixel.
    ui.pointer_move(grab + Point::new(0, 41), Modifiers::NONE);
    let pos2 = ui.scroll_info(sc).unwrap().pos.y;
    ui.pointer_up(grab + Point::new(0, 41), Modifiers::NONE);
    assert!((pos2 - pos).abs() <= 30, "one pixel of drag moved the content from {pos} to {pos2}");
}

/// An auto repeat button whose capture ends while its repeat message is on its way.
struct Canceller;
impl Widget for Canceller {
    fn type_name(&self) -> &'static str {
        "Canceller"
    }
    fn on_message(&mut self, cx: &mut Ctx, _msg: &Message) {
        cx.pointer_cancel();
    }
}

#[test]
fn m03_auto_repeat_after_capture_is_lost_in_the_same_update() {
    let mut ui = plain();
    let root = ui.root();
    let mut b = Button::new("+");
    b.auto_repeat = true;
    let b = ui.add(root, b);
    ui.set_rect(b, Rect::new(10, 10, 40, 20));
    let c = ui.add(root, Canceller);
    ui.pointer_down(Point::new(20, 20), 1, Modifiers::NONE);
    assert_eq!(ui.next_wake_time(), Some(1500));
    ui.post_message_delayed(c, tbid!("cancel"), Value::Null, 490);
    ui.set_time(1510);
    ui.update();
    assert_eq!(ui.captured(), None);
    for _ in 0..50 {
        ui.set_time(ui.now() + 100);
        ui.update();
    }
    assert_eq!(ui.next_wake_time(), None, "the repeat timer keeps running without a press");
}

/// Sections: `value` and the `Changed` event of the header, with a connection.
#[test]
fn m04_section_connection() {
    let mut ui = plain();
    let root = ui.root();
    let src = "TBLayout: axis: y\n\tTBSection: id: sec, text: \"S\", connection: open\n\t\tTBWidget: id: body\n\t\t\tlp: width: 50, height: 50\n\tTBCheckBox: id: cb, connection: open\n";
    let col = ui.inflate(root, src).unwrap()[0];
    ui.resize_to_preferred(col);
    ui.update();
    let sec = ui.find(root, tbid!("sec")).unwrap();
    let cb = ui.find(root, tbid!("cb")).unwrap();
    // The check box opens the section.
    ui.set_value(cb, 1);
    assert_eq!(ui.value(sec), Value::Int(1));
    let header = ui.get::<Section>(sec).unwrap().header().unwrap();
    assert_eq!(ui.value(header), Value::Int(1));
    // Closing the section by its header: TB did not sync this either (the section
    // sends no Changed); just record what happens.
    ui.set_rect(header, Rect::new(0, 0, 50, 20));
    let p = ui.to_root(header, Point::new(5, 5));
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
}

/// Window activation: closing the top window reactivates the one below and gives
/// its focus back; popups do not steal activation.
#[test]
fn m05_window_activation_and_focus_restore() {
    let mut ui = skinned();
    let root = ui.root();
    let mk = |ui: &mut Ui, x: i32| {
        let w = ui.add(root, Window::new("W"));
        ui.with_widget::<Window, _>(w, |win, _| win.animate = false);
        ui.set_rect(w, Rect::new(x, 20, 200, 150));
        ui.inflate(w, "TBLayout: axis: y\n\tTBButton: id: b1, text: one\n\tTBButton: id: b2, text: two\n\tTBSelectDropdown: id: dd\n\t\titems\n\t\t\titem: text: A\n\t\t\titem: text: B\n").unwrap();
        w
    };
    let w1 = mk(&mut ui, 20);
    let w2 = mk(&mut ui, 300);
    ui.update();
    assert!(ui.is_active_window(w2) && !ui.is_active_window(w1));
    // Focus the second button of w1 by clicking it.
    let b2 = ui.find(w1, tbid!("b2")).unwrap();
    let p = center(&ui, b2);
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert!(ui.is_active_window(w1) && !ui.is_active_window(w2));
    assert_eq!(ui.focused(), Some(b2));
    assert_eq!(*ui.children(root).last().unwrap(), w1);
    // Click in w2, then close w2: w1 is active again with b2 focused.
    let b1 = ui.find(w2, tbid!("b1")).unwrap();
    let p = center(&ui, b1);
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(ui.focused(), Some(b1));
    ui.close_window(w2);
    ui.update();
    assert!(!ui.exists(w2));
    assert!(ui.is_active_window(w1));
    assert_eq!(ui.focused(), Some(b2));
    // Open the dropdown of w1: w1 stays active, and choosing keeps focus sane.
    let dd = ui.find(w1, tbid!("dd")).unwrap();
    let p = center(&ui, dd);
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    let menu = ui.with_widget::<SelectDropdown, _>(dd, |w, cx| w.menu_if_open(cx)).flatten().expect("menu");
    for _ in 0..30 {
        ui.set_time(ui.now() + 16);
        ui.update();
    }
    assert!(ui.is_active_window(w1));
    assert_eq!(ui.focused(), Some(dd));
    let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
    let it = ui.get::<SelectList>(list).unwrap().item_widget(&ui, 1).unwrap();
    let p = center(&ui, it);
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    for _ in 0..30 {
        ui.set_time(ui.now() + 16);
        ui.update();
    }
    assert!(!ui.exists(menu));
    assert_eq!(ui.text(dd), Some("B"));
    assert!(ui.is_active_window(w1));
    assert_eq!(ui.focused(), Some(dd));
}

/// The dropdown's menu closes with the window it is in, and a dropdown removed with
/// its window leaves no menu behind.
#[test]
fn m06_dropdown_menu_goes_with_its_window() {
    let mut ui = skinned();
    let root = ui.root();
    let w = ui.add(root, Window::new("W"));
    ui.set_rect(w, Rect::new(20, 20, 200, 150));
    ui.inflate(w, "TBSelectDropdown: id: dd\n\titems\n\t\titem: text: A\n\t\titem: text: B\n").unwrap();
    for _ in 0..30 {
        ui.set_time(ui.now() + 16);
        ui.update();
    }
    let dd = ui.find(w, tbid!("dd")).unwrap();
    let p = center(&ui, dd);
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    let menu = ui.with_widget::<SelectDropdown, _>(dd, |w, cx| w.menu_if_open(cx)).flatten().expect("menu");
    ui.remove(w);
    for _ in 0..60 {
        ui.set_time(ui.now() + 16);
        ui.update();
    }
    assert!(!ui.exists(menu));
    assert_eq!(ui.children(root).len(), 0, "{:?}", ui.children(root));
    let _ = Rc::new(RefCell::new(0));
}
