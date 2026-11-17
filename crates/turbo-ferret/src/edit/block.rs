//! Blocks (lines that may wrap) and their fragments.
//! TB: tb_style_edit.cpp (TBBlock, TBTextFragment)

use alloc::string::String;
use alloc::vec::Vec;

use crate::font::FontDesc;
use crate::geom::{Point, Rect};
use crate::widgets::TextAlign;

use super::content::Content;
use super::{
    CHAR_NEWLINE, CHAR_PASSWORD, CHAR_SPACE, CHAR_TAB, EditEnv, EditPainter, Props, StyleEdit, TAB_SPACE, TextOfs,
    is_space, is_wordbreak, next_boundary,
};

/// The settings a layout needs.
#[derive(Copy, Clone)]
pub(crate) struct LayoutCfg {
    pub layout_width: i32,
    pub wrapping: bool,
    pub password: bool,
    pub styling: bool,
    pub font: FontDesc,
    pub size_affects_layout: bool,
}

/// A piece of a block: a word, a space, a tab, a line break or embedded content.
/// TB: TBTextFragment
#[derive(Clone, Debug)]
pub struct Fragment {
    /// Position in the block.
    pub xpos: i32,
    pub ypos: i32,
    /// Byte range in the block's text.
    pub ofs: usize,
    pub len: usize,
    /// The line it is on.
    pub line_ypos: i32,
    pub line_height: i32,
    /// From the last layout.
    pub width: i32,
    pub height: i32,
    pub baseline: i32,
    is_break: bool,
    is_space: bool,
    is_tab: bool,
    /// The width was measured in this layout pass (TB: is_width_valid).
    width_valid: bool,
    pub content: Option<Content>,
}

impl Fragment {
    /// A hard line break.
    pub fn is_break(&self) -> bool {
        self.is_break
    }
    pub fn is_space(&self) -> bool {
        self.is_space
    }
    pub fn is_tab(&self) -> bool {
        self.is_tab
    }
    pub fn is_embedded(&self) -> bool {
        self.content.is_some()
    }
}

/// A block of text: one line, which may wrap. TB: TBBlock
#[derive(Clone, Debug)]
pub struct Block {
    pub text: String,
    pub fragments: Vec<Fragment>,
    pub ypos: i32,
    pub height: i32,
    pub align: TextAlign,
    pub line_width_max: i32,
}

/// No wrapping before the character at `ofs`? TB: is_never_break_before
fn is_never_break_before(s: &[u8], ofs: usize) -> bool {
    match s.get(ofs).copied().unwrap_or(0) {
        b'\n' | b'\r' | b' ' | b'-' | b'.' | b',' | b':' | b';' | b'!' | b'?' | b')' | b']' | b'}' | b'>' => true,
        // The first quote of a word after a space may start a line.
        b'\'' | b'"' => ofs > 0 && !is_space(s[ofs - 1]),
        _ => false,
    }
}

/// No wrapping after the character at `ofs`? TB: is_never_break_after
fn is_never_break_after(s: &[u8], ofs: usize) -> bool {
    match s.get(ofs).copied().unwrap_or(0) {
        b'(' | b'[' | b'{' | b'<' | b'@' | b'$' => true,
        // The last quote of a word before a space may end a line.
        b'\'' | b'"' => !is_space(s.get(ofs + 1).copied().unwrap_or(0)),
        _ => false,
    }
}

/// The length of the fragment at the start of `text`, and whether it is a content
/// tag. TB: GetNextFragment
fn next_fragment(text: &str, styling: bool) -> (usize, bool) {
    let b = text.as_bytes();
    match b.first() {
        None => return (0, false),
        Some(b'\t') => return (1, false),
        Some(b'\r') | Some(b'\n') => return (if b.starts_with(b"\r\n") { 2 } else { 1 }, false),
        _ => {}
    }
    if styling {
        let n = Content::tag_len(text);
        if n > 0 {
            return (n, true);
        }
    }
    // A word, or one word-breaking character.
    match b.iter().position(|&c| is_wordbreak(c)) {
        Some(0) => (1, false),
        Some(i) => (i, false),
        None => (b.len(), false),
    }
}

impl Block {
    pub(crate) fn empty(align: TextAlign) -> Block {
        Block::with_text(String::new(), align)
    }

