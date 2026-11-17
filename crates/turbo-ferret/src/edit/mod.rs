//! `StyleEdit`: the text model, layout and editing behind `EditField`.
//!
//! TB: tb_style_edit.cpp (TBStyleEdit, TBBlock, TBTextFragment, TBCaret, TBSelection,
//! TBUndoRedoStack) and tb_style_edit_content.cpp.
//!
//! What changed in the port:
//! - Blocks and fragments are `Vec`s, and positions are indices and byte offsets
//!   (`TextOfs`), where TB had linked lists and pointers.
//! - TB called back into a listener (the edit field) in the middle of operations. Here
//!   the style edit asks an [`EditEnv`] only for font metrics and embedded content,
//!   and collects what the owner has to do afterwards in [`Effects`].
//! - Painting is read-only: fragments keep their width, height and baseline from the
//!   last layout, and style changes use a stack local to the paint call.
//! - The clipboard is the owner's business: `copy` and `cut` return the text, `paste`
//!   takes it.
//! - Text is a Rust `String`, so it is always valid UTF-8.

mod block;
mod content;
mod undo;

use alloc::string::String;
use alloc::vec::Vec;

use crate::event::{Modifiers, SpecialKey};
use crate::font::FontDesc;
use crate::geom::{Color, Point, Rect, Size};
use crate::widgets::TextAlign;

pub use block::{Block, Fragment};
pub use content::Content;
pub use undo::{UndoEvent, UndoStack};

use block::LayoutCfg;

/// Columns of a tab stop, in widths of "x".
pub const TAB_SPACE: i32 = 4;
const CHAR_NEWLINE: &str = "¶";
const CHAR_SPACE: &str = "·";
const CHAR_TAB: &str = "»";
const CHAR_PASSWORD: &str = "•";

/// What a style edit needs from its surroundings to lay out text: font metrics, and
/// the embedded content of styled text.
#[allow(unused_variables)]
pub trait EditEnv {
    fn string_width(&mut self, font: FontDesc, text: &str) -> i32;
    fn font_height(&mut self, font: FontDesc) -> i32;
    fn font_ascent(&mut self, font: FontDesc) -> i32;

    /// The content for a tag (`<...>`, whole tag given) in styled text, or `None` to
    /// show the tag as text. TB: TBTextFragmentContentFactory::CreateFragmentContent
    fn create_content(&mut self, tag: &str) -> Option<Content> {
        Content::from_tag(tag)
    }
    /// The size of content the environment made (`Content::Widget`).
    fn content_size(&mut self, content: &Content) -> Size {
        Size::ZERO
    }
    /// Content was laid out at `pos` (relative to the first line, unscrolled).
    /// TB: TBTextFragmentContent::UpdatePos
    fn content_moved(&mut self, content: &Content, pos: Point) {}
    /// The fragment of this content is gone.
    fn content_removed(&mut self, content: &Content) {}
}

/// What paint needs. TB: the drawing half of TBStyleEditListener
pub trait EditPainter {
    fn string_width(&mut self, font: FontDesc, text: &str) -> i32;
    fn draw_string(&mut self, pos: Point, font: FontDesc, color: Color, text: &str);
    fn fill_rect(&mut self, rect: Rect, color: Color);
    /// TB: DrawTextSelectionBg (also used for selected embedded content)
    fn draw_selection(&mut self, rect: Rect);
    fn draw_caret(&mut self, rect: Rect);
}

/// What to do with the caret blink timer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Blink {
    Start,
    Stop,
    /// Stop and start again (the caret was moved: show it for a full period).
    Restart,
}

/// What the owner has to do after calling into the style edit: the listener
/// callbacks of TB, collected. Take them with [`StyleEdit::take_effects`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    /// Repaint. TB: Invalidate
    pub invalidate: bool,
    /// The scroll position changed. TB: Scroll
    pub scrolled: bool,
    /// The content size changed. TB: UpdateScrollbars
    pub update_scrollbars: bool,
    /// The text changed. TB: OnChange
    pub changed: bool,
    /// TB: CaretBlinkStart / CaretBlinkStop
    pub blink: Option<Blink>,
}

/// A position in the text: a block and a byte offset in it. TB: TBTextOfs
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct TextOfs {
    pub block: usize,
    pub ofs: usize,
}

impl TextOfs {
    pub fn new(block: usize, ofs: usize) -> TextOfs {
        TextOfs { block, ofs }
    }
}

/// TB: TB_CARET_POS
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum CaretPos {
    #[default]
    Beginning,
    End,
}

/// TB: TBCaret
#[derive(Copy, Clone, Debug)]
pub struct Caret {
    /// Relative to the text (unscrolled).
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub wanted_x: i32,
    pub on: bool,
    /// At a wrap point, stay on the first of the two lines.
    pub prefer_first: bool,
    pub pos: TextOfs,
}

fn is_space(c: u8) -> bool {
    c == b' '
}

fn is_linebreak(c: u8) -> bool {
    c == b'\n' || c == b'\r'
}

/// TB: is_wordbreak
fn is_wordbreak(c: u8) -> bool {
    matches!(
        c,
        b'\n'
            | b'\r'
            | b'\t'
            | b'"'
            | b'\''
            | b'/'
            | b'\\'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'('
            | b')'
            | b'>'
            | b'<'
            | b'-'
            | b'+'
            | b'*'
            | b','
            | b'.'
            | b';'
            | b':'
            | b'&'
            | b'|'
            | b'#'
            | b'!'
            | b'='
            | b'^'
            | b'~'
            | b'?'
            | b'@'
            | b'$'
            | b' '
    )
}

/// The offset of the character boundary at or before `i`.
fn floor_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// One character forward. TB: utf8::move_inc
fn next_boundary(s: &str, i: usize) -> usize {
    match s.get(i..).and_then(|t| t.chars().next()) {
        Some(c) => i + c.len_utf8(),
        None => s.len(),
    }
}

/// One character back. TB: utf8::move_dec
fn prev_boundary(s: &str, i: usize) -> usize {
    if i == 0 { 0 } else { floor_boundary(s, i - 1) }
}

