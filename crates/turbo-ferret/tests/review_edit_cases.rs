//! `StyleEdit` regressions found in review (2026-11): one test per bug, each asserting
//! what a user expects.

use turbo_ferret::edit::*;
use turbo_ferret::geom::Point;
use turbo_ferret::{FontDesc, Modifiers, SpecialKey};

const CW: i32 = 10;
const LH: i32 = 20;

#[derive(Default)]
struct Env {
    moved: Vec<(Content, Point)>,
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
    fn content_moved(&mut self, content: &Content, pos: Point) {
        self.moved.push((*content, pos));
    }
}

fn setup(wrapping: bool) -> (StyleEdit, Env) {
    let mut env = Env::default();
    let mut se = StyleEdit::new();
    se.set_multiline(true);
    se.set_wrapping(&mut env, wrapping);
    se.set_layout_size(&mut env, 400, 200, false);
    se.set_windows_style_break(false);
    se.reformat(&mut env, true);
    se.take_effects();
    (se, env)
}

fn key(se: &mut StyleEdit, env: &mut Env, k: SpecialKey) {
    se.key_down(env, None, k, Modifiers::NONE);
}

fn typed(se: &mut StyleEdit, env: &mut Env, text: &str) {
    for c in text.chars() {
        se.key_down(env, Some(c), SpecialKey::None, Modifiers::NONE);
    }
}

fn block_texts(se: &StyleEdit) -> Vec<String> {
    se.blocks().iter().map(|b| b.text.clone()).collect()
}

fn ypos(se: &StyleEdit) -> Vec<(i32, i32)> {
    se.blocks().iter().map(|b| (b.ypos, b.height)).collect()
}

#[test]
fn t1_pasting_lines_twice_keeps_their_order() {
    let (mut se, mut env) = setup(true);
    se.paste(&mut env, "A\nB\n");
    se.paste(&mut env, "A\nB\n");
    assert_eq!(se.text(), "A\nB\nA\nB\n");
}

#[test]
fn t2_typing_after_pasting_a_line_stays_on_one_line() {
    let (mut se, mut env) = setup(true);
    se.set_text(&mut env, "ab", CaretPos::Beginning);
    key(&mut se, &mut env, SpecialKey::Right);
    se.paste(&mut env, "x\n");
    typed(&mut se, &mut env, "y");
    assert_eq!(se.text(), "ax\nyb");
    // "yb" is one line.
    assert_eq!(block_texts(&se), ["ax\n", "yb"]);
}

#[test]
fn t3_typing_after_undoing_a_line_join_stays_on_one_line() {
    let (mut se, mut env) = setup(true);
    se.set_text(&mut env, "a\nb", CaretPos::Beginning);
    key(&mut se, &mut env, SpecialKey::Down);
    assert_eq!(se.caret.pos, TextOfs::new(1, 0));
    key(&mut se, &mut env, SpecialKey::Backspace);
    assert_eq!(se.text(), "ab");
    se.undo(&mut env);
    assert_eq!(se.text(), "a\nb");
    typed(&mut se, &mut env, "X");
    assert_eq!(se.text(), "a\nXb");
    assert_eq!(block_texts(&se), ["a\n", "Xb"]);
}

#[test]
fn t4_redo_after_a_trailing_line_break_restores_the_text() {
    let (mut se, mut env) = setup(true);
    se.paste(&mut env, "line\n");
    typed(&mut se, &mut env, "abc");
    assert_eq!(se.text(), "line\nabc");
    se.undo(&mut env);
    assert_eq!(se.text(), "line\n");
    se.redo(&mut env);
    assert_eq!(se.text(), "line\nabc", "redo");
    se.undo(&mut env);
    assert_eq!(se.text(), "line\n", "second undo");
}

