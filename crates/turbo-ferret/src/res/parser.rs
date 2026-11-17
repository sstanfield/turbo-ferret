//! The TB resource text format parser.
//!
//! TB: parser/tb_parser.cpp (TBParser, the line reader) and tb_node_tree.cpp
//! (TBNodeTarget, which builds the tree and handles `@file` and `@include`). The two are
//! one pass here. The input is a `&str` and is never modified (H-15).
//!
//! Problems TB reported through `TBParserTarget::OnError` and then skipped (bad
//! indentation, a missing `@file` or `@include`, an include cycle) are collected in
//! [`ParseCtx::warnings`] and parsing goes on. Only a file that cannot be loaded or decoded
//! at the top level, or a document over [`MAX_NODES`], is an error.

use alloc::borrow::Cow;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use crate::services::ResourceLoader;
use crate::value::Value;

use super::node::ResNode;
use super::refs::{self, RefTrees};

/// The deepest a node may be (the root's children are at depth 1). Deeper lines and
/// includes are skipped with a warning; this bounds recursion over parsed trees.
pub const MAX_DEPTH: usize = 256;

/// The deepest `@file` includes may nest (cycles are caught before this).
pub const MAX_INCLUDE_DEPTH: usize = 32;

/// The most nodes one parse may create, `@file` and `@include` copies included. Bounds
/// the memory a small file can claim through repeated includes.
pub const MAX_NODES: usize = 1 << 20;

/// A parse problem: where it happened and what it was. `line` is 1-based, 0 if the
/// problem is with the file as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub file: String,
    pub line: u32,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}", self.file, self.msg)
        } else {
            write!(f, "{}:{}: {}", self.file, self.line, self.msg)
        }
    }
}

impl core::error::Error for ParseError {}

/// What a parse needs besides the text: the file name (for messages and for resolving
/// `@file` paths), the loader for `@file`, the ref trees for `@include @tree>...` and
/// `@if @tree>...`, and the stack of files being parsed (for cycle detection).
/// Warnings collect here.
pub struct ParseCtx<'a> {
    file: String,
    loader: Option<&'a mut dyn ResourceLoader>,
    refs: Option<&'a RefTrees>,
    include_stack: Vec<String>,
    nodes: usize,
    /// Non-fatal problems, in the order found. TB: TBNodeTarget::OnError (debug print only).
    pub warnings: Vec<ParseError>,
}

impl<'a> ParseCtx<'a> {
    /// A context for the file `file`, with no loader and no ref trees.
    pub fn new(file: &str) -> Self {
        ParseCtx {
            file: String::from(file),
            loader: None,
            refs: None,
            include_stack: Vec::new(),
            nodes: 0,
            warnings: Vec::new(),
        }
    }

    pub fn with_loader(mut self, loader: &'a mut dyn ResourceLoader) -> Self {
        self.loader = Some(loader);
        self
    }

    pub fn with_refs(mut self, refs: &'a RefTrees) -> Self {
        self.refs = Some(refs);
        self
    }

    /// The file being parsed (the innermost one, during an `@file` include).
    pub fn file(&self) -> &str {
        &self.file
    }

    fn error(&self, line: u32, msg: String) -> ParseError {
        ParseError { file: self.file.clone(), line, msg }
    }

    fn warn(&mut self, line: u32, msg: String) {
        let w = self.error(line, msg);
        self.warnings.push(w);
    }

    /// Count `n` new nodes against [`MAX_NODES`].
    fn add_nodes(&mut self, n: usize, line: u32) -> Result<(), ParseError> {
        self.nodes = self.nodes.saturating_add(n);
        if self.nodes > MAX_NODES {
            return Err(self.error(line, format!("More than {MAX_NODES} nodes")));
        }
        Ok(())
    }
}

/// Parse `src` into an unnamed root node, then resolve `@if`/`@else`.
/// TB: tb_node_tree.cpp TBNode::ReadData
pub fn parse(src: &str, ctx: &mut ParseCtx) -> Result<ResNode, ParseError> {
    ctx.include_stack.push(ctx.file.clone());
    let result = parse_body(src, ctx);
    ctx.include_stack.pop();
    result
}