/// Edits and lays out text. TB: TBStyleEdit
pub struct StyleEdit {
    blocks: Vec<Block>,
    pub caret: Caret,
    /// Start and stop, in order. TB: TBSelection
    selection: Option<(TextOfs, TextOfs)>,
    undo: UndoStack,
    layout_width: i32,
    layout_height: i32,
    content_width: i32,
    content_height: i32,
    scroll_x: i32,
    scroll_y: i32,
    /// 0 = not selecting, 1 = by character, 2 = by word (double click).
    select_state: u8,
    mousedown_point: Point,
    font: FontDesc,
    align: TextAlign,
    multiline: bool,
    styling: bool,
    read_only: bool,
    selection_on: bool,
    show_whitespace: bool,
    password: bool,
    wrapping: bool,
    win_style_br: bool,
    calculate_content_width_needed: bool,
    lock_scrollbars: u32,
    effects: Effects,
}

impl Default for StyleEdit {
    fn default() -> Self {
        StyleEdit::new()
    }
}

impl StyleEdit {
    /// An empty style edit. It has no font metrics yet: call `set_font` or `reformat`
    /// with an environment before it is used. TB: TBStyleEdit::TBStyleEdit
    pub fn new() -> StyleEdit {
        StyleEdit {
            blocks: alloc::vec![Block::empty(TextAlign::Left)],
            caret: Caret {
                x: 0,
                y: 0,
                width: 2,
                height: 0,
                wanted_x: 0,
                on: false,
                prefer_first: true,
                pos: TextOfs::default(),
            },
            selection: None,
            undo: UndoStack::default(),
            layout_width: 0,
            layout_height: 0,
            content_width: 0,
            content_height: 0,
            scroll_x: 0,
            scroll_y: 0,
            select_state: 0,
            mousedown_point: Point::ZERO,
            font: FontDesc::default(),
            align: TextAlign::Left,
            multiline: false,
            styling: false,
            read_only: false,
            selection_on: true,
            show_whitespace: false,
            password: false,
            wrapping: false,
            win_style_br: cfg!(windows),
            calculate_content_width_needed: false,
            lock_scrollbars: 0,
            effects: Effects::default(),
        }
    }

    // ---- state ----

