//! `EditField` regressions found in review (2026-11), and a randomized test of the
//! field through a headless `Ui`.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::edit::{Content, TextOfs};
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

type Changes = Rc<RefCell<Vec<String>>>;

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

fn lines(n: usize) -> String {
    (0..n).map(|i| format!("line {i}\n")).collect()
}

/// The scroll position of the text, of the scroll info and of the vertical bar.
fn scroll_state(ui: &Ui, e: WidgetId) -> (i32, i32, i32, i32) {
    let w = ui.get::<EditField>(e).unwrap();
    let info = ui.scroll_info(e).unwrap();
    (w.style_edit().scroll().y, info.pos.y, info.max.y, w.style_edit().content_height())
}

#[test]
fn e1_delete_command_keeps_the_scroll_position_in_range() {
    let mut ui = ui();
    let e = field(&mut ui, true);
    ui.set_focus(e);
    ui.set_text(e, &lines(30));
    ui.update();
    press(&mut ui, SpecialKey::End, Modifiers::CTRL);
    let (scroll, pos, max, _) = scroll_state(&ui, e);
    assert!(scroll > 0 && scroll == pos && pos == max, "{scroll} {pos} {max}");
    // The context menu's "selectall" and "delete".
    ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, tbid!("selectall")));
    ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, tbid!("delete")));
    ui.update();
    assert_eq!(ui.text(e), Some(""));
    let (scroll, pos, max, height) = scroll_state(&ui, e);
    assert_eq!((scroll, pos, max), (0, 0, 0), "content height {height}");
}

#[test]
fn e2_turning_wrapping_off_keeps_the_scroll_position_in_range() {
    let mut ui = ui();
    let e = field(&mut ui, true);
    ui.set_focus(e);
    // Every line wraps into three.
    let text: String = (0..10).map(|_| "aaaaaaaaaaaaaaa bbbbbbbbbbbbbbb ccccccccccccccc\n").collect();
    ui.set_text(e, &text);
    ui.update();
    press(&mut ui, SpecialKey::End, Modifiers::CTRL);
    let (scroll, _, max, _) = scroll_state(&ui, e);
    assert!(scroll > 0 && scroll == max);
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_wrapping(cx, false));
    ui.update();
    let (scroll, pos, max, height) = scroll_state(&ui, e);
    assert!(scroll <= max && pos <= max, "scroll {scroll}, info {pos}, max {max}, content height {height}");
}

fn wrapper_of(ui: &Ui, e: WidgetId) -> (usize, WidgetId) {
    let se = ui.get::<EditField>(e).unwrap().style_edit();
    for (bi, b) in se.blocks().iter().enumerate() {
        for f in &b.fragments {
            if let Some(Content::Widget(w)) = f.content {
                return (bi, w);
            }
        }
    }
    panic!("no widget");
}

#[test]
fn e3_embedded_widget_is_placed_on_its_line() {
    let mut ui = ui();
    let e = field(&mut ui, true);
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_styling(cx, true));
    ui.update();
    ui.set_text(e, "one\ntwo\n<widget TBButton: text: \"Hi\", id: \"hi\">\nfour");
    ui.update();
    let (bi, wrapper) = wrapper_of(&ui, e);
    let se = ui.get::<EditField>(e).unwrap().style_edit();
    let block_y = se.blocks()[bi].ypos;
    let frag = se.blocks()[bi].fragments.iter().find(|f| f.content.is_some()).unwrap();
    assert_eq!((bi, block_y), (2, 2 * LH));
    assert_eq!(ui.rect(wrapper).y, block_y + frag.ypos, "the wrapper's y after set_text");
}