#[test]
fn t4b_redo_after_a_trailing_line_break_restores_the_text() {
    let (mut se, mut env) = setup(true);
    se.paste(&mut env, "line\n");
    typed(&mut se, &mut env, "abc");
    assert_eq!(se.text(), "line\nabc");
    se.undo(&mut env);
    se.undo(&mut env);
    assert_eq!(se.text(), "");
    se.redo(&mut env);
    assert_eq!(se.text(), "line\n");
    se.redo(&mut env);
    assert_eq!(se.text(), "line\nabc", "second redo");
}

#[test]
fn t5_undoing_a_paste_at_a_line_start_moves_the_lines_below_back() {
    let (mut se, mut env) = setup(true);
    se.set_text(&mut env, "a\nb\nc", CaretPos::Beginning);
    key(&mut se, &mut env, SpecialKey::Down);
    se.paste(&mut env, "one\ntwo");
    assert_eq!(se.text(), "a\none\ntwob\nc");
    se.undo(&mut env);
    assert_eq!(se.text(), "a\nb\nc");
    assert_eq!(ypos(&se), [(0, LH), (LH, LH), (2 * LH, LH)]);
    assert_eq!(se.content_height(), 3 * LH);
}

#[test]
fn t6_undo_keeps_the_caret_on_its_line_and_redone_text_is_replaced_in_place() {
    let (mut se, mut env) = setup(true);
    typed(&mut se, &mut env, "a");
    key(&mut se, &mut env, SpecialKey::Enter);
    typed(&mut se, &mut env, "hello");
    assert_eq!(se.text(), "a\nhello\n");
    se.undo(&mut env);
    assert_eq!(se.text(), "a\n\n");
    let after_undo = se.caret.pos;
    se.redo(&mut env);
    assert_eq!(se.text(), "a\nhello\n");
    assert_eq!(se.selected_text(), "hello");
    // Typing replaces the selected "hello".
    typed(&mut se, &mut env, "x");
    assert_eq!(se.text(), "a\nx\n", "typing over the redone text");
    assert_eq!(after_undo, TextOfs::new(1, 0), "caret after undo");
}

#[test]
fn t8_end_key_in_styled_text_does_not_put_the_caret_inside_a_tag() {
    let (mut se, mut env) = setup(true);
    se.set_styling(true);
    se.set_text(&mut env, "<u>abc</u>", CaretPos::Beginning);
    key(&mut se, &mut env, SpecialKey::End);
    typed(&mut se, &mut env, "d");
    assert_eq!(se.text(), "<u>abc</u>d");
}

#[test]
fn t8b_click_right_of_an_embedded_fragment() {
    let (mut se, mut env) = setup(true);
    se.set_styling(true);
    se.set_text(&mut env, "ab <hr>", CaretPos::Beginning);
    // Right half of the line (the <hr> takes the layout width).
    assert_eq!(se.blocks()[0].fragments.last().unwrap().line_ypos, LH);
    se.mouse_down(&mut env, Point::new(390, LH + 1), 1, false);
    se.mouse_up(&mut env, false, false);
    let ofs = se.caret.pos.ofs;
    assert!(ofs == 3 || ofs == 7, "caret at {ofs}, inside the tag \"<hr>\" (3..7)");
}

#[test]
fn t9_lone_cr_paste_keeps_the_order() {
    let (mut se, mut env) = setup(true);
    se.set_text(&mut env, "x\n", CaretPos::Beginning);
    key(&mut se, &mut env, SpecialKey::End);
    se.paste(&mut env, "a\rb");
    // The order is kept. The lone carriage return became a line feed: in front of
    // the existing one it would otherwise read as one "\r\n" break.
    assert_eq!(se.text(), "xa\nb\n");
}

#[test]
fn t10_select_all_on_empty_text_selects_nothing() {
    let (mut se, mut env) = setup(true);
    se.select_all();
    assert!(!se.is_selected(), "selection {:?}", se.selection());
    assert_eq!(se.cut(&mut env), None);
    assert!(!se.can_undo());
}

