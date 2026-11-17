//! M7: the style edit by itself (no widgets): TB's test_tb_style_edit.cpp ported,
//! plus caret movement, selection, wrapping, styling and the effects it reports.
//!
//! The environment measures every character 10 wide and 20 high, so positions are
//! easy to read.

use turbo_ferret::edit::*;
use turbo_ferret::geom::{Color, Point, Rect};
use turbo_ferret::widgets::TextAlign;
use turbo_ferret::{FontDesc, Modifiers, SpecialKey};

const CW: i32 = 10;
const LH: i32 = 20;

#[derive(Default)]
struct Env {
    removed: Vec<Content>,
}

impl EditEnv for Env {
    fn string_width(&mut self, _font: FontDesc, text: &str) -> i32 {
        text.chars().count() as i32 * CW
    }
    fn font_height(&mut self, _font: FontDesc) -> i32 {
        LH
    }
    fn font_ascent(&mut self, _font: FontDesc) -> i32 {
        15
    }
    fn content_removed(&mut self, content: &Content) {
        self.removed.push(*content);
    }
}

/// A multiline style edit, 1000x1000, with Windows style breaks (as TB's test).
fn setup() -> (StyleEdit, Env) {
    let mut env = Env::default();
    let mut se = StyleEdit::new();
    se.set_multiline(true);
    se.set_wrapping(&mut env, true);
    se.set_layout_size(&mut env, 1000, 1000, false);
    se.set_windows_style_break(true);
    se.reformat(&mut env, true);
    se.take_effects();
    (se, env)
}

fn ins(se: &mut StyleEdit, env: &mut Env, text: &str) {
    se.insert_text(env, text, false, false);
}

fn key(se: &mut StyleEdit, env: &mut Env, k: SpecialKey, mods: Modifiers) -> bool {
    se.key_down(env, None, k, mods)
}

fn typed(se: &mut StyleEdit, env: &mut Env, text: &str) {
    for c in text.chars() {
        assert!(se.key_down(env, Some(c), SpecialKey::None, Modifiers::NONE));
    }
}

// ---- TB: test_tb_style_edit.cpp ----

#[test]
fn settext_singleline() {
    let (mut se, mut env) = setup();
    se.set_multiline(false);
    se.set_text(&mut env, "One\nTwo", CaretPos::End);
    assert_eq!(se.text(), "One");
}

#[test]
fn settext_multiline() {
    // Both unix and windows line endings are kept.
    let (mut se, mut env) = setup();
    se.set_text(&mut env, "One\nTwo", CaretPos::End);
    assert_eq!(se.text(), "One\nTwo");
    assert_eq!(se.blocks().len(), 2);
    se.set_text(&mut env, "One\r\nTwo", CaretPos::End);
    assert_eq!(se.text(), "One\r\nTwo");
    assert_eq!(se.blocks().len(), 2);
}

#[test]
fn settext_non_ascii_is_not_a_line_break() {
    // TB tested malformed UTF-8 here; a Rust string cannot hold it. Characters whose
    // encoding ends in 0x8A (like U+010A) must still not be taken for "\n" (0x0A).
    let (mut se, mut env) = setup();
    se.set_text(&mut env, "\u{010A}\u{208A}", CaretPos::End);
    assert_eq!(se.blocks().len(), 1);
    se.set_text(&mut env, "\u{10A}\nHello", CaretPos::End);
    assert_eq!(se.blocks().len(), 2);
}

#[test]
fn settext_undoredo_ins() {
    // Single characters typed in a row are merged up to the word boundary.
    let (mut se, mut env) = setup();
    for c in ["O", "N", "E", " ", "T", "W", "O"] {
        ins(&mut se, &mut env, c);
    }
    assert_eq!(se.text(), "ONE TWO");
    se.undo(&mut env);
    assert_eq!(se.text(), "ONE ");
    se.undo(&mut env);
    assert_eq!(se.text(), "");
    se.redo(&mut env);
    assert_eq!(se.text(), "ONE ");
    se.redo(&mut env);
    assert_eq!(se.text(), "ONE TWO");
    // Multi byte characters merge too. This replaces "TWO": a redone part stays
    // selected.
    for c in ["L", "Ö", "K"] {
        ins(&mut se, &mut env, c);
    }
    assert_eq!(se.text(), "ONE LÖK");
    se.undo(&mut env);
    assert_eq!(se.text(), "ONE ");
}