    /// What happened since the last call (see [`Effects`]).
    pub fn take_effects(&mut self) -> Effects {
        core::mem::take(&mut self.effects)
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
    pub fn selection(&self) -> Option<(TextOfs, TextOfs)> {
        self.selection
    }
    pub fn layout_size(&self) -> Size {
        Size::new(self.layout_width, self.layout_height)
    }
    pub fn scroll(&self) -> Point {
        Point::new(self.scroll_x, self.scroll_y)
    }
    pub fn font(&self) -> FontDesc {
        self.font
    }
    pub fn align(&self) -> TextAlign {
        self.align
    }
    pub fn is_multiline(&self) -> bool {
        self.multiline
    }
    pub fn is_styling(&self) -> bool {
        self.styling
    }
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
    pub fn is_password(&self) -> bool {
        self.password
    }
    pub fn is_wrapping(&self) -> bool {
        self.wrapping
    }
    /// True while a pointer selection is going on.
    pub fn is_selecting(&self) -> bool {
        self.select_state != 0
    }
    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    pub fn set_multiline(&mut self, on: bool) {
        self.multiline = on;
    }
    pub fn set_styling(&mut self, on: bool) {
        self.styling = on;
    }
    pub fn set_read_only(&mut self, on: bool) {
        self.read_only = on;
    }
    /// Whether the pointer can select text.
    pub fn set_selection_enabled(&mut self, on: bool) {
        self.selection_on = on;
    }
    /// Whether Enter inserts "\r\n" (the default on Windows) or "\n". Text that is set
    /// or pasted keeps its own line breaks. TB: SetWindowsStyleBreak
    pub fn set_windows_style_break(&mut self, on: bool) {
        self.win_style_br = on;
    }

    /// TB: SetPassword
    pub fn set_password(&mut self, env: &mut dyn EditEnv, on: bool) {
        if self.password != on {
            self.password = on;
            self.reformat(env, true);
        }
    }

    /// TB: SetWrapping
    pub fn set_wrapping(&mut self, env: &mut dyn EditEnv, on: bool) {
        if self.wrapping != on {
            self.wrapping = on;
            self.reformat(env, false);
        }
    }

    /// TB: SetFont
    pub fn set_font(&mut self, env: &mut dyn EditEnv, font: FontDesc) {
        if self.font != font {
            self.font = font;
            self.reformat(env, true);
        }
    }

    /// Set the alignment of new blocks and of the selected blocks (or the caret's).
    /// TB: SetAlign
    pub fn set_align(&mut self, env: &mut dyn EditEnv, align: TextAlign) {
        self.align = align;
        let (first, last) = match self.selection {
            Some((a, b)) => (a.block, b.block),
            None => (self.caret.pos.block, self.caret.pos.block),
        };
        for bi in first..=last.min(self.blocks.len() - 1) {
            if self.blocks[bi].align != align {
                self.blocks[bi].align = align;
                self.layout_block(env, bi, false, false);
            }
        }
    }

    fn cfg(&self) -> LayoutCfg {
        LayoutCfg {
            layout_width: self.layout_width,
            wrapping: self.wrapping,
            password: self.password,
            styling: self.styling,
            font: self.font,
            size_affects_layout: self.size_affects_layout(),
        }
    }

    /// True if a change of the layout size needs a new layout.
    /// TB: GetSizeAffectsLayout
    pub fn size_affects_layout(&self) -> bool {
        self.wrapping || self.align != TextAlign::Left
    }

    // ---- text offsets ----

    /// TB: TBTextOfs::GetGlobalOfs
    pub fn global_ofs(&self, pos: TextOfs) -> usize {
        self.blocks.iter().take(pos.block).map(|b| b.text.len()).sum::<usize>() + pos.ofs
    }

    /// TB: TBTextOfs::SetGlobalOfs
    pub fn from_global_ofs(&self, mut gofs: usize) -> Option<TextOfs> {
        for (i, b) in self.blocks.iter().enumerate() {
            if gofs <= b.text.len() {
                return Some(TextOfs::new(i, floor_boundary(&b.text, gofs)));
            }
            gofs -= b.text.len();
        }
        None
    }

    fn last_pos(&self) -> TextOfs {
        let b = self.blocks.len() - 1;
        TextOfs::new(b, self.blocks[b].text.len())
    }

    // ---- blocks ----

    /// A block was inserted at `at`: positions in later blocks move with them.
    fn blocks_inserted(&mut self, at: usize) {
        let fix = |p: &mut TextOfs| {
            if p.block >= at {
                p.block += 1;
            }
        };
        fix(&mut self.caret.pos);
        if let Some((a, b)) = &mut self.selection {
            fix(a);
            fix(b);
        }
    }

    /// The block at `at` was removed.
    fn remove_block(&mut self, env: &mut dyn EditEnv, at: usize) {
        let block = self.blocks.remove(at);
        block.release(env);
        let fix = |p: &mut TextOfs| {
            if p.block > at {
                p.block -= 1;
            } else if p.block == at {
                // What pointed into it is re-placed by the caller; keep it valid.
                *p = TextOfs::new(at.saturating_sub(1), 0);
            }
        };
        fix(&mut self.caret.pos);
        if let Some((a, b)) = &mut self.selection {
            fix(a);
            fix(b);
        }
    }

    /// TB: TBBlock::Layout + SetSize
    fn layout_block(&mut self, env: &mut dyn EditEnv, bi: usize, update_fragments: bool, propagate_height: bool) {
        let cfg = self.cfg();
        let Some((old_w, new_w, new_h)) = self.blocks[bi].layout(&cfg, env, update_fragments) else { return };
        self.blocks[bi].ypos = if bi > 0 { self.blocks[bi - 1].ypos + self.blocks[bi - 1].height } else { 0 };
        self.set_block_size(bi, old_w, new_w, new_h, propagate_height);
        self.effects.invalidate = true;
    }

    /// TB: TBBlock::SetSize
    fn set_block_size(&mut self, bi: usize, old_w: i32, new_w: i32, new_h: i32, propagate_height: bool) {
        let dh = new_h - self.blocks[bi].height;
        self.blocks[bi].height = new_h;
        if dh != 0 && propagate_height {
            for i in bi + 1..self.blocks.len() {
                self.blocks[i].ypos = self.blocks[i - 1].ypos + self.blocks[i - 1].height;
            }
            self.effects.invalidate = true;
        }
        // The content width can only be kept up to date in constant time when it
        // grows. If the widest block shrinks, it is recalculated when asked for.
        if !self.wrapping && !self.multiline {
            self.content_width = new_w;
        } else if new_w > self.content_width {
            self.content_width = new_w;
        } else if new_w < old_w && old_w == self.content_width {
            self.calculate_content_width_needed = true;
        }
        let last = &self.blocks[self.blocks.len() - 1];
        self.content_height = last.ypos + last.height;
        if self.lock_scrollbars == 0 && propagate_height {
            self.effects.update_scrollbars = true;
        }
    }

    /// Move what follows an extra line break in block `bi` into a new block.
    /// TB: TBBlock::Split
    fn split_block(&mut self, env: &mut dyn EditEnv, bi: usize) {
        let b = self.blocks[bi].text.as_bytes();
        let mut len = b.len();
        // The ending line break stays.
        let brlen = if len > 1 && b[len - 2] == b'\r' && b[len - 1] == b'\n' { 2 } else { 1 };
        len = len.saturating_sub(brlen);
        let Some(mut i) = (0..len).find(|&i| is_linebreak(b[i])) else { return };
        if i + 1 < len && b[i] == b'\r' && b[i + 1] == b'\n' {
            i += 1;
        }
        i += 1;
        let tail = self.blocks[bi].text.split_off(i);
        self.blocks.insert(bi + 1, Block::with_text(tail, self.align));
        self.blocks_inserted(bi + 1);
        // TB: TBBlock::Set on the new block
        self.split_block(env, bi + 1);
        self.layout_block(env, bi + 1, true, true);
    }

    /// Join block `bi` with the next one if it lost its ending line break.
    /// TB: TBBlock::Merge
    fn merge_block(&mut self, env: &mut dyn EditEnv, bi: usize) {
        if bi + 1 >= self.blocks.len() || self.blocks[bi].ends_with_break() {
            return;
        }
        let next = core::mem::take(&mut self.blocks[bi + 1].text);
        self.blocks[bi].text.push_str(&next);
        self.remove_block(env, bi + 1);
        // Make sure the layout moves the blocks that follow.
        self.blocks[bi].height = 0;
        self.layout_block(env, bi, true, true);
    }

    /// Insert text into block `bi`. Returns the bytes that were inserted.
    /// TB: TBBlock::InsertText
    fn insert_in_block(&mut self, env: &mut dyn EditEnv, bi: usize, ofs: usize, text: &str, recurse: bool) -> usize {
        self.begin_lock_scrollbars();
        let b = text.as_bytes();
        let mut first_line_len = b.len();
        if let Some(i) = b.iter().position(|&c| is_linebreak(c)) {
            first_line_len = i;
            // The line break goes in too, but not in a single line field.
            if self.multiline {
                if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') {
                    first_line_len += 1;
                }
                first_line_len += 1;
            }
        }
        let mut inserted = first_line_len;
        let ofs = floor_boundary(&self.blocks[bi].text, ofs);
        self.blocks[bi].text.insert_str(ofs, &text[..first_line_len]);
        self.split_block(env, bi);
        self.layout_block(env, bi, true, true);
        // The lines after the first line break go into the blocks that follow.
        if recurse && self.multiline {
            let mut next = bi + 1;
            let mut rest = &text[first_line_len..];
            while !rest.is_empty() {
                if next >= self.blocks.len() {
                    self.blocks.push(Block::empty(self.align));
                }
                let consumed = self.insert_in_block(env, next, 0, rest, false);
                if consumed == 0 {
                    break;
                }
                rest = &rest[consumed..];
                inserted += consumed;
                next += 1;
            }
        }
        self.end_lock_scrollbars();
        inserted
    }