#[test]
fn e3b_embedded_widget_follows_its_line() {
    let mut ui = ui();
    let e = field(&mut ui, true);
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_styling(cx, true));
    ui.set_focus(e);
    // Set before the first layout pass of this text, then force a clean layout.
    ui.set_text(e, "one\ntwo\n<widget TBButton: text: \"Hi\", id: \"hi\">\nfour");
    ui.set_rect(e, Rect::new(10, 10, 210, 5 * LH));
    ui.update();
    let (_, wrapper) = wrapper_of(&ui, e);
    let y0 = ui.rect(wrapper).y;
    // A new line above.
    assert_eq!(caret(&ui, e), TextOfs::new(0, 0));
    press(&mut ui, SpecialKey::Enter, Modifiers::NONE);
    ui.update();
    let (bi, wrapper2) = wrapper_of(&ui, e);
    let se = ui.get::<EditField>(e).unwrap().style_edit();
    assert_eq!((bi, se.blocks()[bi].ypos), (3, 3 * LH));
    assert_eq!(wrapper, wrapper2, "the widget's own line did not change");
    assert_eq!(ui.rect(wrapper2).y, y0 + LH, "y before {y0}");
}

#[test]
fn e4_changed_events_match_the_text() {
    let mut ui = ui();
    let e = field(&mut ui, true);
    let log = changes(&mut ui, e);
    ui.set_focus(e);
    type_text(&mut ui, "héllo wörld");
    press(&mut ui, SpecialKey::Enter, Modifiers::NONE);
    type_text(&mut ui, "日本");
    assert_eq!(ui.text(e), log.borrow().last().map(String::as_str));
    ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, tbid!("undo")));
    assert_eq!(ui.text(e), log.borrow().last().map(String::as_str));
    ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, tbid!("selectall")));
    ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, tbid!("cut")));
    assert_eq!(ui.text(e), Some(""));
    assert_eq!(log.borrow().last().map(String::as_str), Some(""));
    let n = log.borrow().len();
    ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, tbid!("paste")));
    assert_eq!(log.borrow().len(), n + 1);
    assert_eq!(ui.text(e), log.borrow().last().map(String::as_str));
}

#[test]
fn e5_read_only_cannot_be_changed_by_input() {
    let mut ui = ui();
    let e = field(&mut ui, true);
    ui.set_text(e, "fixed text\nsecond");
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_read_only(cx, true));
    ui.set_focus(e);
    let log = changes(&mut ui, e);
    ui.set_clipboard_text("clip");
    type_text(&mut ui, "x");
    for k in [SpecialKey::Delete, SpecialKey::Backspace, SpecialKey::Enter, SpecialKey::Tab] {
        press(&mut ui, k, Modifiers::NONE);
    }
    ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, tbid!("selectall")));
    for id in [tbid!("cut"), tbid!("paste"), tbid!("delete"), tbid!("undo"), tbid!("redo")] {
        ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, id));
    }
    press(&mut ui, SpecialKey::Delete, Modifiers::NONE);
    type_text(&mut ui, "y");
    assert_eq!(ui.text(e), Some("fixed text\nsecond"));
    assert_eq!(*log.borrow(), Vec::<String>::new());
}

#[test]
fn e6_right_aligned_overflowing_text_can_be_scrolled_to_its_start() {
    let mut ui = ui();
    let e = field(&mut ui, false);
    ui.with_widget::<EditField, _>(e, |w, cx| w.set_text_align(cx, TextAlign::Right));
    ui.set_focus(e);
    ui.set_text(e, "0123456789012345678901234");
    ui.update();
    press(&mut ui, SpecialKey::Home, Modifiers::NONE);
    let w = ui.get::<EditField>(e).unwrap();
    let se = w.style_edit();
    // The caret is at the start of the text and must be visible: 0 <= x - scroll < width.
    let x = se.caret.x - se.scroll().x;
    assert_eq!(se.caret.pos, TextOfs::new(0, 0));
    assert!(
        (0..200).contains(&x),
        "caret x {} scroll {} (first fragment at {})",
        se.caret.x,
        se.scroll().x,
        se.blocks()[0].fragments[0].xpos
    );
}