#[test]
fn settext_undoredo_ins_scattered() {
    let (mut se, mut env) = setup();
    ins(&mut se, &mut env, "AAA");
    se.set_caret_global_ofs(&mut env, 2, true, false);
    ins(&mut se, &mut env, ".");
    se.set_caret_global_ofs(&mut env, 1, true, false);
    ins(&mut se, &mut env, ".");
    assert_eq!(se.text(), "A.A.A");
    se.undo(&mut env);
    assert_eq!(se.text(), "AA.A");
    se.undo(&mut env);
    assert_eq!(se.text(), "AAA");
    se.redo(&mut env);
    assert_eq!(se.text(), "AA.A");
    se.redo(&mut env);
    assert_eq!(se.text(), "A.A.A");
}

#[test]
fn settext_undoredo_ins_multiline() {
    let (mut se, mut env) = setup();
    ins(&mut se, &mut env, "ONE\nTWO");
    assert_eq!(se.text(), "ONE\nTWO");
    se.undo(&mut env);
    assert_eq!(se.text(), "");
    assert_eq!(se.blocks().len(), 1);
    se.redo(&mut env);
    assert_eq!(se.text(), "ONE\nTWO");
}

#[test]
fn settext_undoredo_del() {
    let (mut se, mut env) = setup();
    ins(&mut se, &mut env, "ONE TWO");
    se.select_global(3, 7);
    se.remove_selection(&mut env);
    assert_eq!(se.text(), "ONE");
    se.undo(&mut env);
    assert_eq!(se.text(), "ONE TWO");
    se.redo(&mut env);
    assert_eq!(se.text(), "ONE");
}

#[test]
fn settext_undoredo_ins_linebreak_1() {
    let (mut se, mut env) = setup();
    ins(&mut se, &mut env, "ONETWO");
    se.set_caret_global_ofs(&mut env, 3, true, false);
    ins(&mut se, &mut env, "\n");
    assert_eq!(se.text(), "ONE\nTWO");
    se.undo(&mut env);
    assert_eq!(se.text(), "ONETWO");
    se.redo(&mut env);
    assert_eq!(se.text(), "ONE\nTWO");
}

#[test]
fn settext_undoredo_ins_linebreak_2() {
    // A line break at the end of a line that has none needs one more.
    let (mut se, mut env) = setup();
    se.insert_break(&mut env);
    assert_eq!(se.text(), "\r\n\r\n");
    se.insert_break(&mut env);
    assert_eq!(se.text(), "\r\n\r\n\r\n");
    se.undo(&mut env);
    assert_eq!(se.text(), "\r\n\r\n");
    se.undo(&mut env);
    assert_eq!(se.text(), "");
}

#[test]
fn settext_undoredo_ins_linebreak_3() {
    let (mut se, mut env) = setup();
    se.set_windows_style_break(false);
    se.insert_break(&mut env);
    assert_eq!(se.text(), "\n\n");
    se.insert_break(&mut env);
    assert_eq!(se.text(), "\n\n\n");
    se.undo(&mut env);
    assert_eq!(se.text(), "\n\n");
    se.undo(&mut env);
    assert_eq!(se.text(), "");
}

#[test]
fn settext_undoredo_ins_linebreak_4() {
    let (mut se, mut env) = setup();
    ins(&mut se, &mut env, "ONE");
    se.insert_break(&mut env);
    assert_eq!(se.text(), "ONE\r\n\r\n");
    se.undo(&mut env);
    assert_eq!(se.text(), "ONE");
    se.redo(&mut env);
    assert_eq!(se.text(), "ONE\r\n\r\n");
}

