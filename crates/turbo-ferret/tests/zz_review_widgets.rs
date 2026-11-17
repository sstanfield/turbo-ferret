//! Widget regressions found in review (2026-11): corner cases, each with the TB behaviour it checks.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::widgets::*;
use turbo_ferret::*;

#[derive(Default)]
struct NullR {
    next: u32,
}
impl Renderer for NullR {
    fn create_bitmap(&mut self, _size: Size, _rgba: &[u8]) -> Option<BitmapId> {
        self.next += 1;
        Some(BitmapId(self.next))
    }
    fn update_bitmap(&mut self, _bitmap: BitmapId, _rgba: &[u8]) {}
    fn destroy_bitmap(&mut self, _bitmap: BitmapId) {}
    fn draw(&mut self, _bitmap: Option<BitmapId>, _clip: Rect, _quads: &[Quad]) {}
}

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

/// With the default skin and font.
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

fn names() -> Rc<RefCell<StringItemSource>> {
    let src = StringItemSource::shared();
    for (name, id) in [("Cherry", "c"), ("apple", "a"), ("Banana", "b"), ("date", "d")] {
        src.borrow_mut().add(StringItem::with_id(name, TbId::new(id)));
    }
    src
}

// ---------------------------------------------------------------------------

/// TB's ReadItems used GetValueString, which translates "@key" language strings.
#[test]
fn p01_item_text_language_strings() {
    let mut ui = ui();
    ui.lang_mut().set("hello", "Hello!");
    let root = ui.root();
    let src = "TBSelectDropdown: id: drop\n\titems\n\t\titem: text: \"@hello\"\n\t\titem: text: \"Plain\"\nTBButton: text: \"@hello\"\n";
    let ids = ui.inflate(root, src).unwrap();
    ui.update();
    assert_eq!(ui.text(ids[1]), Some("Hello!"), "generic text property translates");
    assert_eq!(ui.text(ids[0]), Some("Hello!"), "item text should translate as in TB");
}

/// A popup for a target taller than half the root gets a negative height.
#[test]
fn p02_popup_negative_height() {
    let mut ui = skinned();
    ui.set_viewport(Size::new(300, 60));
    let root = ui.root();
    let d = ui.add(root, SelectDropdown::default());
    ui.set_rect(d, Rect::new(10, 10, 120, 40));
    let src = names();
    ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.set_source(cx, Some(src.clone())));
    ui.update();
    click(&mut ui, d);
    let menu = ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.menu_if_open(cx)).flatten().expect("menu open");
    ui.update();
    let mut r = NullR::default();
    ui.paint(&mut r);
    let mr = ui.rect(menu);
    assert!(mr.h >= 0, "menu rect {mr:?}");
}

/// TB: TBSection::SetValue goes through the header, whose Changed handler asks for a
/// scroll into view. Here the section is busy in set_value and misses it.
#[test]
fn p03_section_set_value_scrolls_into_view() {
    let mut ui = ui();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Off));
    ui.with_widget::<ScrollContainer, _>(sc, |w, cx| w.set_adapt_content_size(cx, true));
    let col = ui.add(ui.content_root(sc), Layout::new(Axis::Y));
    let spacer = ui.add(col, Plain);
    ui.set_layout_params(spacer, Some(LayoutParams::fixed(100, 150)));
    let sec = ui.add(col, Section::new("Far down"));
    let body = ui.add(ui.content_root(sec), Plain);
    ui.set_layout_params(body, Some(LayoutParams::fixed(100, 120)));
    let header = ui.get::<Section>(sec).unwrap().header().unwrap();
    ui.set_layout_params(header, Some(LayoutParams::fixed(100, 20)));
    ui.set_rect(sc, Rect::new(0, 0, 100, 200));
    ui.update();
    ui.set_value(sec, 1);
    ui.update();
    ui.update();
    let info = ui.scroll_info(sc).unwrap();
    assert!(info.max.y > 0);
    assert!(info.pos.y > 0, "expanded by value: should scroll into view as in TB, pos {:?}", info.pos);
}

/// TB: DeleteAllItems resets the list's value to -1 at once.
#[test]
fn p04_list_value_after_clear_and_refill() {
    let mut ui = ui();
    let root = ui.root();
    let src = names();
    let l = ui.add(root, SelectList::default());
    ui.with_widget::<SelectList, _>(l, |w, cx| w.set_source(cx, Some(src.clone())));
    ui.set_rect(l, Rect::new(10, 10, 150, 100));
    ui.update();
    ui.set_value(l, 2);
    ui.update();
    {
        let mut s = src.borrow_mut();
        s.clear();
        s.add(StringItem::new("x"));
        s.add(StringItem::new("y"));
        s.add(StringItem::new("z"));
    }
    ui.update();
    assert_eq!(ui.value(l), Value::Int(-1), "TB: OnAllItemsRemoved cleared the selection");
}