    /// TB: TBBlock::RemoveContent
    fn remove_in_block(&mut self, env: &mut dyn EditEnv, bi: usize, ofs: usize, len: usize) {
        if len == 0 {
            return;
        }
        let text = &mut self.blocks[bi].text;
        let (a, b) = (floor_boundary(text, ofs), floor_boundary(text, ofs + len));
        text.replace_range(a..b, "");
        self.layout_block(env, bi, true, true);
    }

    /// The block at `y`. TB: FindBlock
    pub fn find_block(&self, y: i32) -> usize {
        self.blocks.iter().position(|b| y < b.ypos + b.height).unwrap_or(self.blocks.len() - 1)
    }

    // ---- selection ----

    pub fn is_selected(&self) -> bool {
        self.selection.is_some()
    }

    /// TB: TBSelection::Select(start, stop)
    pub fn select(&mut self, start: TextOfs, stop: TextOfs) {
        self.selection = Some((start, stop));
        self.correct_selection_order();
        self.effects.invalidate = true;
    }

    /// TB: TBSelection::Select(from, to) (points)
    fn select_points(&mut self, env: &mut dyn EditEnv, from: Point, to: Point) {
        self.place_caret_at_point(env, from);
        let start = self.caret.pos;
        self.place_caret_at_point(env, to);
        self.select(start, self.caret.pos);
        self.caret.wanted_x = self.caret.x;
    }

    /// Select by offsets into the whole text. TB: TBSelection::Select(int, int)
    pub fn select_global(&mut self, from: usize, to: usize) {
        if let (Some(a), Some(b)) = (self.from_global_ofs(from), self.from_global_ofs(to)) {
            self.select(a, b);
        }
    }

    /// Extend (or start) the selection from where the caret was to where it is.
    /// TB: TBSelection::SelectToCaret
    fn select_to_caret(&mut self, old: TextOfs) {
        let caret = self.caret.pos;
        self.selection = Some(match self.selection {
            None => (old, caret),
            Some((start, stop)) => {
                if start == old {
                    (caret, stop)
                } else {
                    (start, caret)
                }
            }
        });
        self.correct_selection_order();
        self.effects.invalidate = true;
    }

    /// TB: TBSelection::SelectAll
    pub fn select_all(&mut self) {
        self.selection = Some((TextOfs::new(0, 0), self.last_pos()));
        self.effects.invalidate = true;
    }

    /// TB: TBSelection::SelectNothing
    pub fn select_nothing(&mut self) {
        if self.selection.take().is_some() {
            self.effects.invalidate = true;
        }
    }

    /// TB: TBSelection::CorrectOrder
    fn correct_selection_order(&mut self) {
        self.selection = match self.selection {
            Some((a, b)) if a == b => None,
            Some((a, b)) if a > b => Some((b, a)),
            s => s,
        };
    }

    /// The selected text ("" without a selection). TB: TBSelection::GetText
    pub fn selected_text(&self) -> String {
        match self.selection {
            Some((a, b)) => self.text_between(a, b),
            None => String::new(),
        }
    }

    fn text_between(&self, a: TextOfs, b: TextOfs) -> String {
        if a.block == b.block {
            return String::from(&self.blocks[a.block].text[a.ofs..b.ofs]);
        }
        let mut out = String::from(&self.blocks[a.block].text[a.ofs..]);
        for block in &self.blocks[a.block + 1..b.block] {
            out.push_str(&block.text);
        }
        out.push_str(&self.blocks[b.block].text[..b.ofs]);
        out
    }

    /// Remove the selected text. TB: TBSelection::RemoveContent
    pub fn remove_selection(&mut self, env: &mut dyn EditEnv) {
        let Some((start, stop)) = self.selection else { return };
        self.begin_lock_scrollbars();
        if !self.undo.applying {
            let (gofs, text) = (self.global_ofs(start), self.text_between(start, stop));
            self.undo.commit(self.read_only, gofs, &text, false);
        }
        let stop_block = if start.block == stop.block {
            self.remove_in_block(env, start.block, start.ofs, stop.ofs - start.ofs);
            start.block
        } else {
            // The end of the first block, the blocks in between, the start of the last.
            let len = self.blocks[start.block].text.len();
            self.remove_in_block(env, start.block, start.ofs, len - start.ofs);
            for _ in start.block + 1..stop.block {
                self.remove_block(env, start.block + 1);
            }
            self.remove_in_block(env, start.block + 1, 0, stop.ofs);
            start.block + 1
        };
        self.merge_block(env, stop_block);
        self.merge_block(env, start.block);
        self.place_caret(env, start.block, start.ofs as isize, true, false);
        self.caret.wanted_x = self.caret.x;
        self.select_nothing();
        self.end_lock_scrollbars();
    }

    // ---- caret ----

    fn caret_fragment(&self) -> usize {
        self.blocks[self.caret.pos.block].find_fragment(self.caret.pos.ofs, self.caret.prefer_first)
    }

    /// TB: TBCaret::UpdatePos
    fn update_caret_pos(&mut self, env: &mut dyn EditEnv) {
        self.effects.invalidate = true;
        let cfg = self.cfg();
        let block = &self.blocks[self.caret.pos.block];
        let f = &block.fragments[self.caret_fragment()];
        self.caret.x = f.xpos + block.char_x(&cfg, env, f, self.caret.pos.ofs.saturating_sub(f.ofs));
        self.caret.y = f.ypos + block.ypos;
        self.caret.height = f.height;
        if self.caret.height == 0 {
            // Without a height it is probably inside a style switch: use the line's.
            self.caret.y = f.line_ypos + block.ypos;
            self.caret.height = f.line_height;
        }
    }