#[test]
fn settext_undoredo_bugfix1() {
    let (mut se, mut env) = setup();
    se.insert_break(&mut env);
    ins(&mut se, &mut env, "A");
    assert_eq!(se.text(), "\r\nA\r\n");
    assert_eq!(se.content_height(), LH * 2);
    key(&mut se, &mut env, SpecialKey::Backspace, Modifiers::NONE);
    assert_eq!(se.text(), "\r\n\r\n");
    assert_eq!(se.content_height(), LH * 2);
    se.undo(&mut env);
    assert_eq!(se.text(), "\r\nA\r\n");
    assert_eq!(se.content_height(), LH * 2);
    se.redo(&mut env);
    assert_eq!(se.text(), "\r\n\r\n");
    assert_eq!(se.content_height(), LH * 2);
}

#[test]
fn settext_insert_linebreaks_move() {
    let (mut se, mut env) = setup();
    ins(&mut se, &mut env, "Foo\n");
    ins(&mut se, &mut env, "Foo\n");
    ins(&mut se, &mut env, "Foo\n");
    assert_eq!(se.text(), "Foo\nFoo\nFoo\n");
}

#[test]
fn multiline_overflow_1() {
    // A long line without an allowed break position does not wrap.
    let (mut se, mut env) = setup();
    se.set_layout_size(&mut env, 200, 1000, false);
    let line = "this_is_a_long_line_that_should_not_wrap";
    se.set_text(&mut env, &format!("{line}\n{line}"), CaretPos::End);
    assert_eq!(se.content_height(), LH * 2);
    assert_eq!(se.content_width(), line.len() as i32 * CW);
}

// ---- more ----

#[test]
fn wrapping_and_indentation() {
    let (mut se, mut env) = setup();
    se.set_layout_size(&mut env, 100, 1000, false);
    // 10 characters fit on a line.
    se.set_text(&mut env, "aaaa bbbb cccc", CaretPos::Beginning);
    assert_eq!(se.blocks().len(), 1);
    assert_eq!(se.content_height(), LH * 2);
    let lines: Vec<i32> = se.blocks()[0].fragments.iter().map(|f| f.line_ypos).collect();
    assert_eq!(lines, [0, 0, 0, 0, LH], "the space before cccc ends the first line");
    // Wider: one line again.
    se.set_layout_size(&mut env, 200, 1000, false);
    assert_eq!(se.content_height(), LH);
    // Without wrapping the size does not matter.
    se.set_wrapping(&mut env, false);
    se.set_layout_size(&mut env, 50, 1000, false);
    assert_eq!(se.content_height(), LH);
    assert_eq!(se.content_width(), 140);

    // A wrapped list point continues under its text.
    se.set_wrapping(&mut env, true);
    se.set_layout_size(&mut env, 100, 1000, false);
    se.set_text(&mut env, "- aaaa bbbb", CaretPos::Beginning);
    let b = &se.blocks()[0];
    let last = b.fragments.last().unwrap();
    assert_eq!((b.str(last), last.line_ypos, last.xpos), ("bbbb", LH, 2 * CW));
}

#[test]
fn alignment() {
    let (mut se, mut env) = setup();
    se.set_layout_size(&mut env, 100, 100, false);
    se.set_text(&mut env, "abc", CaretPos::Beginning);
    assert_eq!(se.blocks()[0].fragments[0].xpos, 0);
    se.set_align(&mut env, TextAlign::Right);
    assert_eq!(se.blocks()[0].fragments[0].xpos, 70);
    se.set_align(&mut env, TextAlign::Center);
    assert_eq!(se.blocks()[0].fragments[0].xpos, 35);
    assert!(se.size_affects_layout());
}