#[test]
fn t10b_cut_and_delete_on_empty_text_do_nothing() {
    let (mut se, mut env) = setup(true);
    se.select_all();
    assert_eq!(se.cut(&mut env), None, "cut hands an empty string to the clipboard");
    se.select_all();
    key(&mut se, &mut env, SpecialKey::Delete);
    assert!(!se.can_undo(), "an empty removal was recorded for undo");
}

#[test]
fn t11_content_is_placed_at_its_block() {
    let (mut se, mut env) = setup(false);
    se.set_styling(true);
    se.set_text(&mut env, "a\nb\n<hr>", CaretPos::Beginning);
    let last = env.moved.iter().rev().find(|(c, _)| matches!(c, Content::Hr { .. })).copied();
    // The rule is the only thing on the third line: at the top of its block.
    assert_eq!(last.map(|(_, p)| p.y), Some(2 * LH), "after set_text: {:?}", env.moved);
    assert_eq!(se.blocks()[2].ypos, 2 * LH);
}

#[test]
fn t11b_content_follows_when_lines_are_added_above() {
    let (mut se, mut env) = setup(false);
    se.set_styling(true);
    se.set_text(&mut env, "a\nb\n<hr>", CaretPos::Beginning);
    se.reformat(&mut env, false);
    let y = |env: &Env| env.moved.iter().rev().find(|(c, _)| matches!(c, Content::Hr { .. })).map(|(_, p)| p.y);
    let before = y(&env).unwrap();
    key(&mut se, &mut env, SpecialKey::Enter);
    assert_eq!(se.blocks()[3].ypos, 3 * LH);
    assert_eq!(y(&env), Some(before + LH));
}

#[test]
fn t12_shift_right_after_pasted_line_selects_nothing_visible() {
    // Paste a line, then Shift+Right: an empty selection that still counts.
    let (mut se, mut env) = setup(true);
    se.set_text(&mut env, "ab", CaretPos::Beginning);
    key(&mut se, &mut env, SpecialKey::Right);
    se.paste(&mut env, "x\n");
    se.key_down(&mut env, None, SpecialKey::Right, Modifiers::SHIFT);
    assert_eq!(se.selected_text(), "b");
}

#[test]
fn t13_tab_after_wrap_is_measured_at_its_own_position() {
    // 12 characters fit. "aaaaaaaaaa \tb": the tab wraps to line 2 with "b".
    let mut env = Env::default();
    let mut se = StyleEdit::new();
    se.set_multiline(true);
    se.set_wrapping(&mut env, true);
    se.set_layout_size(&mut env, 110, 200, false);
    se.set_text(&mut env, "aaaaaaaaa \tb", CaretPos::Beginning);
    let b = &se.blocks()[0];
    let tab = b.fragments.iter().find(|f| f.is_tab()).unwrap();
    let after = b.fragments.last().unwrap();
    // On line 2 at x 0 a tab is 4 * 10 wide.
    assert_eq!((tab.line_ypos, tab.xpos, tab.width, after.xpos), (LH, 0, 40, 40), "{:#?}", b.fragments);
}

#[test]
fn t14_content_width_shrinks_when_the_widest_line_is_removed_whole() {
    let (mut se, mut env) = setup(false);
    se.set_text(&mut env, "s\nLONG LONG LONG LONG\ns", CaretPos::Beginning);
    assert_eq!(se.content_width(), 19 * CW);
    se.select_all();
    key(&mut se, &mut env, SpecialKey::Delete);
    assert_eq!(se.text(), "");
    assert_eq!(se.content_width(), 0);
}

#[test]
fn t15_caret_follows_a_change_of_alignment() {
    let (mut se, mut env) = setup(true);
    se.set_text(&mut env, "abc", CaretPos::End);
    assert_eq!(se.caret.x, 3 * CW);
    se.set_align(&mut env, turbo_ferret::widgets::TextAlign::Right);
    assert_eq!(se.blocks()[0].fragments[0].xpos, 400 - 3 * CW);
    assert_eq!(se.caret.x, 400);
}
