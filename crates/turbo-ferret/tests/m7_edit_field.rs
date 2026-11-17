//! M7: the edit field widget (headless): typing, pointer selection, clipboard
//! shortcuts, undo and redo, caret blink, scrolling, sizes, the context menu,
//! embedded widgets, value connections, and the widgets that use it.
//!
//! Without a loaded font the box font is used: at size 27 every character is 10
//! wide and a line 27 high.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::edit::{CaretPos, Content, TextOfs};
use turbo_ferret::skin::Condition;
use turbo_ferret::skin::condition::{CondProp, CondTarget, Test};
use turbo_ferret::widgets::*;
use turbo_ferret::*;

const CW: i32 = 10;
const LH: i32 = 27;

fn ui() -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(800, 600));
    ui.set_time(1000);
    for key in ["cut", "copy", "paste", "delete", "selectall"] {
        ui.lang_mut().set(key, key);
    }
    let root = ui.root();
    ui.set_font(root, Some(FontDesc::new(TbId::NONE, LH as u16)));
    ui
}

/// An edit field at (10, 10), 200 wide; one line, or five lines high.
fn field(ui: &mut Ui, multiline: bool) -> WidgetId {
    let root = ui.root();
    let e = ui.add(root, EditField::default());
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_multiline(cx, multiline));
    ui.set_rect(e, Rect::new(10, 10, 200, if multiline { 5 * LH } else { LH }));
    ui.update();
    e
}

fn type_text(ui: &mut Ui, text: &str) {
    for c in text.chars() {
        ui.key(Some(c), SpecialKey::None, Modifiers::NONE, true);
        ui.key(Some(c), SpecialKey::None, Modifiers::NONE, false);
    }
}

fn press(ui: &mut Ui, k: SpecialKey, mods: Modifiers) -> bool {
    let handled = ui.key(None, k, mods, true);
    ui.key(None, k, mods, false);
    handled
}

fn caret(ui: &Ui, e: WidgetId) -> TextOfs {
    ui.get::<EditField>(e).unwrap().style_edit().caret.pos
}

fn selected(ui: &Ui, e: WidgetId) -> String {
    ui.get::<EditField>(e).unwrap().style_edit().selected_text()
}

type Changes = Rc<RefCell<Vec<String>>>;

/// The texts of the `Changed` events of `e`.
fn changes(ui: &mut Ui, e: WidgetId) -> Changes {
    let log: Changes = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    ui.on_event(e, move |_, ev| {
        if ev.kind == EventKind::Changed {
            l.borrow_mut().push(ev.value.as_ref().map(|v| v.as_str().into_owned()).unwrap_or_default());
        }
        false
    });
    log
}

#[test]
fn typing_changes_the_text_and_reports_it() {
    let mut ui = ui();
    let e = field(&mut ui, false);
    let log = changes(&mut ui, e);
    assert!(ui.node(e).unwrap().is_focusable());
    assert_eq!(ui.text(e), Some(""));
    assert!(ui.set_focus(e));
    type_text(&mut ui, "Hej då");
    assert_eq!(ui.text(e), Some("Hej då"));
    assert_eq!(ui.value(e), Value::str("Hej då"));
    assert_eq!(log.borrow().last().map(String::as_str), Some("Hej då"));
    assert_eq!(log.borrow().len(), 6);
    assert!(press(&mut ui, SpecialKey::Backspace, Modifiers::NONE));
    assert_eq!(ui.text(e), Some("Hej d"));
    // Moving the caret is not a change, and key-up events are swallowed.
    let n = log.borrow().len();
    assert!(press(&mut ui, SpecialKey::Left, Modifiers::NONE));
    assert_eq!(log.borrow().len(), n);
    assert_eq!(caret(&ui, e), TextOfs::new(0, 4));
    // A single line field leaves Enter and Tab to others (Tab moves the focus).
    assert!(!ui.key(None, SpecialKey::Enter, Modifiers::NONE, true));
    assert_eq!(ui.text(e), Some("Hej d"));

    // Set from code: the caret goes to the start, and a change is reported.
    ui.set_text(e, "New text");
    assert_eq!((ui.text(e), caret(&ui, e)), (Some("New text"), TextOfs::new(0, 0)));
    assert_eq!(log.borrow().last().map(String::as_str), Some("New text"));
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_text_and_caret(cx, "At the end", CaretPos::End));
    assert_eq!(caret(&ui, e).ofs, 10);
    ui.with_widget::<EditField, _>(e, |w, cx| w.append_text(cx, "!", false));
    assert_eq!(ui.text(e), Some("At the end!"));
}