#[test]
fn caret_moves_by_character_word_and_line() {
    let (mut se, mut env) = setup();
    se.set_text(&mut env, "one two\nÖl three", CaretPos::Beginning);
    let ctrl = Modifiers::CTRL;
    let none = Modifiers::NONE;
    assert_eq!(se.caret.pos, TextOfs::new(0, 0));
    assert!(!key(&mut se, &mut env, SpecialKey::Up, none), "nothing above the first line");
    key(&mut se, &mut env, SpecialKey::Right, none);
    assert_eq!((se.caret.pos, se.caret.x), (TextOfs::new(0, 1), CW));
    key(&mut se, &mut env, SpecialKey::Right, ctrl);
    assert_eq!(se.caret.pos, TextOfs::new(0, 4), "after the word and its space");
    key(&mut se, &mut env, SpecialKey::End, none);
    assert_eq!(se.caret.pos, TextOfs::new(0, 7), "before the line break");
    // Right at the end of a line goes to the next line.
    key(&mut se, &mut env, SpecialKey::Right, none);
    assert_eq!((se.caret.pos, se.caret.y), (TextOfs::new(1, 0), LH));
    // One step over a two byte character.
    key(&mut se, &mut env, SpecialKey::Right, none);
    assert_eq!((se.caret.pos, se.caret.x), (TextOfs::new(1, 2), CW));
    key(&mut se, &mut env, SpecialKey::Left, none);
    assert_eq!(se.caret.pos, TextOfs::new(1, 0));
    key(&mut se, &mut env, SpecialKey::Left, none);
    assert_eq!(se.caret.pos, TextOfs::new(0, 7));
    // Down keeps the wanted x; Up comes back to it.
    key(&mut se, &mut env, SpecialKey::Home, none);
    key(&mut se, &mut env, SpecialKey::Right, ctrl);
    assert_eq!(se.caret.x, 4 * CW);
    assert!(key(&mut se, &mut env, SpecialKey::Down, none));
    assert_eq!((se.caret.pos.block, se.caret.x), (1, 4 * CW));
    assert!(!key(&mut se, &mut env, SpecialKey::Down, none), "nothing below the last line");
    key(&mut se, &mut env, SpecialKey::Up, none);
    assert_eq!(se.caret.pos, TextOfs::new(0, 4));
    // Ctrl+End and Ctrl+Home.
    key(&mut se, &mut env, SpecialKey::End, ctrl);
    assert_eq!(se.caret.pos, TextOfs::new(1, "Öl three".len()));
    key(&mut se, &mut env, SpecialKey::Left, ctrl);
    assert_eq!(se.caret.pos, TextOfs::new(1, "Öl ".len()));
    key(&mut se, &mut env, SpecialKey::Home, ctrl);
    assert_eq!(se.caret.pos, TextOfs::new(0, 0));
    assert_eq!(se.caret_global_ofs(), 0);
    assert_eq!(se.from_global_ofs(10), Some(TextOfs::new(1, 2)));
    assert_eq!(se.from_global_ofs(9), Some(TextOfs::new(1, 0)), "never inside a character");
    assert_eq!(se.from_global_ofs(999), None);
}