    /// Move the caret a character or a word. TB: TBCaret::Move
    pub fn move_caret(&mut self, env: &mut dyn EditEnv, forward: bool, word: bool) -> bool {
        // Stay on the same line when it reaches the wrap point.
        self.caret.prefer_first = forward;
        let word = word && !self.password;
        let mut pos = self.caret.pos;
        let text = &self.blocks[pos.block].text;
        let (b, len) = (text.as_bytes(), text.len());
        let at = |i: usize| b.get(i).copied().unwrap_or(b'\n');
        if word && !(forward && pos.ofs == len) && !(!forward && pos.ofs == 0) {
            if forward {
                if is_linebreak(at(pos.ofs)) {
                    pos.ofs += 1;
                } else if is_wordbreak(at(pos.ofs)) {
                    while pos.ofs < len && is_wordbreak(at(pos.ofs)) && !is_linebreak(at(pos.ofs)) {
                        pos.ofs += 1;
                    }
                } else {
                    while pos.ofs < len && !is_wordbreak(at(pos.ofs)) {
                        pos.ofs += 1;
                    }
                    while pos.ofs < len && is_space(at(pos.ofs)) {
                        pos.ofs += 1;
                    }
                }
            } else {
                while pos.ofs > 0 && is_space(at(pos.ofs - 1)) {
                    pos.ofs -= 1;
                }
                if pos.ofs > 0 && is_wordbreak(at(pos.ofs - 1)) {
                    while pos.ofs > 0 && is_wordbreak(at(pos.ofs - 1)) {
                        pos.ofs -= 1;
                    }
                } else {
                    while pos.ofs > 0 && !is_wordbreak(at(pos.ofs - 1)) {
                        pos.ofs -= 1;
                    }
                }
            }
        } else if forward && pos.ofs >= len && pos.block + 1 < self.blocks.len() {
            pos = TextOfs::new(pos.block + 1, 0);
        } else if !forward && pos.ofs == 0 && pos.block > 0 {
            pos = TextOfs::new(pos.block - 1, self.blocks[pos.block - 1].text.len());
        } else if forward {
            pos.ofs = next_boundary(text, pos.ofs);
        } else {
            pos.ofs = prev_boundary(text, pos.ofs);
        }
        self.place_caret(env, pos.block, pos.ofs as isize, true, forward)
    }

    /// Put the caret at a point (in text coordinates). TB: TBCaret::Place(point)
    pub fn place_caret_at_point(&mut self, env: &mut dyn EditEnv, p: Point) -> bool {
        let bi = self.find_block(p.y);
        let cfg = self.cfg();
        let block = &self.blocks[bi];
        let fi = block.find_fragment_at(p.x, p.y - block.ypos);
        let f = &block.fragments[fi];
        let ofs = f.ofs + block.char_ofs(&cfg, env, f, p.x - f.xpos);
        if !self.place_caret(env, bi, ofs as isize, true, false) {
            return false;
        }
        if self.caret_fragment() != fi {
            self.caret.prefer_first = !self.caret.prefer_first;
            self.place_caret(env, bi, ofs as isize, true, false);
        }
        true
    }

    /// TB: TBCaret::Place(TB_CARET_POS)
    pub fn place_caret_at(&mut self, env: &mut dyn EditEnv, pos: CaretPos) {
        let p = match pos {
            CaretPos::Beginning => TextOfs::new(0, 0),
            CaretPos::End => self.last_pos(),
        };
        self.place_caret(env, p.block, p.ofs as isize, true, false);
    }

    /// Put the caret at an offset in a block. An offset outside the block continues
    /// into the neighbours. Returns true if the caret moved.
    /// TB: TBCaret::Place(block, ofs, allow_snap, snap_forward)
    pub fn place_caret(
        &mut self,
        env: &mut dyn EditEnv,
        block: usize,
        ofs: isize,
        allow_snap: bool,
        snap_forward: bool,
    ) -> bool {
        let mut bi = block.min(self.blocks.len() - 1);
        let mut ofs = ofs;
        while bi + 1 < self.blocks.len() && ofs > self.blocks[bi].text.len() as isize {
            ofs -= self.blocks[bi].text.len() as isize;
            bi += 1;
        }
        while bi > 0 && ofs < 0 {
            bi -= 1;
            ofs += self.blocks[bi].text.len() as isize;
        }
        let mut ofs = floor_boundary(&self.blocks[bi].text, ofs.max(0) as usize);
        // Do not stand inside a line break.
        if allow_snap {
            let f = &self.blocks[bi].fragments[self.blocks[bi].find_fragment(ofs, false)];
            if ofs > f.ofs && f.is_break() {
                if snap_forward && bi + 1 < self.blocks.len() {
                    bi += 1;
                    ofs = 0;
                } else {
                    ofs = f.ofs;
                }
            }
        }
        let new = TextOfs::new(bi, ofs);
        let changed = self.caret.pos != new;
        self.caret.pos = new;
        self.update_caret_pos(env);
        changed
    }

    /// TB: TBCaret::SetGlobalOfs
    pub fn set_caret_global_ofs(&mut self, env: &mut dyn EditEnv, gofs: usize, allow_snap: bool, snap_forward: bool) {
        if let Some(p) = self.from_global_ofs(gofs) {
            self.place_caret(env, p.block, p.ofs as isize, allow_snap, snap_forward);
        }
    }

    pub fn caret_global_ofs(&self) -> usize {
        self.global_ofs(self.caret.pos)
    }

    /// TB: TBCaret::AvoidLineBreak
    fn avoid_line_break(&mut self, env: &mut dyn EditEnv) {
        let f = &self.blocks[self.caret.pos.block].fragments[self.caret_fragment()];
        if self.caret.pos.ofs > f.ofs && f.is_break() {
            self.caret.pos.ofs = f.ofs;
        }
        self.update_caret_pos(env);
    }

    /// TB: TBCaret::ResetBlink
    fn reset_blink(&mut self) {
        self.caret.on = true;
        self.effects.blink = Some(Blink::Restart);
        self.effects.invalidate = true;
    }

    /// Flip the caret (the owner's blink timer calls this).
    pub fn blink(&mut self) {
        self.caret.on = !self.caret.on;
        self.effects.invalidate = true;
    }