#[test]
fn inflated_properties() {
    let mut ui = ui();
    ui.lang_mut().set("search", "Search");
    let root = ui.root();
    let src = r#"
TBEditField: id: a, text: "Hello", placeholder: "@search", type: "search", text-align: right
TBEditField: id: b, multiline: 1, readonly: 1, styling: 1, adapt-to-content: 1, virtual-width: 300
	text: "One\nTwo"
TBEditField: id: c, type: password, wrap: 1
"#;
    let ids = ui.inflate(root, src).unwrap();
    let a = ui.get::<EditField>(ids[0]).unwrap();
    assert_eq!(ui.text(ids[0]), Some("Hello"));
    assert_eq!((a.placeholder(), a.edit_type(), a.text_align()), ("Search", EditType::Search, TextAlign::Right));
    assert!(!a.is_multiline() && !a.is_read_only() && !a.is_wrapping());
    let b = ui.get::<EditField>(ids[1]).unwrap();
    assert!(b.is_multiline() && b.is_read_only() && b.is_styling() && b.is_wrapping());
    assert!(b.adapt_to_content_size());
    assert_eq!(b.virtual_width(), 300);
    assert_eq!(b.style_edit().blocks().len(), 2);
    let c = ui.get::<EditField>(ids[2]).unwrap();
    assert_eq!(c.edit_type(), EditType::Password);
    assert!(c.style_edit().is_password() && c.is_wrapping());
    assert_eq!(EditType::parse("E-Mail"), None);
    assert_eq!(EditType::parse("my email field"), Some(EditType::Email));
}

#[test]
fn pointer_places_the_caret_and_selects() {
    let mut ui = ui();
    let e = field(&mut ui, false);
    ui.set_text(e, "one two three");
    // A click between "t" and "w".
    let at = |chars: i32| Point::new(10 + chars * CW + 1, 20);
    ui.pointer_down(at(5), 1, Modifiers::NONE);
    assert_eq!(ui.focused(), Some(e));
    assert_eq!(caret(&ui, e).ofs, 5);
    // Dragging selects; the field keeps the pointer, so nothing pans.
    ui.pointer_move(at(10), Modifiers::NONE);
    assert_eq!(selected(&ui, e), "wo th");
    ui.pointer_up(at(10), Modifiers::NONE);
    assert_eq!(selected(&ui, e), "wo th");
    // Typing replaces the selection.
    type_text(&mut ui, "X");
    assert_eq!(ui.text(e), Some("one tXree"));
    // A double click selects a word.
    ui.pointer_down(at(1), 2, Modifiers::NONE);
    ui.pointer_up(at(1), Modifiers::NONE);
    assert_eq!(selected(&ui, e), "one ");
}