#[test]
fn typing_deleting_and_selecting_with_shift() {
    let (mut se, mut env) = setup();
    typed(&mut se, &mut env, "Hello wörld");
    assert_eq!(se.text(), "Hello wörld");
    assert!(se.take_effects().changed);
    key(&mut se, &mut env, SpecialKey::Backspace, Modifiers::NONE);
    assert_eq!(se.text(), "Hello wörl");
    key(&mut se, &mut env, SpecialKey::Backspace, Modifiers::CTRL);
    assert_eq!(se.text(), "Hello ", "Ctrl+Backspace removes a word");
    key(&mut se, &mut env, SpecialKey::Home, Modifiers::NONE);
    key(&mut se, &mut env, SpecialKey::Delete, Modifiers::NONE);
    assert_eq!(se.text(), "ello ");
    // Moving the caret is not a change.
    se.take_effects();
    key(&mut se, &mut env, SpecialKey::Right, Modifiers::NONE);
    let fx = se.take_effects();
    assert!(!fx.changed && fx.invalidate && fx.blink == Some(Blink::Restart));

    // Shift selects; typing replaces the selection.
    key(&mut se, &mut env, SpecialKey::Right, Modifiers::SHIFT);
    key(&mut se, &mut env, SpecialKey::Right, Modifiers::SHIFT);
    assert_eq!(se.selected_text(), "ll");
    assert_eq!(se.selection(), Some((TextOfs::new(0, 1), TextOfs::new(0, 3))));
    key(&mut se, &mut env, SpecialKey::Left, Modifiers::SHIFT);
    assert_eq!(se.selected_text(), "l");
    typed(&mut se, &mut env, "X");
    assert_eq!(se.text(), "eXlo ");
    assert!(!se.is_selected());
    // An unshifted move drops the selection.
    se.select_all();
    assert_eq!(se.selected_text(), "eXlo ");
    key(&mut se, &mut env, SpecialKey::Left, Modifiers::NONE);
    assert!(!se.is_selected());

    // Keys with Ctrl or Cmd are not typed, and control characters are not text.
    assert!(!se.key_down(&mut env, Some('c'), SpecialKey::None, Modifiers::CTRL));
    assert!(!se.key_down(&mut env, Some('v'), SpecialKey::None, Modifiers::SUPER));
    assert!(!se.key_down(&mut env, Some('\u{8}'), SpecialKey::None, Modifiers::NONE));
    assert_eq!(se.text(), "eXlo ");
}

#[test]
fn enter_tab_and_read_only() {
    let (mut se, mut env) = setup();
    se.set_windows_style_break(false);
    typed(&mut se, &mut env, "ab");
    key(&mut se, &mut env, SpecialKey::Left, Modifiers::NONE);
    assert!(key(&mut se, &mut env, SpecialKey::Enter, Modifiers::NONE));
    assert_eq!(se.text(), "a\nb");
    assert_eq!(se.caret.pos, TextOfs::new(1, 0));
    assert!(key(&mut se, &mut env, SpecialKey::Tab, Modifiers::NONE));
    assert_eq!(se.text(), "a\n\tb");
    // A tab goes to the next tab stop (4 x).
    let tab = &se.blocks()[1].fragments[0];
    assert!(tab.is_tab() && tab.width == 4 * CW);

    // A single line field leaves Enter and Tab to others.
    let (mut one, mut env1) = setup();
    one.set_multiline(false);
    assert!(!key(&mut one, &mut env1, SpecialKey::Enter, Modifiers::NONE));
    assert!(!key(&mut one, &mut env1, SpecialKey::Tab, Modifiers::NONE));
    assert_eq!(one.text(), "");

    // Read only: the caret moves and text can be selected, nothing changes.
    se.set_read_only(true);
    assert!(!se.key_down(&mut env, Some('x'), SpecialKey::None, Modifiers::NONE));
    assert!(!key(&mut se, &mut env, SpecialKey::Backspace, Modifiers::NONE));
    assert!(key(&mut se, &mut env, SpecialKey::Left, Modifiers::SHIFT));
    assert_eq!(se.text(), "a\n\tb");
    assert!(se.is_selected());
    assert!(!se.can_undo() || se.text() == "a\n\tb");
}

