//! Randomized test of `StyleEdit` against a plain `String` model, from the review
//! (2026-11): random edits, caret moves, undo and redo must agree with the model.

use turbo_ferret::edit::*;
use turbo_ferret::geom::{Color, Point, Rect};
use turbo_ferret::{FontDesc, Modifiers, SpecialKey};

const CW: i32 = 10;
const LH: i32 = 20;

#[derive(Default)]
struct Env {
    removed: Vec<Content>,
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
    fn content_removed(&mut self, content: &Content) {
        self.removed.push(*content);
    }
    fn content_moved(&mut self, content: &Content, pos: Point) {
        self.moved.push((*content, pos));
    }
}

struct NullPainter;
impl EditPainter for NullPainter {
    fn string_width(&mut self, _font: FontDesc, text: &str) -> i32 {
        text.chars().count() as i32 * CW
    }
    fn draw_string(&mut self, _pos: Point, _font: FontDesc, _color: Color, _text: &str) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn draw_selection(&mut self, _rect: Rect) {}
    fn draw_caret(&mut self, _rect: Rect) {}
}

fn setup(multiline: bool, wrapping: bool, w: i32, h: i32) -> (StyleEdit, Env) {
    let mut env = Env::default();
    let mut se = StyleEdit::new();
    se.set_multiline(multiline);
    se.set_wrapping(&mut env, wrapping);
    se.set_layout_size(&mut env, w, h, false);
    se.set_windows_style_break(false);
    se.reformat(&mut env, true);
    se.take_effects();
    (se, env)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

/// Structural invariants. Returns a description of the first violated one.
fn check(se: &StyleEdit, strict_blocks: bool) -> Result<(), String> {
    let blocks = se.blocks();
    if blocks.is_empty() {
        return Err("no blocks".into());
    }
    let mut y = 0;
    for (i, b) in blocks.iter().enumerate() {
        // Fragments tile the text.
        let mut ofs = 0;
        for f in &b.fragments {
            if f.ofs != ofs {
                return Err(format!("fragment gap: block {i}: fragment at {} expected {}", f.ofs, ofs));
            }
            if !b.text.is_char_boundary(f.ofs) || !b.text.is_char_boundary(f.ofs + f.len) {
                return Err(format!("fragment boundary: block {i}: fragment not on char boundary"));
            }
            ofs += f.len;
        }
        if ofs != b.text.len() {
            return Err(format!("fragment cover: block {i}: {} of {} ({:?})", ofs, b.text.len(), b.text));
        }
        if strict_blocks {
            let t = b.text.as_str();
            let body = t.strip_suffix("\r\n").or_else(|| t.strip_suffix('\n')).or_else(|| t.strip_suffix('\r'));
            let has_break = body.is_some();
            let body = body.unwrap_or(t);
            if body.contains(['\n', '\r']) {
                return Err(format!("inner break in block {i}: {:?}", t));
            }
            if !has_break && i + 1 < blocks.len() {
                return Err(format!("no ending break: block {i} of {}: {:?}", blocks.len(), t));
            }
        }
        if b.ypos != y {
            return Err(format!("ypos wrong: block {i}: ypos {} expected {}", b.ypos, y));
        }
        y += b.height;
    }
    let valid = |p: TextOfs| p.block < blocks.len() && blocks[p.block].text.is_char_boundary(p.ofs);
    if !valid(se.caret.pos) {
        return Err(format!("caret invalid: {:?}", se.caret.pos));
    }
    if let Some((a, b)) = se.selection() {
        if !valid(a) || !valid(b) || a > b {
            return Err(format!("selection invalid: {:?} {:?}", a, b));
        }
    }
    let joined: String = blocks.iter().map(|b| b.text.as_str()).collect();
    if joined != se.text() {
        return Err("text() differs from blocks".into());
    }
    if se.content_height() != y {
        return Err(format!("content height {} expected {}", se.content_height(), y));
    }
    Ok(())
}

const CHARS: &[&str] = &[
    "a", "b", "Z", " ", " ", ".", "-", "(", "é", "ö", "€", "日", "😀", "e\u{301}", "\u{10A}", "<", ">", "/", "u", "'",
    "\"",
];
const PASTES: &[&str] = &[
    "hello world",
    "one\ntwo",
    "one\r\ntwo\r\n",
    "\n",
    "\r\n",
    "x\n",
    "\n\n\n",
    "日本語 text\nmore ö€😀\n",
    "<u>under</u>",
    "<color #ff0000>red</color>",
    "<hr>",
    "<widget TBButton: text: 'x'>",
    "<not closed",
    "<é>",
    "</",
    "<color é>",
    "tab\there",
    "- list item that is long enough to wrap around the layout width for sure",
];

#[derive(Clone, Debug)]
enum Op {
    Type(&'static str),
    Key(SpecialKey, bool, bool),
    Del(bool, bool),
    Enter,
    Tab,
    Paste(&'static str),
    Cut,
    Undo,
    Redo,
    Mouse(Point, Point, u32),
    Resize(i32),
    SelectAll,
    Delete,
    Password,
    Wrapping,
}

fn gen_ops(seed: u64, steps: usize) -> Vec<Op> {
    let mut rng = Rng(seed | 1);
    let keys = [
        SpecialKey::Left,
        SpecialKey::Right,
        SpecialKey::Up,
        SpecialKey::Down,
        SpecialKey::Home,
        SpecialKey::End,
        SpecialKey::PageUp,
        SpecialKey::PageDown,
    ];
    (0..steps)
        .map(|_| {
            let op = rng.below(100);
            if op < 25 {
                Op::Type(rng.pick(CHARS))
            } else if op < 45 {
                Op::Key(keys[rng.below(keys.len() as u64) as usize], rng.below(3) == 0, rng.below(3) == 0)
            } else if op < 55 {
                Op::Del(rng.below(2) == 0, rng.below(4) == 0)
            } else if op < 62 {
                Op::Enter
            } else if op < 65 {
                Op::Tab
            } else if op < 75 {
                Op::Paste(rng.pick(PASTES))
            } else if op < 80 {
                Op::Cut
            } else if op < 86 {
                Op::Undo
            } else if op < 90 {
                Op::Redo
            } else if op < 94 {
                let p = Point::new(rng.below(160) as i32 - 20, rng.below(100) as i32 - 20);
                let q = Point::new(rng.below(160) as i32 - 20, rng.below(100) as i32 - 20);
                Op::Mouse(p, q, 1 + rng.below(2) as u32)
            } else if op < 96 {
                Op::Resize(30 + rng.below(200) as i32)
            } else if op < 97 {
                Op::SelectAll
            } else if op < 98 {
                Op::Delete
            } else if op < 99 {
                Op::Password
            } else {
                Op::Wrapping
            }
        })
        .collect()
}

#[derive(Copy, Clone)]
struct Mode {
    multiline: bool,
    wrapping: bool,
    styling: bool,
    /// Check that every block but the last ends with a line break.
    strict: bool,
    /// Work around the "caret after the line break" state after every step.
    normalize: bool,
    /// After every step, undo + redo must give the same text.
    roundtrip: bool,
    /// The caret's x/y and the content width must be up to date after every step.
    geometry: bool,
}

fn apply(ops: &[Op], m: Mode) -> Result<(), String> {
    let (mut se, mut env) = setup(m.multiline, m.wrapping, 120, 60);
    se.set_styling(m.styling);
    for (step, op) in ops.iter().enumerate() {
        let before = se.text();
        let caret = se.caret_global_ofs();
        let sel = se.selection().map(|(a, b)| (se.global_ofs(a), se.global_ofs(b)));
        let mut expect: Option<String> = None;
        let without_sel = |t: &str| match sel {
            Some((a, b)) => (format!("{}{}", &t[..a], &t[b..]), a),
            None => (t.to_string(), caret),
        };
        let mods = |shift: bool, ctrl: bool| {
            let mut mods = Modifiers::NONE;
            if shift {
                mods |= Modifiers::SHIFT;
            }
            if ctrl {
                mods |= Modifiers::CTRL;
            }
            mods
        };
        match op {
            Op::Type(c) => {
                let (mut t, at) = without_sel(&before);
                t.insert_str(at, c);
                expect = Some(t);
                for ch in c.chars() {
                    se.key_down(&mut env, Some(ch), SpecialKey::None, Modifiers::NONE);
                }
            }
            Op::Key(k, shift, ctrl) => {
                expect = Some(before.clone());
                se.key_down(&mut env, None, *k, mods(*shift, *ctrl));
            }
            Op::Del(back, ctrl) => {
                let k = if *back { SpecialKey::Backspace } else { SpecialKey::Delete };
                if sel.is_some() {
                    expect = Some(without_sel(&before).0);
                }
                se.key_down(&mut env, None, k, mods(false, *ctrl));
                if se.text().len() > before.len() {
                    return Err(format!("step {step}: {op:?}: grew the text"));
                }
            }
            Op::Enter => {
                se.key_down(&mut env, None, SpecialKey::Enter, Modifiers::NONE);
            }
            Op::Tab => {
                if m.multiline {
                    let (mut t, at) = without_sel(&before);
                    t.insert(at, '\t');
                    expect = Some(t);
                }
                se.key_down(&mut env, None, SpecialKey::Tab, Modifiers::NONE);
            }
            Op::Paste(p) => {
                if m.multiline {
                    let (mut t, at) = without_sel(&before);
                    t.insert_str(at, p);
                    expect = Some(t);
                }
                se.paste(&mut env, p);
            }
            Op::Cut => {
                let cut = se.cut(&mut env);
                if se.is_password() {
                    expect = Some(before.clone());
                } else if let Some((a, b)) = sel {
                    if cut.as_deref() != Some(&before[a..b]) {
                        return Err(format!("step {step}: {op:?}: cut returned {cut:?}"));
                    }
                    expect = Some(without_sel(&before).0);
                } else {
                    expect = Some(before.clone());
                }
            }
            Op::Undo => se.undo(&mut env),
            Op::Redo => se.redo(&mut env),
            Op::Mouse(p, q, clicks) => {
                expect = Some(before.clone());
                se.mouse_down(&mut env, *p, *clicks, false);
                se.mouse_move(&mut env, *q);
                se.mouse_up(&mut env, false, false);
            }
            Op::Resize(w) => {
                expect = Some(before.clone());
                se.set_layout_size(&mut env, *w, 60, false);
            }
            Op::SelectAll => {
                expect = Some(before.clone());
                se.select_all();
            }
            Op::Delete => {
                expect = Some(without_sel(&before).0);
                se.delete(&mut env);
            }
            Op::Password => {
                expect = Some(before.clone());
                let on = !se.is_password();
                se.set_password(&mut env, on);
            }
            Op::Wrapping => {
                expect = Some(before.clone());
                let on = !se.is_wrapping();
                se.set_wrapping(&mut env, on);
            }
        }
        let ctx = format!("(caret {caret}, sel {sel:?}, before {before:?})");
        if let Some(e) = expect {
            if e != se.text() {
                return Err(format!("step {step}: {op:?}: text mismatch: {:?}, expected {:?} {ctx}", se.text(), e));
            }
        }
        if let Err(e) = check(&se, m.strict) {
            return Err(format!("step {step}: {op:?}: {e} {ctx} after {:?}", se.text()));
        }
        if m.geometry {
            let (x, y, h) = (se.caret.x, se.caret.y, se.caret.height);
            let p = se.caret.pos;
            se.place_caret(&mut env, p.block, p.ofs as isize, false, false);
            if (x, y, h) != (se.caret.x, se.caret.y, se.caret.height) {
                return Err(format!(
                    "step {step}: {op:?}: caret geometry stale: {:?} now {:?} {ctx} after {:?}",
                    (x, y, h),
                    (se.caret.x, se.caret.y, se.caret.height),
                    se.text()
                ));
            }
            let widest = se.blocks().iter().map(|b| b.line_width_max).max().unwrap_or(0);
            let cw = se.content_width();
            if cw != widest {
                return Err(format!(
                    "step {step}: {op:?}: content width {cw} but widest line {widest} {ctx} after {:?}",
                    se.text()
                ));
            }
        }
        if m.normalize {
            let p = se.caret.pos;
            let b = &se.blocks()[p.block];
            if p.ofs == b.text.len()
                && p.block + 1 < se.blocks().len()
                && !b.text.is_empty()
                && se.selection().is_none()
            {
                se.place_caret(&mut env, p.block + 1, 0, true, false);
            }
        }
        if m.roundtrip && se.can_undo() {
            let t = se.text();
            se.undo(&mut env);
            let u = se.text();
            se.redo(&mut env);
            if se.text() != t {
                return Err(format!(
                    "step {step}: {op:?}: undo+redo differs: {:?} -> {:?} -> {:?} {ctx}",
                    t,
                    u,
                    se.text()
                ));
            }
            if let Err(e) = check(&se, m.strict) {
                return Err(format!("step {step}: {op:?}: after undo+redo: {e} {ctx} after {:?}", se.text()));
            }
        }
        se.paint(&mut NullPainter, Rect::new(0, 0, 120, 60), Color::rgb(0, 0, 0));
        se.take_effects();
    }
    // Undo everything, then redo everything: the text must come back.
    let end = se.text();
    let mut n = 0;
    while se.can_undo() {
        se.undo(&mut env);
        n += 1;
        check(&se, false).map_err(|e| format!("end: undo all: {e}"))?;
    }
    let start = se.text();
    for _ in 0..n {
        se.redo(&mut env);
        check(&se, false).map_err(|e| format!("end: redo all: {e}"))?;
    }
    if se.text() != end {
        return Err(format!("end: undo all ({n}) to {start:?} + redo all gives {:?} expected {:?}", se.text(), end));
    }
    Ok(())
}

fn apply_caught(ops: &[Op], m: Mode) -> Result<(), String> {
    let ops = ops.to_vec();
    match std::panic::catch_unwind(move || apply(&ops, m)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
            Err(format!("PANIC {msg:?}"))
        }
    }
}

/// The kind of a failure (to keep the same failure while shrinking).
fn kind(e: &str) -> String {
    // "step N: Op: kind..." or "end: kind..." or "PANIC ..."
    let rest = if e.starts_with("step") {
        // Skip "step N: " then the op up to the next ": ".
        let after = e.split_once(": ").map_or(e, |x| x.1);
        let mut depth = 0;
        let mut cut = 0;
        let bytes = after.as_bytes();
        for i in 0..bytes.len() {
            match bytes[i] {
                b'(' | b'{' => depth += 1,
                b')' | b'}' => depth -= 1,
                b':' if depth == 0 && bytes.get(i + 1) == Some(&b' ') => {
                    cut = i + 2;
                    break;
                }
                _ => {}
            }
        }
        &after[cut..]
    } else {
        e
    };
    rest.chars().filter(|c| !c.is_ascii_digit()).take(16).collect()
}

fn shrink(mut ops: Vec<Op>, m: Mode, err: &str) -> (Vec<Op>, String) {
    let k = kind(err);
    let mut err = err.to_string();
    loop {
        let mut changed = false;
        let mut i = 0;
        while i < ops.len() {
            let mut t = ops.clone();
            t.remove(i);
            match apply_caught(&t, m) {
                Err(e) if kind(&e) == k => {
                    ops = t;
                    err = e;
                    changed = true;
                }
                _ => i += 1,
            }
        }
        if !changed {
            return (ops, err);
        }
    }
}

fn run_many(m: Mode) {
    std::panic::set_hook(Box::new(|_| {}));
    let mut failures: Vec<(String, u64, Vec<Op>, String)> = Vec::new();
    let mut count = 0;
    let mut panics = 0;
    // 40 seeds per mode in the normal run; `FUZZ_FULL=1` runs the 299 the review ran
    // (about 12 s).
    let seeds: u64 = if std::env::var_os("FUZZ_FULL").is_some() { 300 } else { 41 };
    for seed in 1..seeds {
        // gen_ops(seed * 7919, 300) reproduces the unshrunk sequence.
        let ops = gen_ops(seed * 7919, 300);
        if let Err(e) = apply_caught(&ops, m) {
            count += 1;
            if e.starts_with("PANIC") {
                panics += 1;
            }
            let k = kind(&e);
            if failures.iter().all(|f| f.0 != k) && failures.len() < 8 {
                let (ops, e) = shrink(ops, m, &e);
                failures.push((k, seed, ops, e));
            }
        }
    }
    let _ = std::panic::take_hook();
    if !failures.is_empty() {
        let text: Vec<String> = failures
            .iter()
            .map(|(k, seed, ops, e)| format!("[{k}] seed {seed} (shrunk to {} ops): {e}\n   ops: {ops:?}", ops.len()))
            .collect();
        panic!("{count} of {} seeds fail ({panics} with a panic); kinds:\n{}", seeds - 1, text.join("\n\n"));
    }
}

const BASE: Mode = Mode {
    multiline: true,
    wrapping: true,
    styling: false,
    strict: false,
    normalize: false,
    roundtrip: false,
    geometry: false,
};

#[test]
fn fuzz_multiline_nowrap_geometry() {
    run_many(Mode { wrapping: false, normalize: true, geometry: true, ..BASE });
}

#[test]
fn fuzz_multiline_wrapping_loose() {
    run_many(BASE);
}
#[test]
fn fuzz_multiline_wrapping_strict() {
    run_many(Mode { strict: true, ..BASE });
}
#[test]
fn fuzz_multiline_wrapping_normalized() {
    run_many(Mode { strict: true, normalize: true, ..BASE });
}
#[test]
fn fuzz_multiline_wrapping_normalized_roundtrip() {
    run_many(Mode { strict: true, normalize: true, roundtrip: true, ..BASE });
}
#[test]
fn fuzz_multiline_nowrap_normalized() {
    run_many(Mode { wrapping: false, strict: true, normalize: true, ..BASE });
}
#[test]
fn fuzz_multiline_styling_normalized() {
    run_many(Mode { styling: true, strict: true, normalize: true, ..BASE });
}
#[test]
fn fuzz_singleline() {
    run_many(Mode { multiline: false, wrapping: false, ..BASE });
}
#[test]
fn fuzz_singleline_roundtrip() {
    run_many(Mode { multiline: false, wrapping: false, roundtrip: true, ..BASE });
}
#[test]
fn fuzz_singleline_styling() {
    run_many(Mode { multiline: false, wrapping: false, styling: true, ..BASE });
}