#[test]
fn clipboard_shortcuts_and_undo() {
    let mut ui = ui();
    let e = field(&mut ui, false);
    ui.set_focus(e);
    type_text(&mut ui, "one two");
    let shortcut = |ui: &mut Ui, name: &str| ui.shortcut(TbId::new(name), Modifiers::NONE);
    assert!(shortcut(&mut ui, "selectall"));
    assert_eq!(selected(&ui, e), "one two");
    assert!(shortcut(&mut ui, "copy"));
    assert_eq!(ui.clipboard_text().as_deref(), Some("one two"));
    assert_eq!(ui.text(e), Some("one two"));
    // Cut a word.
    ui.with_widget::<EditField, _>(e, |w, cx| w.select(cx, 0, 4));
    assert!(shortcut(&mut ui, "cut"));
    assert_eq!(ui.text(e), Some("two"));
    assert_eq!(ui.clipboard_text().as_deref(), Some("one "));
    // Paste it at the end, twice.
    press(&mut ui, SpecialKey::End, Modifiers::NONE);
    shortcut(&mut ui, "paste");
    shortcut(&mut ui, "paste");
    assert_eq!(ui.text(e), Some("twoone one "));
    // Undo and redo step through the changes.
    let w = ui.get::<EditField>(e).unwrap();
    assert!(w.can_undo() && !w.can_redo());
    shortcut(&mut ui, "undo");
    assert_eq!(ui.text(e), Some("twoone "));
    shortcut(&mut ui, "undo");
    assert_eq!(ui.text(e), Some("two"));
    shortcut(&mut ui, "redo");
    assert_eq!(ui.text(e), Some("twoone "));
    assert!(ui.get::<EditField>(e).unwrap().can_redo());
    shortcut(&mut ui, "delete");
    assert_eq!(ui.text(e), Some("two"), "the redone text was selected, and deleted");
    assert!(!shortcut(&mut ui, "save"), "other shortcuts are not the field's");

    // Read only: copy works, nothing else changes the text. Nothing is copied from
    // a password field.
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_read_only(cx, true));
    shortcut(&mut ui, "selectall");
    shortcut(&mut ui, "cut");
    shortcut(&mut ui, "paste");
    type_text(&mut ui, "x");
    assert_eq!(ui.text(e), Some("two"));
    ui.set_clipboard_text("kept");
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_edit_type(cx, EditType::Password));
    shortcut(&mut ui, "copy");
    assert_eq!(ui.clipboard_text().as_deref(), Some("kept"));
}

#[test]
fn caret_blinks_while_focused() {
    let mut ui = ui();
    let e = field(&mut ui, false);
    let on = |ui: &Ui| ui.get::<EditField>(e).unwrap().style_edit().caret.on;
    assert!(!on(&ui));
    assert_eq!(ui.next_wake_time(), None);
    ui.set_focus(e);
    assert!(on(&ui));
    assert_eq!(ui.next_wake_time(), Some(1500));
    ui.set_time(1500);
    ui.update();
    assert!(!on(&ui));
    assert_eq!(ui.next_wake_time(), Some(2000));
    ui.set_time(2000);
    ui.update();
    assert!(on(&ui));
    // A key shows the caret for a full period again.
    ui.set_time(2400);
    press(&mut ui, SpecialKey::Left, Modifiers::NONE);
    assert!(on(&ui));
    assert_eq!(ui.next_wake_time(), Some(2900));
    // Without focus there is no caret and no timer.
    let root = ui.root();
    let other = ui.add(root, Button::new("other"));
    ui.set_focus(other);
    assert!(!on(&ui));
    assert_eq!(ui.next_wake_time(), None);
}

#[test]
fn multiline_scrolls_and_wraps() {
    let mut ui = ui();
    let e = field(&mut ui, true);
    let w = ui.get::<EditField>(e).unwrap();
    assert!(w.is_multiline() && w.is_wrapping());
    ui.set_focus(e);
    // 200 wide: 20 characters fit on a line.
    type_text(&mut ui, "aaaaaaaaa bbbbbbbbb ccccccccc");
    let se = ui.get::<EditField>(e).unwrap().style_edit();
    assert_eq!((se.blocks().len(), se.content_height()), (1, 2 * LH));
    // Enter makes lines; past the fifth line it scrolls.
    for _ in 0..6 {
        assert!(press(&mut ui, SpecialKey::Enter, Modifiers::NONE));
    }
    let info = ui.scroll_info(e).unwrap();
    assert_eq!(info.max.y, 8 * LH - 5 * LH);
    assert_eq!(info.pos.y, info.max.y, "the caret is kept in view");
    assert_eq!(ui.get::<EditField>(e).unwrap().overflow_y(), 3 * LH);
    // The wheel scrolls back, a line and a half per notch (40 px).
    assert!(ui.wheel(Point::new(50, 50), Point::new(0, -1), Modifiers::NONE));
    assert_eq!(ui.scroll_info(e).unwrap().pos.y, 3 * LH - 40);
    // `scroll_to` (panning uses it) clamps.
    ui.scroll_to(e, Point::new(0, -100));
    assert_eq!(ui.scroll_info(e).unwrap().pos, Point::ZERO);
    // Typing brings the caret back into view.
    type_text(&mut ui, "x");
    assert_eq!(ui.scroll_info(e).unwrap().pos.y, 3 * LH);
}