#[test]
fn selection_across_blocks_and_clipboard_text() {
    let (mut se, mut env) = setup();
    se.set_text(&mut env, "one\ntwo\nthree\nfour", CaretPos::Beginning);
    assert_eq!(se.blocks().len(), 4);
    se.select(TextOfs::new(2, 2), TextOfs::new(0, 1));
    assert_eq!(se.selection(), Some((TextOfs::new(0, 1), TextOfs::new(2, 2))), "put in order");
    assert_eq!(se.copy().as_deref(), Some("ne\ntwo\nth"));
    assert_eq!(se.cut(&mut env).as_deref(), Some("ne\ntwo\nth"));
    assert_eq!(se.text(), "oree\nfour");
    assert_eq!(se.blocks().len(), 2);
    assert_eq!(se.caret.pos, TextOfs::new(0, 1));
    assert_eq!(se.blocks()[1].ypos, LH);
    se.paste(&mut env, "ne\ntwo\nth");
    assert_eq!(se.text(), "one\ntwo\nthree\nfour");
    assert_eq!(se.blocks().len(), 4);
    assert_eq!(se.caret.pos, TextOfs::new(2, 2));
    assert_eq!(se.content_height(), 4 * LH);
    se.undo(&mut env);
    se.undo(&mut env);
    assert_eq!(se.text(), "one\ntwo\nthree\nfour");
    // Nothing to copy without a selection, and never from a password field.
    se.select_nothing();
    assert_eq!(se.copy(), None);
    assert_eq!(se.cut(&mut env), None);
    se.select_all();
    se.set_password(&mut env, true);
    assert_eq!(se.copy(), None);
    se.set_password(&mut env, false);
    se.delete(&mut env);
    assert!(se.is_empty());
    assert_eq!(se.blocks().len(), 1);
}

#[test]
fn pointer_selects_by_character_and_by_word() {
    let (mut se, mut env) = setup();
    se.set_text(&mut env, "one two three", CaretPos::Beginning);
    // Press in "two" and drag into "three".
    assert!(se.mouse_down(&mut env, Point::new(5 * CW + 2, 5), 1, false));
    assert!(se.is_selecting());
    assert!(!se.is_selected());
    assert_eq!(se.caret.pos.ofs, 5);
    assert!(se.mouse_move(&mut env, Point::new(10 * CW + 1, 5)));
    assert_eq!(se.selected_text(), "wo th");
    // Keys are ignored while selecting.
    assert!(!key(&mut se, &mut env, SpecialKey::Left, Modifiers::NONE));
    se.mouse_up(&mut env, false, false);
    assert!(!se.is_selecting());
    assert!(!se.mouse_move(&mut env, Point::new(0, 5)), "no selection in progress");
    assert_eq!(se.selected_text(), "wo th");

    // A double click selects the word (with the space after it); dragging extends
    // by words.
    se.mouse_down(&mut env, Point::new(5 * CW, 5), 2, false);
    assert_eq!(se.selected_text(), "two ");
    se.mouse_move(&mut env, Point::new(9 * CW, 5));
    assert_eq!(se.selected_text(), "two three");
    se.mouse_up(&mut env, false, false);

    // A touch places the caret on release, unless it became a pan.
    se.mouse_down(&mut env, Point::new(2 * CW + 1, 5), 1, true);
    assert!(!se.is_selecting());
    se.mouse_up(&mut env, true, true);
    assert!(se.is_selected(), "a cancelled touch changes nothing");
    se.mouse_down(&mut env, Point::new(2 * CW + 1, 5), 1, true);
    se.mouse_up(&mut env, true, false);
    assert!(!se.is_selected());
    assert_eq!(se.caret.pos.ofs, 2);
}