    // ---- content, size, scroll ----

    /// TB: Clear
    pub fn clear(&mut self, env: &mut dyn EditEnv) {
        self.undo.clear(true, true);
        self.select_nothing();
        if self.is_empty() {
            return;
        }
        for b in self.blocks.drain(..) {
            b.release(env);
        }
        self.blocks.push(Block::empty(self.align));
        self.caret.pos = TextOfs::default();
        // (TB kept the width of the widest removed block here.)
        self.content_width = 0;
        self.layout_block(env, 0, true, true);
        self.place_caret(env, 0, 0, true, false);
        self.caret.wanted_x = self.caret.x;
        self.effects.invalidate = true;
    }

    /// TB: IsEmpty
    pub fn is_empty(&self) -> bool {
        self.blocks.len() == 1 && self.blocks[0].text.is_empty()
    }

    /// Replace the text. TB: SetText
    pub fn set_text(&mut self, env: &mut dyn EditEnv, text: &str, pos: CaretPos) {
        let was_empty = self.is_empty();
        self.clear(env);
        if text.is_empty() {
            self.caret.wanted_x = self.caret.x;
            self.scroll_if_needed(true, true);
            // (TB reported no change when text was replaced by nothing.)
            self.effects.changed = !was_empty;
            return;
        }
        self.insert_in_block(env, 0, 0, text, true);
        self.place_caret(env, 0, 0, true, false);
        self.caret.wanted_x = self.caret.x;
        self.scroll_if_needed(true, false);
        if pos == CaretPos::End {
            self.place_caret_at(env, CaretPos::End);
        }
        self.effects.changed = true;
    }

    /// The whole text. TB: GetText
    pub fn text(&self) -> String {
        self.text_between(TextOfs::new(0, 0), self.last_pos())
    }

    /// Insert text at the caret (replacing the selection), or after the last
    /// character. TB: InsertText
    pub fn insert_text(&mut self, env: &mut dyn EditEnv, text: &str, after_last: bool, clear_undo_redo: bool) {
        self.remove_selection(env);
        if after_last {
            let p = self.last_pos();
            self.place_caret(env, p.block, p.ofs as isize, false, false);
        }
        let pos = self.caret.pos;
        let inserted = self.insert_in_block(env, pos.block, pos.ofs, text, true);
        if clear_undo_redo {
            self.undo.clear(true, true);
        } else {
            let gofs = self.caret_global_ofs();
            self.undo.commit(self.read_only, gofs, &text[..inserted.min(text.len())], true);
        }
        self.place_caret(env, pos.block, (pos.ofs + inserted) as isize, false, false);
        self.caret.wanted_x = self.caret.x;
    }

    /// TB: AppendText
    pub fn append_text(&mut self, env: &mut dyn EditEnv, text: &str, clear_undo_redo: bool) {
        self.insert_text(env, text, true, clear_undo_redo);
    }

    /// TB: InsertBreak
    pub fn insert_break(&mut self, env: &mut dyn EditEnv) {
        if !self.multiline {
            return;
        }
        // At the end of a last line without a line break, two are needed: one to end
        // the line and one for the new, empty line.
        let block = &self.blocks[self.caret.pos.block];
        let at_open_end = self.caret.pos.ofs == block.text.len() && !block.ends_with_break();
        let br = match (self.win_style_br, at_open_end) {
            (true, true) => "\r\n\r\n",
            (true, false) => "\r\n",
            (false, true) => "\n\n",
            (false, false) => "\n",
        };
        self.insert_text(env, br, false, false);
        self.avoid_line_break(env);
        if self.caret.pos.block + 1 < self.blocks.len() {
            self.place_caret(env, self.caret.pos.block + 1, 0, true, false);
        }
    }

    /// TB: ScrollIfNeeded
    pub fn scroll_if_needed(&mut self, x: bool, y: bool) {
        if self.layout_width <= 0 || self.layout_height <= 0 {
            return; // before the first layout
        }
        let (mut nx, mut ny) = (self.scroll_x, self.scroll_y);
        let c = &self.caret;
        if x {
            if c.x - self.scroll_x < 0 {
                nx = c.x;
            }
            if c.x + c.width - self.scroll_x > self.layout_width {
                nx = c.x + c.width - self.layout_width;
            }
        }
        if y {
            if c.y - self.scroll_y < 0 {
                ny = c.y;
            }
            if c.y + c.height - self.scroll_y > self.layout_height {
                ny = c.y + c.height - self.layout_height;
            }
        }
        self.set_scroll_pos(nx, ny);
    }

    /// TB: SetScrollPos
    pub fn set_scroll_pos(&mut self, x: i32, y: i32) {
        let x = x.min(self.content_width() - self.layout_width).max(0);
        let mut y = y.min(self.content_height - self.layout_height).max(0);
        if !self.multiline {
            y = 0;
        }
        if x != self.scroll_x || y != self.scroll_y {
            self.scroll_x = x;
            self.scroll_y = y;
            self.effects.scrolled = true;
            self.effects.invalidate = true;
        }
    }

    /// Hold back `Effects::update_scrollbars` during many changes (it may make the
    /// content width be recalculated over all blocks). TB: BeginLockScrollbars
    fn begin_lock_scrollbars(&mut self) {
        self.lock_scrollbars += 1;
    }

    /// TB: EndLockScrollbars
    fn end_lock_scrollbars(&mut self) {
        self.lock_scrollbars -= 1;
        if self.lock_scrollbars == 0 {
            self.effects.update_scrollbars = true;
        }
    }

    /// Set the size the text is laid out in. `is_virtual` is for a trial layout that
    /// must not scroll. TB: SetLayoutSize
    pub fn set_layout_size(&mut self, env: &mut dyn EditEnv, width: i32, height: i32, is_virtual: bool) {
        if width == self.layout_width && height == self.layout_height {
            return;
        }
        let reformat = self.layout_width != width;
        self.layout_width = width;
        self.layout_height = height;
        if reformat && self.size_affects_layout() {
            self.reformat(env, false);
        }
        self.update_caret_pos(env);
        self.caret.wanted_x = self.caret.x;
        if !is_virtual {
            // A bounds check: scroll back if it is outside now.
            self.set_scroll_pos(self.scroll_x, self.scroll_y);
        }
    }