#[test]
fn single_line_scrolls_horizontally() {
    let mut ui = ui();
    let e = field(&mut ui, false);
    ui.set_focus(e);
    type_text(&mut ui, "0123456789012345678901234");
    let info = ui.scroll_info(e).unwrap();
    assert_eq!((info.max, info.pos), (Point::new(50, 0), Point::new(50, 0)));
    press(&mut ui, SpecialKey::Home, Modifiers::NONE);
    assert_eq!(ui.scroll_info(e).unwrap().pos, Point::ZERO);
    // Dragging a selection past the right edge scrolls it along (on a timer).
    ui.pointer_down(Point::new(100, 20), 1, Modifiers::NONE);
    ui.pointer_move(Point::new(260, 20), Modifiers::NONE);
    for _ in 0..10 {
        ui.set_time(ui.now() + 40);
        ui.update();
    }
    assert_eq!(ui.scroll_info(e).unwrap().pos.x, 50);
    assert!(selected(&ui, e).ends_with("901234"));
    ui.pointer_up(Point::new(260, 20), Modifiers::NONE);
    let x = ui.scroll_info(e).unwrap().pos.x;
    ui.set_time(ui.now() + 200);
    ui.update();
    assert_eq!(ui.scroll_info(e).unwrap().pos.x, x, "the timer stops with the selection");
}

#[test]
fn preferred_sizes() {
    let mut ui = ui();
    let root = ui.root();
    let one = ui.add(root, EditField::default());
    let ps = ui.preferred_size_free(one);
    assert_eq!((ps.pref.h, ps.min.h, ps.max.h), (LH, LH, LH));
    let many = ui.add(root, EditField::default());
    ui.with_widget::<EditField, _>(many, |w, cx| w.set_multiline(cx, true));
    assert_eq!(ui.preferred_size_free(many).pref, Size::new(10 * LH, 5 * LH));

    // Adapting to the content: as large as the text, wrapped at the virtual width.
    ui.with_widget::<EditField, _>(many, |w, cx| {
        w.set_adapt_to_content_size(cx, true);
        w.set_virtual_width(cx, 100);
    });
    ui.set_text(many, "aaaa bbbb cccc");
    let ps = ui.preferred_size_free(many);
    // (The space that ends the first line counts for its width.)
    assert_eq!((ps.pref, ps.min.h, ps.max.h), (Size::new(100, 2 * LH), 2 * LH, 2 * LH));
    // With a width given, it wraps at that instead.
    assert_eq!(ui.preferred_size(many, SizeConstraints::new(60, 1000)).pref, Size::new(50, 3 * LH));
    // More text is a new preferred size; an empty field is one line high.
    ui.with_widget::<EditField, _>(many, |w, cx| w.append_text(cx, " dddd eeee", false));
    assert_eq!(ui.preferred_size_free(many).pref.h, 3 * LH);
    ui.set_text(many, "");
    assert_eq!(ui.preferred_size_free(many).pref, Size::new(0, LH));
}