/// Load `ctx`'s file through its loader and parse it. A file that is missing, unreadable
/// or not UTF-8 is an error. TB: tb_node_tree.cpp TBNode::ReadFile
pub fn parse_file(ctx: &mut ParseCtx) -> Result<ResNode, ParseError> {
    let data = match ctx.loader.as_deref_mut() {
        Some(loader) => loader.load(&ctx.file).map_err(|e| ctx.error(0, e.to_string()))?,
        None => return Err(ctx.error(0, String::from("No resource loader"))),
    };
    let src = core::str::from_utf8(&data).map_err(|e| ctx.error(0, format!("Not UTF-8: {e}")))?;
    parse(src, ctx)
}

fn parse_body(src: &str, ctx: &mut ParseCtx) -> Result<ResNode, ParseError> {
    let mut p = Parser {
        ctx,
        root: ResNode::new(""),
        target: Vec::new(),
        void_depth: 0,
        indent: 0,
        line_nr: 1,
        multi_token: String::new(),
        multi_value: String::new(),
        multi_sub_level: false,
        pending_multiline: false,
    };
    p.read(src)?;
    let Parser { ctx, mut root, .. } = p;
    refs::resolve_conditions(&mut root, ctx.refs);
    Ok(root)
}

/// The parser state. TB: TBParser plus TBNodeTarget.
struct Parser<'c, 'a> {
    ctx: &'c mut ParseCtx<'a>,
    root: ResNode,
    /// The node new tokens are added to, as child indices from the root.
    /// TB: TBNodeTarget::m_target_node
    target: Vec<usize>,
    /// Levels entered below a node with no children. Tokens there are dropped. (TB set
    /// `m_target_node` to null here and never recovered it.)
    void_depth: usize,
    indent: usize,
    line_nr: u32,
    multi_token: String,
    multi_value: String,
    multi_sub_level: bool,
    pending_multiline: bool,
}