    /// Lay out every block again. TB: Reformat
    pub fn reformat(&mut self, env: &mut dyn EditEnv, update_fragments: bool) {
        self.begin_lock_scrollbars();
        let mut ypos = 0;
        for bi in 0..self.blocks.len() {
            // Set ypos directly: every block is visited anyway.
            self.blocks[bi].ypos = ypos;
            self.layout_block(env, bi, update_fragments, false);
            self.blocks[bi].ypos = ypos;
            ypos += self.blocks[bi].height;
        }
        let last = &self.blocks[self.blocks.len() - 1];
        self.content_height = last.ypos + last.height;
        self.end_lock_scrollbars();
        self.update_caret_pos(env);
        self.effects.invalidate = true;
    }

    /// TB: GetContentWidth
    pub fn content_width(&mut self) -> i32 {
        if self.calculate_content_width_needed {
            self.calculate_content_width_needed = false;
            self.content_width = self.blocks.iter().map(|b| b.line_width_max).max().unwrap_or(0);
        }
        self.content_width
    }

    /// TB: GetContentHeight
    pub fn content_height(&self) -> i32 {
        self.content_height
    }

    /// TB: GetOverflowX
    pub fn overflow_x(&mut self) -> i32 {
        (self.content_width() - self.layout_width).max(0)
    }

    /// TB: GetOverflowY
    pub fn overflow_y(&self) -> i32 {
        (self.content_height - self.layout_height).max(0)
    }

    // ---- input ----

    /// A key went down. Returns true if it was used. TB: KeyDown
    pub fn key_down(&mut self, env: &mut dyn EditEnv, key: Option<char>, special: SpecialKey, mods: Modifiers) -> bool {
        if self.select_state != 0 {
            return false;
        }
        use SpecialKey as K;
        let ctrl = mods.contains(Modifiers::CTRL);
        let shift = mods.contains(Modifiers::SHIFT);
        let mut handled = true;
        let move_caret =
            matches!(special, K::Left | K::Right | K::Up | K::Down | K::Home | K::End | K::PageUp | K::PageDown);
        if !shift && move_caret {
            self.select_nothing();
        }
        let old = self.caret.pos;
        let (line_ypos, line_height) = {
            let f = &self.blocks[old.block].fragments[self.caret_fragment()];
            (f.line_ypos, f.line_height)
        };
        let block_y = self.blocks[old.block].ypos;
        let wanted_x = self.caret.wanted_x;
        let editable = !self.read_only;

        if matches!(special, K::Up | K::Down) && ctrl {
            let line = env.font_height(self.font);
            let dy = if special == K::Up { -line } else { line };
            self.set_scroll_pos(self.scroll_x, self.scroll_y + dy);
        } else if special == K::Left {
            self.move_caret(env, false, ctrl);
        } else if special == K::Right {
            self.move_caret(env, true, ctrl);
        } else if special == K::Up {
            handled = self.place_caret_at_point(env, Point::new(wanted_x, block_y + line_ypos - 1));
        } else if special == K::Down {
            handled = self.place_caret_at_point(env, Point::new(wanted_x, block_y + line_ypos + line_height + 1));
        } else if special == K::PageUp {
            self.place_caret_at_point(env, Point::new(wanted_x, self.caret.y - self.layout_height));
        } else if special == K::PageDown {
            self.place_caret_at_point(env, Point::new(wanted_x, self.caret.y + self.layout_height + line_height));
        } else if special == K::Home && ctrl {
            self.place_caret_at_point(env, Point::ZERO);
        } else if special == K::End && ctrl {
            let last = &self.blocks[self.blocks.len() - 1];
            let y = last.ypos + last.height;
            self.place_caret_at_point(env, Point::new(32000, y));
        } else if special == K::Home {
            self.place_caret_at_point(env, Point::new(0, self.caret.y));
        } else if special == K::End {
            self.place_caret_at_point(env, Point::new(32000, self.caret.y));
        } else if key == Some('8') && ctrl {
            self.show_whitespace = !self.show_whitespace;
            self.effects.invalidate = true;
        } else if editable && matches!(special, K::Delete | K::Backspace) {
            if !self.is_selected() {
                self.move_caret(env, special == K::Delete, ctrl);
                self.select_to_caret(old);
            }
            self.remove_selection(env);
        } else if editable && !shift && special == K::Tab && self.multiline {
            self.insert_text(env, "\t", false, false);
        } else if editable && special == K::Enter && self.multiline && !ctrl {
            self.insert_break(env);
        } else if let (true, Some(c), false) =
            (editable && special != K::Enter, key.filter(|c| !c.is_control()), ctrl || mods.contains(Modifiers::SUPER))
        {
            let mut buf = [0u8; 4];
            self.insert_text(env, c.encode_utf8(&mut buf), false, false);
        } else {
            handled = false;
        }

        if shift && move_caret {
            self.select_to_caret(old);
        }
        if !matches!(special, K::Up | K::Down | K::PageUp | K::PageDown) {
            self.caret.wanted_x = self.caret.x;
        }
        self.reset_blink();
        if !move_caret && handled {
            self.effects.changed = true;
        }
        if handled {
            self.scroll_if_needed(true, true);
        }
        handled
    }

    /// Remove the selection and return its text, for the clipboard. Nothing in a
    /// password field. TB: Cut
    pub fn cut(&mut self, env: &mut dyn EditEnv) -> Option<String> {
        let text = self.copy()?;
        self.key_down(env, None, SpecialKey::Delete, Modifiers::NONE);
        Some(text)
    }

    /// The selected text, for the clipboard. Nothing in a password field. TB: Copy
    pub fn copy(&self) -> Option<String> {
        if self.password || !self.is_selected() {
            return None;
        }
        Some(self.selected_text())
    }