#[test]
fn context_menu_runs_the_commands() {
    let mut ui = ui();
    let e = field(&mut ui, false);
    ui.set_text(e, "pick me");
    ui.set_focus(e);
    let root = ui.root();
    let mut ev = Event::new(EventKind::ContextMenu, e);
    ev.pos = Point::new(30, 5);
    assert!(ui.invoke_event(ev));
    let menu = *ui.children(root).last().unwrap();
    let list = ui.get::<MenuWindow>(menu).expect("a menu").list().unwrap();
    assert_eq!(ui.rect(menu).origin(), Point::new(40, 15));
    let layout = ui.get::<SelectList>(list).unwrap().item_layout().unwrap();
    let items: Vec<_> = ui.children(layout).to_vec();
    let names: Vec<_> = items.iter().map(|&i| ui.text(i).unwrap_or("-").to_string()).collect();
    assert_eq!(names, ["cut", "copy", "paste", "delete", "-", "selectall"]);
    // Choose "selectall", then "copy" from a second menu.
    let choose = |ui: &mut Ui, index: usize| {
        let mut ev = Event::new(EventKind::ContextMenu, e);
        ev.pos = Point::new(30, 5);
        ui.invoke_event(ev);
        let menu = *ui.children(ui.root()).last().unwrap();
        let list = ui.get::<MenuWindow>(menu).unwrap().list().unwrap();
        let layout = ui.get::<SelectList>(list).unwrap().item_layout().unwrap();
        let item = ui.children(layout)[index];
        ui.send(item, EventKind::Click);
    };
    choose(&mut ui, 5);
    assert_eq!(selected(&ui, e), "pick me");
    choose(&mut ui, 1);
    assert_eq!(ui.clipboard_text().as_deref(), Some("pick me"));
    choose(&mut ui, 3);
    assert_eq!(ui.text(e), Some(""));
}

#[test]
fn embedded_widgets_live_in_the_text() {
    let mut ui = ui();
    let e = field(&mut ui, true);
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_styling(cx, true));
    ui.set_text(e, "Press <widget TBButton: text: \"Hi\", id: \"hi\"> now");
    ui.update();
    let hi = ui.find(e, tbid!("hi")).expect("the button exists");
    assert!(ui.get::<Button>(hi).is_some());
    // The fragment is the wrapper of the button, placed after "Press ".
    let se = ui.get::<EditField>(e).unwrap().style_edit();
    let frag = se.blocks()[0].fragments.iter().find(|f| f.content.is_some()).unwrap();
    let Some(Content::Widget(wrapper)) = frag.content else { panic!("{:?}", frag.content) };
    assert_eq!(ui.parent(hi), Some(wrapper));
    assert_eq!(frag.xpos, 6 * CW);
    assert_eq!(ui.rect(wrapper).x, 6 * CW);
    assert!(frag.width > 0 && ui.rect(wrapper).w == frag.width);
    // It takes input itself; the text around it belongs to the field.
    let p = ui.to_root(hi, Point::new(2, 2));
    assert_eq!(ui.widget_at(ui.root(), p), Some(hi));
    assert_eq!(ui.widget_at(ui.root(), Point::new(12, 12)), Some(e));
    let clicked = Rc::new(RefCell::new(0));
    let c = clicked.clone();
    ui.on_event(hi, move |_, ev| {
        if ev.kind == EventKind::Click {
            *c.borrow_mut() += 1;
        }
        false
    });
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(*clicked.borrow(), 1);
    // A change of its line makes the widget anew (as in TB). It scrolls with the
    // text.
    ui.with_widget::<EditField, _>(e, |w, cx| w.append_text(cx, "\n\n\n\n\n\n\n", false));
    assert!(!ui.exists(hi));
    let hi = ui.find(e, tbid!("hi")).expect("made again");
    assert_eq!(ui.to_root(hi, Point::ZERO).y, p.y - 2);
    ui.scroll_to(e, Point::new(0, LH));
    assert_eq!(ui.to_root(hi, Point::ZERO).y, p.y - 2 - LH);
    let wrapper = ui.parent(hi).unwrap();
    // New text: the old widget is gone.
    ui.set_text(e, "plain");
    ui.update();
    assert!(!ui.exists(hi) && !ui.exists(wrapper));
}

#[test]
fn value_connection_goes_both_ways() {
    let mut ui = ui();
    let root = ui.root();
    let ids = ui
        .inflate(root, "TBEditField: connection: user-name\nTBEditField: connection: user-name\nTBTextField: connection: user-name\n")
        .unwrap();
    ui.set_rect(ids[0], Rect::new(0, 0, 200, LH));
    ui.update();
    ui.set_focus(ids[0]);
    type_text(&mut ui, "Ada");
    assert_eq!(ui.text(ids[1]), Some("Ada"));
    assert_eq!(ui.text(ids[2]), Some("Ada"));
    assert_eq!(ui.bound_value(tbid!("user-name")).map(|v| v.as_str().into_owned()).as_deref(), Some("Ada"));
    ui.set_bound_value(tbid!("user-name"), Value::str("Grace"));
    assert_eq!(ui.text(ids[0]), Some("Grace"));
}