    pub(crate) fn with_text(text: String, align: TextAlign) -> Block {
        let mut b = Block { text, fragments: Vec::new(), ypos: 0, height: 0, align, line_width_max: 0 };
        // A block always has at least one fragment, laid out or not.
        b.fragments.push(b.new_fragment(0, 0));
        b
    }

    fn new_fragment(&self, ofs: usize, len: usize) -> Fragment {
        let first = self.text.as_bytes().get(ofs).copied().unwrap_or(0);
        Fragment {
            xpos: 0,
            ypos: 0,
            ofs,
            len,
            line_ypos: 0,
            line_height: 0,
            width: 0,
            height: 0,
            baseline: 0,
            is_break: len > 0 && (first == b'\r' || first == b'\n'),
            is_space: len > 0 && is_space(first),
            is_tab: len > 0 && first == b'\t',
            width_valid: false,
            content: None,
        }
    }

    /// The fragment's text.
    pub fn str(&self, f: &Fragment) -> &str {
        self.text.get(f.ofs..f.ofs + f.len).unwrap_or("")
    }

    pub(crate) fn ends_with_break(&self) -> bool {
        self.fragments.last().is_some_and(|f| f.is_break)
    }

    /// Tell the environment that this block's content is gone.
    pub(crate) fn release(&self, env: &mut dyn EditEnv) {
        for c in self.fragments.iter().filter_map(|f| f.content.as_ref()) {
            env.content_removed(c);
        }
    }

    /// TB: TBBlock::CalculateStringWidth
    fn string_width(cfg: &LayoutCfg, measure: &mut dyn FnMut(&str) -> i32, s: &str) -> i32 {
        if cfg.password {
            // What matters is the number of characters.
            return measure(CHAR_PASSWORD) * s.chars().count() as i32;
        }
        measure(s)
    }

    /// TB: TBBlock::CalculateTabWidth
    fn tab_width(measure: &mut dyn FnMut(&str) -> i32, xpos: i32) -> i32 {
        let tabsize = (measure("x") * TAB_SPACE).max(1);
        (xpos / tabsize) * tabsize + tabsize - xpos
    }

    /// The indentation of wrapped lines: lines that start with whitespace or a list
    /// point continue below their text. TB: TBBlock::GetStartIndentation
    fn start_indentation(&self, cfg: &LayoutCfg, env: &mut dyn EditEnv, first_line_len: usize) -> i32 {
        let font = cfg.font;
        let mut measure = |s: &str| env.string_width(font, s);
        let mut indentation = 0;
        let line = self.text.get(..first_line_len).unwrap_or(&self.text);
        for (i, c) in line.char_indices() {
            match c {
                '\t' => indentation += Block::tab_width(&mut measure, indentation),
                ' ' | '-' | '*' | '\u{2022}' => {
                    indentation += Block::string_width(cfg, &mut measure, &line[i..i + c.len_utf8()]);
                }
                _ => break,
            }
        }
        indentation
    }

    /// Measure fragment `i` (once per layout pass). TB: TBTextFragment::GetWidth
    fn measure_fragment(&mut self, cfg: &LayoutCfg, env: &mut dyn EditEnv, i: usize) -> i32 {
        if self.fragments[i].width_valid {
            return self.fragments[i].width;
        }
        let font = cfg.font;
        let f = &self.fragments[i];
        let (width, height, baseline) = match &f.content {
            Some(c) => {
                let (w, h) = match c {
                    Content::Hr { height, .. } => (cfg.layout_width.max(0), *height),
                    Content::Widget(_) => {
                        let s = env.content_size(c);
                        (s.w, s.h)
                    }
                    _ => (0, 0),
                };
                let baseline = match c {
                    Content::Widget(_) => (h + env.font_ascent(font)) / 2,
                    _ => h,
                };
                (w, h, baseline)
            }
            None => {
                let mut measure = |s: &str| env.string_width(font, s);
                let w = if f.is_break {
                    0
                } else if f.is_tab {
                    Block::tab_width(&mut measure, f.xpos)
                } else {
                    Block::string_width(cfg, &mut measure, self.str(f))
                };
                (w, env.font_height(font), env.font_ascent(font))
            }
        };
        let f = &mut self.fragments[i];
        f.width = width;
        f.height = height;
        f.baseline = baseline;
        f.width_valid = true;
        width
    }