    /// Insert text from the clipboard. TB: Paste
    pub fn paste(&mut self, env: &mut dyn EditEnv, text: &str) {
        self.insert_text(env, text, false, false);
        self.scroll_if_needed(true, true);
        self.effects.changed = true;
    }

    /// Remove the selection. TB: Delete
    pub fn delete(&mut self, env: &mut dyn EditEnv) {
        if self.is_selected() {
            self.remove_selection(env);
            self.effects.changed = true;
        }
    }

    /// TB: Undo
    pub fn undo(&mut self, env: &mut dyn EditEnv) {
        if let Some(e) = self.undo.pop_undo() {
            self.apply_undo_event(env, &e, true);
            self.undo.push_redo(e);
            self.effects.changed = true;
        }
    }

    /// TB: Redo
    pub fn redo(&mut self, env: &mut dyn EditEnv) {
        if let Some(e) = self.undo.pop_redo() {
            self.apply_undo_event(env, &e, false);
            self.undo.push_undo(e);
            self.effects.changed = true;
        }
    }

    /// TB: TBUndoRedoStack::Apply
    fn apply_undo_event(&mut self, env: &mut dyn EditEnv, e: &UndoEvent, reverse: bool) {
        self.undo.applying = true;
        self.select_nothing();
        if e.insert == reverse {
            // Take the text out again.
            self.set_caret_global_ofs(env, e.gofs, false, false);
            let start = self.caret.pos;
            self.set_caret_global_ofs(env, e.gofs + e.text.len(), false, false);
            self.select(start, self.caret.pos);
            self.remove_selection(env);
        } else {
            self.set_caret_global_ofs(env, e.gofs, true, true);
            self.insert_text(env, &e.text, false, false);
            if e.text.len() > 1 {
                self.select_global(e.gofs, e.gofs + e.text.len());
            }
        }
        self.scroll_if_needed(true, true);
        self.undo.applying = false;
    }

    /// Forget the undo and redo history.
    pub fn clear_undo_redo(&mut self) {
        self.undo.clear(true, true);
    }

    /// The pointer went down at `p` (relative to the layout area). TB: MouseDown
    pub fn mouse_down(&mut self, env: &mut dyn EditEnv, p: Point, clicks: u32, touch: bool) -> bool {
        self.mousedown_point = p + self.scroll();
        if !touch && self.selection_on {
            self.select_nothing();
            // Single or double click only.
            self.select_state = ((clicks.max(1) - 1) % 2) as u8 + 1;
            self.mouse_move(env, p);
            self.reset_blink();
        }
        true
    }

    /// TB: MouseUp. `cancel_click` is true if the press became something else (a pan).
    pub fn mouse_up(&mut self, env: &mut dyn EditEnv, touch: bool, cancel_click: bool) -> bool {
        if touch && !cancel_click {
            self.select_nothing();
            self.place_caret_at_point(env, self.mousedown_point);
            self.caret.wanted_x = self.caret.x;
            self.reset_blink();
        }
        self.select_state = 0;
        true
    }

    /// TB: MouseMove
    pub fn mouse_move(&mut self, env: &mut dyn EditEnv, p: Point) -> bool {
        if self.select_state == 0 {
            return false;
        }
        let to = p + self.scroll();
        self.select_points(env, self.mousedown_point, to);
        if self.select_state == 2 {
            // By word: grow both ends to word boundaries.
            let initial = self.selection;
            if let Some((start, _)) = initial {
                self.place_caret(env, start.block, start.ofs as isize, true, false);
            }
            self.move_caret(env, false, true);
            let start = self.caret.pos;
            if let Some((_, stop)) = initial {
                self.place_caret(env, stop.block, stop.ofs as isize, true, false);
            }
            self.move_caret(env, true, true);
            self.select(start, self.caret.pos);
            self.caret.wanted_x = self.caret.x;
        }
        true
    }

    /// TB: Focus
    pub fn focus(&mut self, focus: bool) {
        self.effects.blink = Some(if focus { Blink::Start } else { Blink::Stop });
        self.caret.on = focus;
        self.effects.invalidate = true;
    }

    // ---- paint ----

    /// Paint the text, the selection and the caret into `rect` (at 0, 0 of the layout
    /// area). TB: Paint
    pub fn paint(&self, p: &mut dyn EditPainter, rect: Rect, text_color: Color) {
        let mut props = Props { stack: Vec::new(), color: text_color, underline: false };
        let translate = Point::new(-self.scroll_x, -self.scroll_y);
        let first =
            self.blocks.iter().position(|b| b.ypos + b.height - self.scroll_y >= 0).unwrap_or(self.blocks.len());
        let visible = || {
            self.blocks.iter().enumerate().skip(first).take_while(|(_, b)| b.ypos - self.scroll_y <= rect.y + rect.h)
        };
        // The selection of the visible blocks: behind text, in front of content.
        let (mut bg, mut fg) = (Vec::new(), Vec::new());
        if let Some(sel) = self.selection {
            for (bi, b) in visible() {
                if bi >= sel.0.block && bi <= sel.1.block {
                    b.selection_rects(self, bi, sel, p, translate, &mut bg, &mut fg);
                }
            }
            for r in &bg {
                p.draw_selection(*r);
            }
        }
        for (_, b) in visible() {
            b.paint(self, p, translate, &mut props);
        }
        for r in &fg {
            p.draw_selection(*r);
        }
        if self.caret.on || self.select_state != 0 {
            let c = &self.caret;
            p.draw_caret(Rect::new(translate.x + c.x, translate.y + c.y, c.width, c.height));
        }
    }
}

/// The style in effect while painting. TB: TBTextProps
pub(crate) struct Props {
    stack: Vec<(Color, bool)>,
    pub(crate) color: Color,
    pub(crate) underline: bool,
}

impl Props {
    pub(crate) fn push(&mut self) {
        self.stack.push((self.color, self.underline));
    }
    pub(crate) fn pop(&mut self) {
        if let Some((c, u)) = self.stack.pop() {
            self.color = c;
            self.underline = u;
        }
    }
}