#[test]
fn skin_conditions() {
    let mut ui = ui();
    let e = field(&mut ui, false);
    let cond = |prop: &str, value: TbId| Condition {
        target: CondTarget::This,
        prop: CondProp::Custom(TbId::new(prop)),
        value,
        test: Test::Equal,
    };
    let check = |ui: &Ui, c: &Condition| ui.get::<EditField>(e).unwrap().custom_skin_condition(c);
    assert!(check(&ui, &cond("edit-type", tbid!("text"))));
    assert!(!check(&ui, &cond("edit-type", tbid!("search"))));
    assert!(check(&ui, &cond("multiline", TbId(0))) && !check(&ui, &cond("multiline", TbId(1))));
    assert!(check(&ui, &cond("readonly", TbId(0))));
    ui.with_widget::<EditField, _>(e, |w, cx| {
        w.set_edit_type(cx, EditType::Search);
        w.set_multiline(cx, true);
        w.set_read_only(cx, true);
    });
    assert!(check(&ui, &cond("edit-type", tbid!("search"))));
    assert!(check(&ui, &cond("multiline", TbId(1))));
    assert!(check(&ui, &cond("readonly", TbId(1))));
    assert!(!check(&ui, &cond("something-else", TbId(1))));
}

#[test]
fn inline_select_takes_typed_numbers() {
    let mut ui = ui();
    let root = ui.root();
    let s = ui.inflate(root, "TBInlineSelect: min: 0, max: 50\n").unwrap()[0];
    ui.set_rect(s, Rect::new(10, 10, 160, LH));
    ui.update();
    let edit = ui.find_as::<EditField>(s, TbId::NONE).expect("its edit field");
    assert_eq!(ui.text(edit), Some("0"));
    assert_eq!(ui.get::<EditField>(edit).unwrap().edit_type(), EditType::Number);
    ui.set_focus(edit);
    press(&mut ui, SpecialKey::End, Modifiers::NONE);
    press(&mut ui, SpecialKey::Backspace, Modifiers::NONE);
    type_text(&mut ui, "42");
    assert_eq!(ui.value(s), Value::Int(42));
    // The typed text stays as typed, even beyond the limit; the value is clamped.
    type_text(&mut ui, "9");
    assert_eq!((ui.value(s), ui.text(edit)), (Value::Int(50), Some("429")));
    // Up and Down in the field step the value (the field does not use them).
    press(&mut ui, SpecialKey::Down, Modifiers::NONE);
    assert_eq!((ui.value(s), ui.text(edit)), (Value::Int(49), Some("49")));
    press(&mut ui, SpecialKey::Up, Modifiers::NONE);
    assert_eq!(ui.value(s), Value::Int(50));
}

#[test]
fn message_window_grows_to_show_its_text() {
    let mut ui = ui();
    ui.lang_mut().set("TBMessageWindow.ok", "Ok");
    let root = ui.root();
    let b = ui.add(root, Button::new("ask"));
    let short = ui.show_message(b, tbid!("a"), "Title", "Short.", &MessageSettings::default()).unwrap();
    let long_text = "A much longer text. ".repeat(20);
    let long = ui.show_message(b, tbid!("b"), "Title", &long_text, &MessageSettings::default()).unwrap();
    ui.update();
    assert!(ui.rect(long).h > ui.rect(short).h + 3 * LH, "{:?} vs {:?}", ui.rect(long), ui.rect(short));
    // The text is a read-only, multi line edit field; with enough room nothing
    // overflows.
    let edit = ui.find_as::<EditField>(long, TbId::NONE).unwrap();
    let w = ui.get::<EditField>(edit).unwrap();
    assert!(w.is_read_only() && w.is_multiline());
    assert_eq!(w.overflow_y(), 0);
    assert_eq!(ui.text(edit), Some(long_text.as_str()));
}