    /// TB: TBTextFragment::GetAllowBreakBefore
    fn allow_break_before(&self, f: &Fragment) -> bool {
        match &f.content {
            Some(c) => c.allow_break_before(),
            None => f.len > 0 && !is_never_break_before(self.text.as_bytes(), f.ofs),
        }
    }

    /// TB: TBTextFragment::GetAllowBreakAfter
    fn allow_break_after(&self, f: &Fragment) -> bool {
        match &f.content {
            Some(c) => c.allow_break_after(),
            None => f.len > 0 && !is_never_break_after(self.text.as_bytes(), f.ofs + f.len - 1),
        }
    }

    /// Lay out the block: make the fragments (if the text changed) and place them on
    /// lines. Returns the old and new widest line and the new height, or `None` if
    /// there is no space to lay out in yet. TB: TBBlock::Layout
    pub(crate) fn layout(
        &mut self,
        cfg: &LayoutCfg,
        env: &mut dyn EditEnv,
        update_fragments: bool,
    ) -> Option<(i32, i32, i32)> {
        if update_fragments || self.fragments.is_empty() {
            self.release(env);
            self.fragments.clear();
            let mut ofs = 0;
            loop {
                let (len, is_embed) = next_fragment(&self.text[ofs..], cfg.styling);
                let mut f = self.new_fragment(ofs, len);
                if is_embed {
                    f.content = env.create_content(&self.text[ofs..ofs + len]);
                    (f.is_break, f.is_space, f.is_tab) = (false, false, false);
                }
                let is_break = f.is_break;
                self.fragments.push(f);
                ofs += len;
                // A line break ends the block's fragments (what follows belongs to
                // the next block), and so does the end of the text.
                if is_break || len == 0 || ofs >= self.text.len() {
                    break;
                }
            }
        }
        // Before the first resize there is no space; the layout comes with it.
        if cfg.layout_width <= 0 && cfg.size_affects_layout {
            return None;
        }
        for f in &mut self.fragments {
            f.width_valid = false;
        }
        let old_line_width_max = self.line_width_max;
        self.line_width_max = 0;
        let n = self.fragments.len();
        let mut line_ypos = 0;
        let mut first_line_indentation = 0;
        let mut first = 0;
        while first < n {
            // Find the last fragment of the line, with line width and x positions.
            let mut line_width;
            let mut last = n - 1;
            if cfg.wrapping {
                // The last allowed break point before the overflow.
                let mut allowed_last = None;
                let mut line_xpos = first_line_indentation;
                line_width = 0;
                for i in first..n {
                    // The x is needed for tab widths.
                    self.fragments[i].xpos = line_xpos;
                    let w = self.measure_fragment(cfg, env, i);
                    if line_xpos + w > cfg.layout_width {
                        if let Some(a) = allowed_last {
                            last = a;
                            break;
                        }
                    }
                    let f = &self.fragments[i];
                    if self.allow_break_after(f) && (i + 1 >= n || self.allow_break_before(&self.fragments[i + 1])) {
                        allowed_last = Some(i);
                        line_width = line_xpos + w;
                    }
                    line_xpos += w;
                }
                if allowed_last.is_none() {
                    line_width = line_xpos;
                }
            } else {
                line_width = first_line_indentation;
                for i in first..n {
                    self.fragments[i].xpos = line_width;
                    line_width += self.measure_fragment(cfg, env, i);
                }
            }

            // Commit the line.
            let mut line_height = 0;
            let mut line_baseline = 0;
            for i in first..=last {
                self.measure_fragment(cfg, env, i);
                let f = &mut self.fragments[i];
                line_height = line_height.max(f.height);
                line_baseline = line_baseline.max(f.baseline);
                f.ypos = line_ypos;
            }
            // Now that the line totals are known: x moves by the alignment, y by the
            // fragment's baseline against the line's.
            let xofs = match self.align {
                TextAlign::Right => cfg.layout_width - line_width,
                TextAlign::Center => (cfg.layout_width - line_width) / 2,
                TextAlign::Left => 0,
            };
            let mut adjusted_line_height = line_height;
            let block_ypos = self.ypos;
            for f in &mut self.fragments[first..=last] {
                f.line_ypos = line_ypos;
                f.line_height = line_height;
                f.ypos += line_baseline - f.baseline;
                f.xpos += xofs;
                if let Some(c) = &f.content {
                    env.content_moved(c, Point::new(f.xpos, f.ypos + block_ypos));
                }
                adjusted_line_height = adjusted_line_height.max(line_baseline - f.baseline + f.height);
            }
            if line_height != adjusted_line_height {
                for f in &mut self.fragments[first..=last] {
                    f.line_height = adjusted_line_height;
                }
            }
            self.line_width_max = self.line_width_max.max(line_width);
            // Wrapped lines are indented like the first one.
            if cfg.wrapping && first == 0 {
                let f = &self.fragments[last];
                first_line_indentation = self.start_indentation(cfg, env, f.ofs + f.len);
            }
            line_ypos += adjusted_line_height;
            first = last + 1;
        }
        Some((old_line_width_max, self.line_width_max, line_ypos))
    }