impl Parser<'_, '_> {
    // TB: tb_parser.cpp TBParser::Read
    fn read(&mut self, src: &str) -> Result<(), ParseError> {
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        for line in src.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            self.on_line(line)?;
            self.line_nr = self.line_nr.saturating_add(1);
        }
        if self.pending_multiline {
            self.warn(format!("Unterminated multi-line value for \"{}\"", self.multi_token));
        }
        Ok(())
    }

    // TB: tb_parser.cpp TBParser::OnLine
    fn on_line(&mut self, line: &str) -> Result<(), ParseError> {
        let trimmed = skip_white_space(line);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return Ok(()); // TB: is_space_or_comment (comments go to OnComment, unused)
        }
        if self.pending_multiline {
            return self.on_multiline(trimmed);
        }

        let indent = line.bytes().take_while(|&b| b == b'\t').count();
        let line = &line[indent..];
        if indent > self.indent + 1 {
            self.warn(String::from("Indentation error. (Line skipped)"));
            return Ok(());
        }
        if indent > self.indent {
            self.enter();
            self.indent += 1;
        }
        while indent < self.indent {
            self.leave();
            self.indent -= 1;
        }

        let token_end = line.find([' ', '\t']).unwrap_or(line.len());
        let token = &line[..token_end];
        let rest = skip_white_space(&line[token_end..]);

        if let Some(token) = token.strip_suffix(':') {
            // Compact line: "token: value, child: value, ...". The first value is optional.
            let mut value = Value::Null;
            let mut rest = rest;
            if is_start_of_value(rest) {
                let (v, r) = self.consume_value(rest);
                if self.pending_multiline {
                    // The value continues on the next line; the token waits for it.
                    self.multi_token = String::from(token);
                    return Ok(());
                }
                value = v;
                rest = r;
            }
            self.on_token(token, value)?;
            self.on_compact_line(rest)
        } else {
            // "token value": the rest of the line is the value, quotes and all.
            let value = if token_end < line.len() { Value::parse_auto(&unescape(rest)) } else { Value::Null };
            self.on_token(token, value)
        }
    }

    // TB: tb_parser.cpp TBParser::OnCompactLine
    fn on_compact_line(&mut self, mut line: &str) -> Result<(), ParseError> {
        self.enter();
        while !line.is_empty() {
            line = skip_white_space(line);
            let Some((token, rest)) = line.split_once(':') else {
                break; // Syntax error, expected a token (TB ignores the rest silently).
            };
            let (value, rest) = self.consume_value(skip_white_space(rest));
            line = rest;
            if self.pending_multiline {
                // The value continues on the next line; leave the level once it is done.
                self.multi_token = String::from(token);
                self.multi_sub_level = true;
                return Ok(());
            }
            self.on_token(token, value)?;
        }
        self.leave();
        Ok(())
    }

    // TB: tb_parser.cpp TBParser::OnMultiline
    fn on_multiline(&mut self, line: &str) -> Result<(), ParseError> {
        self.consume_value(skip_white_space(line));
        if !self.pending_multiline {
            let value = Value::str(&self.multi_value);
            let token = core::mem::take(&mut self.multi_token);
            self.multi_value.clear();
            self.on_token(&token, value)?;
            if self.multi_sub_level {
                self.leave();
            }
            self.multi_sub_level = false;
        }
        Ok(())
    }

    /// Read one value (quoted, or up to the next comma) and return it with the rest of
    /// the line. Tracks values that continue on the next line (a trailing `\`).
    // TB: tb_parser.cpp TBParser::ConsumeValue
    fn consume_value<'s>(&mut self, line: &'s str) -> (Value, &'s str) {
        let (value, rest) = match line.as_bytes().first() {
            Some(&q @ (b'"' | b'\'')) => {
                let body = &line[1..];
                let end = find_end_quote(body, q);
                let rest = skip_white_space(body.get(end + 1..).unwrap_or(""));
                let rest = rest.strip_prefix(',').unwrap_or(rest);
                (Value::str(&unescape(&body[..end])), rest)
            }
            _ => {
                let (v, rest) = line.split_once(',').unwrap_or((line, ""));
                (Value::parse_auto(&unescape(v)), rest)
            }
        };
        let continuing = self.pending_multiline;
        self.pending_multiline = skip_white_space(rest) == "\\";
        if continuing || self.pending_multiline {
            self.multi_value.push_str(&value.as_str());
        }
        (value, rest)
    }

    // TB: tb_node_tree.cpp TBNodeTarget::OnToken
    fn on_token(&mut self, name: &str, value: Value) -> Result<(), ParseError> {
        if self.void_depth > 0 {
            return Ok(());
        }
        match name {
            "@file" => self.include_file(&value.as_str()),
            "@include" => self.include_ref(&value.as_str()),
            _ => {
                if self.target.len() >= MAX_DEPTH {
                    self.warn(format!("Nesting deeper than {MAX_DEPTH}. (\"{name}\" skipped)"));
                    return Ok(());
                }
                self.ctx.add_nodes(1, self.line_nr)?;
                let node = ResNode { name: Rc::from(name), value, children: Vec::new() };
                self.root.at_mut(&self.target).children.push(node);
                Ok(())
            }
        }
    }

    /// Descend into the target's last child. TB: tb_node_tree.cpp TBNodeTarget::Enter
    fn enter(&mut self) {
        if self.void_depth == 0 {
            let n = self.root.at(&self.target).children.len();
            if n > 0 {
                self.target.push(n - 1);
                return;
            }
        }
        self.void_depth += 1;
    }

    // TB: tb_node_tree.cpp TBNodeTarget::Leave
    fn leave(&mut self) {
        if self.void_depth > 0 {
            self.void_depth -= 1;
        } else {
            self.target.pop();
        }
    }

    /// `@file path`: parse another file (relative to this one) and add its top-level
    /// nodes to the target. TB: tb_node_tree.cpp TBNodeTarget::IncludeFile
    fn include_file(&mut self, filename: &str) -> Result<(), ParseError> {
        let path = match self.ctx.loader.as_deref() {
            Some(loader) => loader.resolve(&self.ctx.file, filename),
            None => {
                self.warn(format!("Referenced file \"{filename}\" needs a resource loader"));
                return Ok(());
            }
        };
        if self.ctx.include_stack.contains(&path) {
            self.warn(format!("Referenced file \"{path}\" includes itself"));
            return Ok(());
        }
        if self.ctx.include_stack.len() >= MAX_INCLUDE_DEPTH {
            self.warn(format!("Referenced file \"{path}\" nests deeper than {MAX_INCLUDE_DEPTH} files"));
            return Ok(());
        }
        let loaded = match self.ctx.loader.as_deref_mut() {
            Some(loader) => loader.load(&path),
            None => return Ok(()),
        };
        let data = match loaded {
            Ok(data) => data,
            Err(e) => {
                self.warn(format!("Referenced file \"{path}\" was not found! ({e})"));
                return Ok(());
            }
        };
        let Ok(src) = core::str::from_utf8(&data) else {
            self.warn(format!("Referenced file \"{path}\" is not UTF-8"));
            return Ok(());
        };

        let outer = core::mem::replace(&mut self.ctx.file, path);
        let content = parse(src, self.ctx);
        let path = core::mem::replace(&mut self.ctx.file, outer);
        let content = content?;
        if self.fits(height(&content.children), &path) {
            self.root.at_mut(&self.target).children.extend(content.children);
        }
        Ok(())
    }

    /// `@include request`: copy the children of a node read earlier in this file, or of a
    /// ref tree node (`@include @tree>path`). TB: tb_node_tree.cpp TBNodeTarget::IncludeRef
    fn include_ref(&mut self, request: &str) -> Result<(), ParseError> {
        if request.starts_with('@') {
            let Some(source) = refs::follow_from(self.ctx.refs, None, &Value::str(request)) else {
                // TB copied nothing here, silently.
                self.warn(format!("Include \"{request}\" was not found!"));
                return Ok(());
            };
            if self.fits(height(&source.children), request) {
                self.ctx.add_nodes(count(&source.children), self.line_nr)?;
                self.root.at_mut(&self.target).children.extend(source.children.iter().cloned());
            }
            return Ok(());
        }

        // Local look-up, among the nodes read so far.
        let Some(source) = self.root.path_of(request) else {
            self.warn(format!("Include \"{request}\" was not found!"));
            return Ok(());
        };
        if self.target.starts_with(&source) {
            // The source is the target or one of its parents.
            self.warn(format!("Include \"{request}\" includes itself"));
            return Ok(());
        }
        let children = &self.root.at(&source).children;
        let (h, n) = (height(children), count(children));
        if self.fits(h, request) {
            self.ctx.add_nodes(n, self.line_nr)?;
            let copies = self.root.at(&source).children.clone();
            self.root.at_mut(&self.target).children.extend(copies);
        }
        Ok(())
    }

    /// Whether nodes `height` levels deep can be added to the target within [`MAX_DEPTH`].
    fn fits(&mut self, height: usize, what: &str) -> bool {
        if self.target.len() + height > MAX_DEPTH {
            self.warn(format!("Include \"{what}\" would nest deeper than {MAX_DEPTH}. (Skipped)"));
            return false;
        }
        true
    }

    fn warn(&mut self, msg: String) {
        self.ctx.warn(self.line_nr, msg);
    }
}