/// A target that removes the window whose click it gets.
struct Remover;
impl Widget for Remover {
    fn type_name(&self) -> &'static str {
        "Remover"
    }
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if ev.kind == EventKind::Click && cx.parent(ev.target) == Some(cx.root()) {
            cx.remove(ev.target);
        }
        false
    }
}

/// Removing a message window from the handler of its own answer.
#[test]
fn p05_message_window_removed_in_answer_handler() {
    let mut ui = ui();
    let root = ui.root();
    let b = ui.add(root, Remover);
    ui.set_rect(b, Rect::new(10, 10, 80, 20));
    let settings = MessageSettings { dimmer: true, ..MessageSettings::new(MessageButtons::Ok, TbId::NONE) };
    let win = ui.show_message(b, tbid!("confirm"), "Sure?", "Really?", &settings).unwrap();
    ui.update();
    let kids = ui.children(root).to_vec();
    let dimmer = kids[kids.len() - 2];
    assert!(ui.get::<Dimmer>(dimmer).is_some());
    let ok = ui.find(win, tbid!("TBMessageWindow.ok")).unwrap();
    ui.send(ok, EventKind::Click);
    settle(&mut ui);
    assert!(!ui.exists(win));
    assert!(!ui.exists(dimmer), "the dimmer stays forever");
}

/// Window: turning the title bar off moves the content up, also when something
/// measures the window before the next update.
#[test]
fn p06_window_settings_then_measure() {
    let mut ui = skinned();
    let root = ui.root();
    let w = ui.add(root, Window::new("T"));
    ui.with_widget::<Window, _>(w, |win, _| win.animate = false);
    ui.set_rect(w, Rect::new(0, 0, 200, 150));
    let c = ui.add(w, Plain);
    ui.set_gravity(c, Gravity::ALL);
    ui.place_child(w, c);
    ui.update();
    let y0 = ui.rect(c).y;
    assert!(y0 > 10, "{y0}");
    ui.set_window_settings(w, WindowSettings::NONE);
    let _ = ui.preferred_size_free(w);
    ui.update();
    let pr = ui.padding_rect(w);
    assert_eq!(ui.rect(c).y, pr.y, "content rect {:?} padding {:?}", ui.rect(c), pr);
}

/// Keyboard on a list: all disabled items, empty list.
#[test]
fn p07_list_keys_edge_cases() {
    let mut ui = ui();
    let root = ui.root();
    let src = StringItemSource::shared();
    let l = ui.add(root, SelectList::default());
    ui.with_widget::<SelectList, _>(l, |w, cx| w.set_source(cx, Some(src.clone())));
    ui.set_rect(l, Rect::new(10, 10, 150, 100));
    ui.update();
    ui.set_focus(l);
    for k in [SpecialKey::Down, SpecialKey::Up, SpecialKey::Home, SpecialKey::End] {
        ui.key(None, k, Modifiers::NONE, true);
    }
    src.borrow_mut().add(StringItem::new("-"));
    src.borrow_mut().add(StringItem::new("-"));
    ui.update();
    for k in [SpecialKey::Down, SpecialKey::Up, SpecialKey::Home, SpecialKey::End] {
        ui.key(None, k, Modifiers::NONE, true);
    }
    assert_eq!(ui.value(l), Value::Int(-1));
    ui.with_widget::<SelectList, _>(l, |w, cx| w.set_filter(cx, "zzz"));
    ui.update();
    for k in [SpecialKey::Down, SpecialKey::Up, SpecialKey::Home, SpecialKey::End] {
        ui.key(None, k, Modifiers::NONE, true);
    }
    assert_eq!(ui.value(l), Value::Int(-1));
}

/// Radio group: a Changed handler that selects another radio.
#[test]
fn p08_radio_group_handler_selects_other() {
    let mut ui = ui();
    let root = ui.root();
    let ids = ui
        .inflate(
            root,
            "TBRadioButton: id: a, group-id: g\nTBRadioButton: id: b, group-id: g\nTBRadioButton: id: c, group-id: g\n",
        )
        .unwrap();
    for (i, &w) in ids.iter().enumerate() {
        ui.set_rect(w, Rect::new(10, 10 + 30 * i as i32, 20, 20));
    }
    let (a, b) = (ids[0], ids[1]);
    // "a" is not allowed: choosing it selects "b" instead.
    ui.on_event(a, move |ui, ev| {
        if ev.kind == EventKind::Changed && ev.value.as_ref().is_some_and(|v| v.as_int() == 1) {
            ui.set_value(b, 1);
        }
        false
    });
    click(&mut ui, a);
    let vals: Vec<i32> = ids.iter().map(|&w| ui.value(w).as_int()).collect();
    assert_eq!(vals.iter().sum::<i32>(), 1, "exactly one radio is on: {vals:?}");
}