    /// The fragment at byte offset `ofs`. At a border, `prefer_first` chooses the
    /// one before. TB: TBBlock::FindFragment(ofs, prefer_first)
    pub fn find_fragment(&self, ofs: usize, prefer_first: bool) -> usize {
        self.fragments
            .iter()
            .position(|f| if prefer_first { ofs <= f.ofs + f.len } else { ofs < f.ofs + f.len })
            .unwrap_or(self.fragments.len().saturating_sub(1))
    }

    /// The fragment at a point in the block. TB: TBBlock::FindFragment(x, y)
    pub fn find_fragment_at(&self, x: i32, y: i32) -> usize {
        for (i, f) in self.fragments.iter().enumerate() {
            if y < f.line_ypos + f.line_height {
                if x < f.xpos + f.width {
                    return i;
                }
                if self.fragments.get(i + 1).is_some_and(|next| next.line_ypos > f.line_ypos) {
                    return i;
                }
            }
        }
        self.fragments.len().saturating_sub(1)
    }

    /// The x of the character at `ofs` in a fragment. TB: TBTextFragment::GetCharX
    pub(crate) fn char_x(&self, cfg: &LayoutCfg, env: &mut dyn EditEnv, f: &Fragment, ofs: usize) -> i32 {
        if f.is_embedded() || f.is_tab {
            return if ofs == 0 { 0 } else { f.width };
        }
        if f.is_break {
            return 0;
        }
        let font = cfg.font;
        let s = self.str(f);
        Block::string_width(cfg, &mut |t| env.string_width(font, t), s.get(..ofs).unwrap_or(s))
    }

    /// The offset in a fragment of the character border nearest to `x`.
    /// TB: TBTextFragment::GetCharOfs
    pub(crate) fn char_ofs(&self, cfg: &LayoutCfg, env: &mut dyn EditEnv, f: &Fragment, x: i32) -> usize {
        if f.is_embedded() || f.is_tab {
            return if x > f.width / 2 { f.len.min(1) } else { 0 };
        }
        if f.is_break {
            return 0;
        }
        let font = cfg.font;
        let s = self.str(f);
        let mut measure = |t: &str| env.string_width(font, t);
        let mut i = 0;
        while i < s.len() {
            let pos = i;
            i = next_boundary(s, i);
            // Measured from the start of the fragment each time.
            let before = Block::string_width(cfg, &mut measure, &s[..pos]);
            let width = Block::string_width(cfg, &mut measure, &s[..i]);
            if x < width - (width - before) / 2 {
                return pos;
            }
        }
        s.len()
    }

    /// The selection rectangles of this block: `bg` behind selected text, `fg` over
    /// selected content. TB: TBBlock::BuildSelectionRegion
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn selection_rects(
        &self,
        se: &StyleEdit,
        bi: usize,
        sel: (TextOfs, TextOfs),
        p: &mut dyn EditPainter,
        translate: Point,
        bg: &mut Vec<Rect>,
        fg: &mut Vec<Rect>,
    ) {
        let cfg = se.cfg();
        let font = cfg.font;
        let (start, stop) = sel;
        for f in &self.fragments {
            // TB: TBSelection::IsFragmentSelected
            let selected = if start.block == stop.block {
                start.ofs < f.ofs + f.len && stop.ofs >= f.ofs
            } else {
                (bi > start.block && bi < stop.block)
                    || (bi == start.block && f.ofs + f.len > start.ofs)
                    || (bi == stop.block && f.ofs < stop.ofs)
            };
            if !selected {
                continue;
            }
            let x = translate.x + f.xpos;
            let y = translate.y + self.ypos + f.ypos;
            if f.is_embedded() {
                fg.push(Rect::new(x, y, f.width, f.height));
                continue;
            }
            let sofs1 = (if start.block == bi { start.ofs } else { 0 }).max(f.ofs);
            let sofs2 = (if stop.block == bi { stop.ofs } else { self.text.len() }).min(f.ofs + f.len);
            // TB: TBTextFragment::GetStringWidth
            let mut width = |a: usize, b: usize| -> i32 {
                if b <= a {
                    0
                } else if b - a == f.len {
                    f.width
                } else if f.is_tab {
                    Block::tab_width(&mut |t| p.string_width(font, t), f.xpos)
                } else if f.is_break {
                    8
                } else {
                    Block::string_width(&cfg, &mut |t| p.string_width(font, t), self.text.get(a..b).unwrap_or(""))
                }
            };
            let s1x = width(f.ofs, sofs1);
            let s2x = width(sofs1, sofs2);
            bg.push(Rect::new(x + s1x, y, s2x, f.height));
        }
    }