#[test]
fn scrolling_follows_the_caret() {
    let (mut se, mut env) = setup();
    se.set_wrapping(&mut env, false);
    se.set_layout_size(&mut env, 100, 40, false);
    se.take_effects();
    typed(&mut se, &mut env, "0123456789abcdef");
    // Scrolled to the end of the text (as in TB, not past it for the caret's width).
    assert_eq!(se.scroll(), Point::new(160 - 100, 0));
    let fx = se.take_effects();
    assert!(fx.scrolled && fx.update_scrollbars);
    assert_eq!(se.overflow_x(), 60);
    key(&mut se, &mut env, SpecialKey::Home, Modifiers::NONE);
    assert_eq!(se.scroll(), Point::ZERO);
    // Four lines in two lines of height.
    for _ in 0..3 {
        key(&mut se, &mut env, SpecialKey::End, Modifiers::NONE);
        key(&mut se, &mut env, SpecialKey::Enter, Modifiers::NONE);
    }
    assert_eq!(se.content_height(), 4 * LH);
    assert_eq!(se.overflow_y(), 2 * LH);
    assert_eq!(se.scroll().y, 2 * LH);
    // Ctrl+Up scrolls a line without moving the caret (as long as the caret stays
    // in view).
    key(&mut se, &mut env, SpecialKey::Up, Modifiers::NONE);
    assert_eq!(se.scroll().y, 2 * LH);
    let pos = se.caret.pos;
    key(&mut se, &mut env, SpecialKey::Up, Modifiers::CTRL);
    assert_eq!((se.scroll().y, se.caret.pos), (LH, pos));
    se.set_scroll_pos(0, 1000);
    assert_eq!(se.scroll().y, 2 * LH, "clamped");
    // A single line field never scrolls vertically.
    se.set_multiline(false);
    se.set_scroll_pos(0, 20);
    assert_eq!(se.scroll().y, 0);
}

#[test]
fn password_measures_and_hides() {
    let (mut se, mut env) = setup();
    se.set_text(&mut env, "söcret", CaretPos::End);
    se.set_password(&mut env, true);
    // One bullet per character, however many bytes.
    assert_eq!(se.blocks()[0].fragments[0].width, 6 * CW);
    // Word movement is off: a word would give the text away.
    key(&mut se, &mut env, SpecialKey::Left, Modifiers::CTRL);
    assert_eq!(se.caret.pos.ofs, "söcre".len());
}

/// Records what paint draws.
#[derive(Default)]
struct Painter {
    strings: Vec<(Point, Color, String)>,
    fills: Vec<(Rect, Color)>,
    selections: Vec<Rect>,
    carets: Vec<Rect>,
}

impl EditPainter for Painter {
    fn string_width(&mut self, _font: FontDesc, text: &str) -> i32 {
        text.chars().count() as i32 * CW
    }
    fn draw_string(&mut self, pos: Point, _font: FontDesc, color: Color, text: &str) {
        self.strings.push((pos, color, text.to_string()));
    }
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.fills.push((rect, color));
    }
    fn draw_selection(&mut self, rect: Rect) {
        self.selections.push(rect);
    }
    fn draw_caret(&mut self, rect: Rect) {
        self.carets.push(rect);
    }
}

#[test]
fn paint_text_selection_and_caret() {
    let (mut se, mut env) = setup();
    se.set_text(&mut env, "one two\nthree", CaretPos::Beginning);
    se.select_global(2, 10);
    se.focus(true);
    let white = Color::rgb(255, 255, 255);
    let mut p = Painter::default();
    se.paint(&mut p, Rect::new(0, 0, 1000, 1000), white);
    // Words are drawn; spaces and line breaks are not.
    let texts: Vec<&str> = p.strings.iter().map(|s| s.2.as_str()).collect();
    assert_eq!(texts, ["one", "two", "three"]);
    assert_eq!(p.strings[1].0, Point::new(4 * CW, 0));
    assert_eq!(p.strings[2].0, Point::new(0, LH));
    // The selection: "e", " ", "two", the line break (no width, as in TB), "th".
    assert_eq!(
        p.selections,
        [
            Rect::new(2 * CW, 0, CW, LH),
            Rect::new(3 * CW, 0, CW, LH),
            Rect::new(4 * CW, 0, 3 * CW, LH),
            Rect::new(7 * CW, 0, 0, LH),
            Rect::new(0, LH, 2 * CW, LH),
        ]
    );
    assert_eq!(p.carets, [Rect::new(0, 0, 2, LH)]);
    // Without focus there is no caret. Scrolling moves everything.
    se.focus(false);
    se.select_nothing();
    se.set_layout_size(&mut env, 1000, LH, false);
    se.set_scroll_pos(0, LH);
    let mut p = Painter::default();
    se.paint(&mut p, Rect::new(0, 0, 1000, LH), white);
    assert!(p.carets.is_empty());
    assert_eq!(p.strings.last().unwrap().0, Point::new(0, 0), "the second line is at the top now");
    // Ctrl+8 shows whitespace.
    se.key_down(&mut env, Some('8'), SpecialKey::None, Modifiers::CTRL);
    se.set_scroll_pos(0, 0);
    let mut p = Painter::default();
    se.paint(&mut p, Rect::new(0, 0, 1000, LH), white);
    let texts: Vec<&str> = p.strings.iter().map(|s| s.2.as_str()).collect();
    assert_eq!(&texts[..4], ["one", "·", "two", "¶"]);
}