#[test]
fn e7_adapt_to_content_shrinks_after_delete() {
    let mut ui = ui();
    let root = ui.root();
    let e = ui.add(root, EditField::default());
    ui.with_widget::<EditField, _>(e, |w, cx| {
        w.set_multiline(cx, true);
        w.set_wrapping(cx, false);
        w.set_adapt_to_content_size(cx, true);
    });
    ui.set_text(e, "a long first line of text\nshort");
    ui.update();
    let wide = ui.preferred_size(e, SizeConstraints::NONE).pref.w;
    ui.set_focus(e);
    // Remove the long line.
    press(&mut ui, SpecialKey::End, Modifiers::SHIFT);
    press(&mut ui, SpecialKey::Delete, Modifiers::NONE);
    ui.update();
    assert_eq!(ui.text(e), Some("\nshort"));
    let narrow = ui.preferred_size(e, SizeConstraints::NONE).pref.w;
    assert!(narrow < wide, "{narrow} < {wide}");
    assert_eq!(wide - narrow, (25 - 5) * CW);
}

#[test]
fn e8_adapt_to_content_shrinks_after_select_all_delete() {
    let mut ui = ui();
    let root = ui.root();
    let e = ui.add(root, EditField::default());
    ui.with_widget::<EditField, _>(e, |w, cx| {
        w.set_multiline(cx, true);
        w.set_wrapping(cx, false);
        w.set_adapt_to_content_size(cx, true);
    });
    ui.set_text(e, "s\na long middle line of text\ns");
    ui.update();
    let wide = ui.preferred_size(e, SizeConstraints::NONE).pref.w;
    ui.set_focus(e);
    ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, tbid!("selectall")));
    press(&mut ui, SpecialKey::Delete, Modifiers::NONE);
    ui.update();
    assert_eq!(ui.text(e), Some(""));
    let narrow = ui.preferred_size(e, SizeConstraints::NONE).pref.w;
    assert_eq!(wide - narrow, 26 * CW, "wide {wide}, after deleting everything {narrow}");
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// Random input to a real edit field. After every step: `text()` (the cache) is the
/// style edit's text, the last `Changed` carried it, and nothing panics. With
/// `check_scroll`, the scroll position is also within its limits and the same in the
/// style edit and the scroll info.
fn field_fuzz(seed: u64, multiline: bool, check_scroll: bool) -> Result<(), String> {
    let mut ui = ui();
    let e = field(&mut ui, multiline);
    let log = changes(&mut ui, e);
    ui.set_focus(e);
    let mut rng = Rng((seed * 7919) | 1);
    let chars = ["a", "b", " ", ".", "é", "€", "日", "😀", "e\u{301}", "<", ">"];
    let pastes =
        ["hello world", "one\ntwo\n", "x\r\ny", "日本語 text\nmore ö€😀\n", "a long line of text to scroll with"];
    let keys = [
        SpecialKey::Left,
        SpecialKey::Right,
        SpecialKey::Up,
        SpecialKey::Down,
        SpecialKey::Home,
        SpecialKey::End,
        SpecialKey::PageUp,
        SpecialKey::PageDown,
        SpecialKey::Enter,
        SpecialKey::Tab,
        SpecialKey::Delete,
        SpecialKey::Backspace,
    ];
    let cmds = ["cut", "copy", "paste", "delete", "undo", "redo", "selectall"];
    let mut trace: Vec<String> = Vec::new();
    for step in 0..250 {
        let n_changes = log.borrow().len();
        let op = rng.below(100);
        let desc;
        if op < 30 {
            let c = chars[rng.below(chars.len() as u64) as usize];
            desc = format!("type {c:?}");
            type_text(&mut ui, c);
        } else if op < 60 {
            let k = keys[rng.below(keys.len() as u64) as usize];
            let mut mods = Modifiers::NONE;
            if rng.below(3) == 0 {
                mods |= Modifiers::SHIFT;
            }
            if rng.below(4) == 0 {
                mods |= Modifiers::CTRL;
            }
            desc = format!("key {k:?} {mods:?}");
            press(&mut ui, k, mods);
        } else if op < 80 {
            let c = cmds[rng.below(cmds.len() as u64) as usize];
            if c == "paste" && rng.below(2) == 0 {
                ui.set_clipboard_text(pastes[rng.below(pastes.len() as u64) as usize]);
            }
            desc = format!("command {c}");
            ui.with_widget::<EditField, _>(e, |w, cx| w.command(cx, TbId::new(c)));
        } else if op < 88 {
            let p = Point::new(rng.below(220) as i32, rng.below(160) as i32);
            let q = Point::new(rng.below(220) as i32, rng.below(160) as i32);
            desc = format!("drag {p:?} {q:?}");
            ui.pointer_down(p, 1 + rng.below(2) as u32, Modifiers::NONE);
            ui.pointer_move(q, Modifiers::NONE);
            ui.pointer_up(q, Modifiers::NONE);
        } else if op < 92 {
            let d = rng.below(5) as i32 - 2;
            desc = format!("wheel {d}");
            ui.wheel(Point::new(50, 20), Point::new(0, d), Modifiers::NONE);
        } else if op < 95 {
            let t = pastes[rng.below(pastes.len() as u64) as usize];
            desc = format!("append {t:?}");
            ui.with_widget::<EditField, _>(e, |w, cx| w.append_text(cx, t, false));
        } else if op < 97 {
            let t = pastes[rng.below(pastes.len() as u64) as usize];
            desc = format!("set_text {t:?}");
            ui.set_text(e, t);
        } else if op < 99 {
            let on = !ui.get::<EditField>(e).unwrap().is_wrapping();
            desc = format!("wrapping {on}");
            ui.with_widget::<EditField, _>(e, |w, cx| w.set_wrapping(cx, on));
        } else {
            let pw = ui.get::<EditField>(e).unwrap().edit_type() != EditType::Password;
            desc = format!("password {pw}");
            let t = if pw { EditType::Password } else { EditType::Text };
            ui.with_widget::<EditField, _>(e, |w, cx| w.set_edit_type(cx, t));
        }
        ui.set_time(ui.now() + 20);
        ui.update();
        trace.push(desc.clone());
        let tail = || trace[trace.len().saturating_sub(10)..].join(", ");
        let w = ui.get::<EditField>(e).unwrap();
        let real = w.style_edit().text();
        if ui.text(e) != Some(real.as_str()) {
            return Err(format!(
                "step {step} {desc}: text() {:?} but the style edit has {real:?} [{}]",
                ui.text(e),
                tail()
            ));
        }
        if log.borrow().len() > n_changes && log.borrow().last() != Some(&real) {
            return Err(format!(
                "step {step} {desc}: Changed carried {:?}, text {real:?} [{}]",
                log.borrow().last(),
                tail()
            ));
        }
        if log.borrow().len() > n_changes + 1 && !desc.starts_with("type") {
            return Err(format!("step {step} {desc}: {} Changed events [{}]", log.borrow().len() - n_changes, tail()));
        }
        if check_scroll {
            let info = ui.scroll_info(e).unwrap();
            let s = w.style_edit().scroll();
            if s != info.pos || info.pos.x > info.max.x || info.pos.y > info.max.y || info.pos.x < 0 || info.pos.y < 0 {
                return Err(format!(
                    "step {step} {desc}: scroll: style edit {s:?}, info pos {:?} max {:?} [{}]",
                    info.pos,
                    info.max,
                    tail()
                ));
            }
        }
    }
    Ok(())
}

fn field_fuzz_many(multiline: bool, check_scroll: bool) {
    std::panic::set_hook(Box::new(|_| {}));
    let mut fails = Vec::new();
    for seed in 1..150u64 {
        match std::panic::catch_unwind(|| field_fuzz(seed, multiline, check_scroll)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => fails.push(format!("seed {seed}: {e}")),
            Err(p) => {
                let msg =
                    p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
                fails.push(format!("seed {seed}: PANIC {msg:?}"));
            }
        }
    }
    let _ = std::panic::take_hook();
    if !fails.is_empty() {
        let n = fails.len();
        fails.truncate(4);
        panic!("{n} of 149 seeds fail:\n{}", fails.join("\n"));
    }
}

#[test]
fn f1_field_fuzz_text_and_events_multiline() {
    field_fuzz_many(true, false);
}
#[test]
fn f2_field_fuzz_text_and_events_single_line() {
    field_fuzz_many(false, false);
}
#[test]
fn f3_field_fuzz_scroll_multiline() {
    field_fuzz_many(true, true);
}
#[test]
fn f4_field_fuzz_scroll_single_line() {
    field_fuzz_many(false, true);
}