// == Util functions ========================================================================

fn skip_white_space(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

/// The height of the tallest of `nodes`.
fn height(nodes: &[ResNode]) -> usize {
    nodes.iter().map(ResNode::height).max().unwrap_or(0)
}

/// The number of nodes in `nodes` and their subtrees.
fn count(nodes: &[ResNode]) -> usize {
    nodes.iter().map(ResNode::count).sum()
}

/// The index of the closing quote in `body` (the text after the opening quote), or
/// `body.len()` if there is none. A quote preceded by an odd number of backslashes is
/// escaped. TB: tb_parser.cpp IsEndQuote
fn find_end_quote(body: &str, quote: u8) -> usize {
    let mut backslashes = 0usize;
    for (i, &c) in body.as_bytes().iter().enumerate() {
        if c == quote && backslashes % 2 == 0 {
            return i;
        }
        backslashes = if c == b'\\' { backslashes + 1 } else { 0 };
    }
    body.len()
}

/// Whether a compact line's text after the token starts with the token's own value
/// rather than a child.
fn is_start_of_value(s: &str) -> bool {
    s.starts_with(['[', '"', '\'']) || is_start_of_number(s) || is_start_of_color(s) || is_start_of_reference(s)
}

/// TB: tb_value.cpp is_start_of_number
fn is_start_of_number(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let s = s.strip_prefix('.').unwrap_or(s);
    s.starts_with(|c: char| c.is_ascii_digit())
}

/// `#` followed by 3, 4, 6 or 8 hex digits (`#fd0`, `#ffdd00`).
/// TB: tb_parser.cpp is_start_of_color
fn is_start_of_color(s: &str) -> bool {
    match s.strip_prefix('#') {
        Some(hex) => matches!(hex.bytes().take_while(u8::is_ascii_hexdigit).count(), 3 | 4 | 6 | 8),
        None => false,
    }
}

/// `@` followed by a word with no `:` (a language string or ref tree reference; a word
/// ending in `:` is a key such as `@include:`). TB: tb_parser.cpp is_start_of_reference
fn is_start_of_reference(s: &str) -> bool {
    match s.strip_prefix('@') {
        Some(rest) => !rest.split(' ').next().unwrap_or("").contains(':'),
        None => false,
    }
}

/// Replace backslash escapes: `\a \b \f \n \r \t \v \0 \" \' \\`, `\xXX` and `\uXXXX`.
/// Unknown escapes are kept as they are.
///
/// TB read up to 4 hex digits for `\x` as well (it checked the wrong character); here
/// `\x` takes up to 2. A hex escape of 0 produces nothing, as in TB; one that is not a
/// valid character produces U+FFFD. TB: tb_parser.cpp UnescapeString
pub fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains('\\') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let simple = match after.as_bytes().first() {
            Some(b'a') => Some('\x07'),
            Some(b'b') => Some('\x08'),
            Some(b'f') => Some('\x0c'),
            Some(b'n') => Some('\n'),
            Some(b'r') => Some('\r'),
            Some(b't') => Some('\t'),
            Some(b'v') => Some('\x0b'),
            Some(b'0') => Some('\0'),
            Some(b'"') => Some('"'),
            Some(b'\'') => Some('\''),
            Some(b'\\') => Some('\\'),
            _ => None,
        };
        if let Some(c) = simple {
            out.push(c);
            rest = &after[1..];
            continue;
        }
        let max_digits = match after.as_bytes().first() {
            Some(b'x') => 2,
            Some(b'u') => 4,
            _ => {
                // Not an escape: keep the backslash, the next character is read as usual.
                out.push('\\');
                rest = after;
                continue;
            }
        };
        let hex = &after[1..];
        let digits = hex.bytes().take(max_digits).take_while(u8::is_ascii_hexdigit).count();
        let code = u32::from_str_radix(&hex[..digits], 16).unwrap_or(0);
        if code != 0 {
            out.push(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
        rest = &hex[digits..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_codes() {
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(unescape(r"a\nb\tc"), "a\nb\tc");
        assert_eq!(unescape(r#"\"\'\\"#), "\"'\\");
        assert_eq!(unescape(r"\x41\x42C"), "ABC");
        assert_eq!(unescape(r"\u00e5\u20ac"), "å€");
        assert_eq!(unescape(r"\x00x"), "x");
        assert_eq!(unescape(r"\uD800"), "\u{fffd}");
        assert_eq!(unescape(r"\q\n"), "\\q\n");
        assert_eq!(unescape("end\\"), "end\\");
        assert_eq!(unescape(r"\x"), "");
    }

    #[test]
    fn end_quote() {
        assert_eq!(find_end_quote(r#"abc" x"#, b'"'), 3);
        assert_eq!(find_end_quote(r#"a\"c" x"#, b'"'), 4);
        assert_eq!(find_end_quote(r#"a\\" x"#, b'"'), 3);
        assert_eq!(find_end_quote("abc", b'"'), 3);
    }

    #[test]
    fn value_starts() {
        assert!(is_start_of_number("-.5"));
        assert!(!is_start_of_number("+5"));
        assert!(is_start_of_color("#fd0"));
        assert!(is_start_of_color("#ffdd00, x: 1"));
        assert!(!is_start_of_color("#ffdd0"));
        assert!(is_start_of_reference("@foo>bar"));
        assert!(!is_start_of_reference("@include: \"x\""));
        assert!(!is_start_of_reference("foo"));
    }
}