    /// TB: TBBlock::Paint + TBTextFragment::Paint
    pub(crate) fn paint(&self, se: &StyleEdit, p: &mut dyn EditPainter, translate: Point, props: &mut Props) {
        let font = se.font;
        for f in &self.fragments {
            let x = translate.x + f.xpos;
            let y = translate.y + self.ypos + f.ypos;
            let pos = Point::new(x, y);
            if let Some(c) = &f.content {
                match c {
                    Content::Hr { width_percent, height } => {
                        let w = se.layout_width * width_percent / 100;
                        p.fill_rect(Rect::new(x + (se.layout_width - w) / 2, y, w, *height), props.color);
                    }
                    Content::Underline => {
                        props.push();
                        props.underline = true;
                    }
                    Content::TextColor(color) => {
                        props.push();
                        props.color = *color;
                    }
                    Content::StylePop => props.pop(),
                    Content::Widget(_) => {}
                }
                continue;
            }
            let color = props.color;
            if se.password {
                let cw = p.string_width(font, CHAR_PASSWORD);
                for i in 0..self.str(f).chars().count() as i32 {
                    p.draw_string(Point::new(x + i * cw, y), font, color, CHAR_PASSWORD);
                }
            } else if se.show_whitespace {
                let s = if f.is_tab {
                    CHAR_TAB
                } else if f.is_break {
                    CHAR_NEWLINE
                } else if f.is_space {
                    CHAR_SPACE
                } else {
                    self.str(f)
                };
                p.draw_string(pos, font, color, s);
            } else if !f.is_tab && !f.is_break && !f.is_space {
                p.draw_string(pos, font, color, self.str(f));
            }
            if props.underline {
                let line_h = (f.height / 16).max(1);
                p.fill_rect(Rect::new(x, y + f.baseline + 1, f.width, line_h), color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_split_at_word_breaks() {
        fn lens(s: &str, styling: bool) -> Vec<(&str, bool)> {
            let mut out = Vec::new();
            let mut ofs = 0;
            while ofs < s.len() {
                let (len, embed) = next_fragment(&s[ofs..], styling);
                out.push((&s[ofs..ofs + len], embed));
                ofs += len;
            }
            out
        }
        assert_eq!(lens("one two", false), [("one", false), (" ", false), ("two", false)]);
        assert_eq!(lens("a\tb\r\n", false), [("a", false), ("\t", false), ("b", false), ("\r\n", false)]);
        assert_eq!(lens("x.y", false), [("x", false), (".", false), ("y", false)]);
        assert_eq!(lens("Ö-ö", false), [("Ö", false), ("-", false), ("ö", false)]);
        assert_eq!(lens("a<u>b", true), [("a", false), ("<u>", true), ("b", false)]);
        assert_eq!(lens("a<u>b", false), [("a", false), ("<", false), ("u", false), (">", false), ("b", false)]);
        assert_eq!(next_fragment("", true), (0, false));
    }

    #[test]
    fn break_rules() {
        let s = b"a (b) \"c\" d.";
        assert!(is_never_break_after(s, 2)); // "("
        assert!(is_never_break_before(s, 4)); // ")"
        assert!(!is_never_break_before(s, 6)); // opening quote after a space
        assert!(is_never_break_before(s, 8)); // closing quote
        assert!(is_never_break_before(s, 11)); // "."
        assert!(!is_never_break_before(s, 10));
    }
}