#[test]
fn styling_tags_become_content() {
    let (mut se, mut env) = setup();
    se.set_styling(true);
    let red = Color::rgb(255, 0, 0);
    se.set_text(&mut env, "a <color #ff0000>red</color> <u>line</u>\n<hr>\nplain <b>", CaretPos::Beginning);
    let kinds: Vec<Option<Content>> = se.blocks()[0].fragments.iter().map(|f| f.content).collect();
    assert_eq!(kinds[2], Some(Content::TextColor(red)));
    assert_eq!(kinds[4], Some(Content::StylePop));
    assert_eq!(kinds[6], Some(Content::Underline));
    // Tags take no space; the horizontal line is as wide as the layout.
    let b = &se.blocks()[0];
    assert_eq!(b.fragments[2].width, 0);
    assert_eq!(b.fragments[3].xpos, 2 * CW, "\"red\" follows \"a \" directly");
    let hr = &se.blocks()[1].fragments[0];
    assert_eq!((hr.content, hr.width, hr.height), (Some(Content::Hr { width_percent: 100, height: 2 }), 1000, 2));
    // An unknown tag stays text.
    let last = se.blocks()[2].fragments.last().unwrap();
    assert_eq!((last.content, se.blocks()[2].str(last)), (None, "<b>"));

    let white = Color::rgb(255, 255, 255);
    let mut p = Painter::default();
    se.paint(&mut p, Rect::new(0, 0, 1000, 1000), white);
    let colored: Vec<(&str, Color)> = p.strings.iter().map(|s| (s.2.as_str(), s.1)).collect();
    assert_eq!(&colored[..3], [("a", white), ("red", red), ("line", white)]);
    // The underline under "line", and the horizontal line.
    assert_eq!(p.fills.len(), 2);
    assert_eq!((p.fills[0].0.w, p.fills[0].0.y), (4 * CW, 15 + 1));
    // The line sits on the text baseline of its line (15), 2 high.
    assert_eq!(p.fills[1].0, Rect::new(0, LH + 13, 1000, 2));

    // The text itself keeps the tags; without styling they are plain text.
    assert!(se.text().contains("<color #ff0000>"));
    se.set_styling(false);
    se.reformat(&mut env, true);
    assert!(se.blocks()[0].fragments.iter().all(|f| f.content.is_none()));
    assert!(!env.removed.is_empty(), "the environment heard that the content went away");
}

#[test]
fn clear_and_empty_text() {
    let (mut se, mut env) = setup();
    assert!(se.is_empty());
    se.set_text(&mut env, "x\ny", CaretPos::End);
    assert!(se.take_effects().changed);
    assert!(se.can_undo() || !se.is_empty());
    se.clear(&mut env);
    assert!(se.is_empty() && !se.can_undo() && !se.can_redo());
    assert_eq!((se.blocks().len(), se.content_height(), se.caret.pos), (1, LH, TextOfs::new(0, 0)));
    se.append_text(&mut env, "a", false);
    se.append_text(&mut env, "b", true);
    assert_eq!(se.text(), "ab");
    assert!(!se.can_undo(), "append with clear_undo_redo forgets the history");
}
