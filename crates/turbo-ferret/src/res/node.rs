//! `ResNode`: a tree node with a name, a value and ordered children.
//!
//! TB: tb_node_tree.h, tb_node_tree.cpp (TBNode). There are no parent pointers (H-03):
//! lookups that need the document root (`@>` references), the ref trees (`@tree>`
//! references) or the language (`@key` strings) get them from a [`Lookup`].

use alloc::borrow::Cow;
use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::lang::Language;
use crate::services::ResourceLoader;
use crate::value::Value;

use super::parser::{self, ParseCtx, ParseError};
use super::refs::{self, RefTrees};

/// A node of a parsed resource: a name, a value and children, in file order.
///
/// The root returned by the parser has an empty name and a null value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResNode {
    pub name: Rc<str>,
    pub value: Value,
    pub children: Vec<ResNode>,
}

/// Split a request at its first `>`. TB: tb_node_tree.cpp TBNode::GetNextNodeSeparator
pub(crate) fn split_request(request: &str) -> (&str, &str) {
    match request.split_once('>') {
        Some((name, rest)) => (name, rest),
        None => (request, ""),
    }
}

impl ResNode {
    /// A node with a name and no value. TB: tb_node_tree.cpp TBNode::Create
    pub fn new(name: &str) -> Self {
        ResNode { name: Rc::from(name), value: Value::Null, children: Vec::new() }
    }

    /// A node with a name and a value.
    pub fn with_value(name: &str, value: impl Into<Value>) -> Self {
        ResNode { name: Rc::from(name), value: value.into(), children: Vec::new() }
    }

    /// Parse a resource from a string, with no loader and no ref trees (`@file` and
    /// `@include @tree>...` are reported as warnings and skipped). Warnings are dropped;
    /// use [`parse`](super::parse) with a [`ParseCtx`] to see them.
    /// TB: tb_node_tree.cpp TBNode::ReadData
    pub fn parse_str(src: &str) -> Result<ResNode, ParseError> {
        parser::parse(src, &mut ParseCtx::new("{data}"))
    }

    /// Load and parse a resource file through `loader`, using `refs` for `@include
    /// @tree>...` and `@if @tree>...`. Warnings are dropped; use
    /// [`parse_file`](super::parse_file) with a [`ParseCtx`] to see them.
    /// TB: tb_node_tree.cpp TBNode::ReadFile
    pub fn load(loader: &mut dyn ResourceLoader, path: &str, refs: Option<&RefTrees>) -> Result<ResNode, ParseError> {
        let mut ctx = ParseCtx::new(path).with_loader(loader);
        if let Some(refs) = refs {
            ctx = ctx.with_refs(refs);
        }
        parser::parse_file(&mut ctx)
    }

    /// Append a child and return it. TB: tb_node_tree.h TBNode::Add
    pub fn add(&mut self, child: ResNode) -> &mut ResNode {
        self.children.push(child);
        let last = self.children.len() - 1;
        &mut self.children[last]
    }

    pub fn children(&self) -> &[ResNode] {
        &self.children
    }

    /// The children, in order.
    pub fn iter(&self) -> core::slice::Iter<'_, ResNode> {
        self.children.iter()
    }

    /// The children with the given name, in order.
    pub fn iter_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a ResNode> + 'a {
        self.children.iter().filter(move |c| &*c.name == name)
    }

    pub fn first_child(&self) -> Option<&ResNode> {
        self.children.first()
    }

    pub fn last_child(&self) -> Option<&ResNode> {
        self.children.last()
    }

    /// The first direct child with exactly this name. TB: tb_node_tree.cpp TBNode::GetNodeInternal
    pub fn child(&self, name: &str) -> Option<&ResNode> {
        self.children.iter().find(|c| &*c.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut ResNode> {
        self.children.iter_mut().find(|c| &*c.name == name)
    }

    /// The node at `path` (names separated by `>`, as in `"dishes>pizza>special"`), without
    /// following references. An empty path is this node.
    /// TB: tb_node_tree.cpp TBNode::GetNode (GET_MISS_POLICY_NULL)
    pub fn get(&self, path: &str) -> Option<&ResNode> {
        let mut node = self;
        let mut request = path;
        while !request.is_empty() {
            let (name, rest) = split_request(request);
            node = node.child(name)?;
            request = rest;
        }
        Some(node)
    }

    /// Mutable [`get`](Self::get).
    pub fn get_mut(&mut self, path: &str) -> Option<&mut ResNode> {
        let mut node = self;
        let mut request = path;
        while !request.is_empty() {
            let (name, rest) = split_request(request);
            node = node.child_mut(name)?;
            request = rest;
        }
        Some(node)
    }

    /// The node at `path`, creating any missing nodes (appended, with null values).
    /// TB: tb_node_tree.cpp TBNode::GetNode (GET_MISS_POLICY_CREATE)
    ///
    /// A path is followed [`MAX_DEPTH`](super::MAX_DEPTH) names deep at most (what
    /// the parser allows too): trees are walked recursively.
    pub fn get_or_create(&mut self, path: &str) -> &mut ResNode {
        let mut node = self;
        let mut request = path;
        let mut depth = 0;
        while !request.is_empty() && depth < super::MAX_DEPTH {
            depth += 1;
            let (name, rest) = split_request(request);
            let index = match node.children.iter().position(|c| &*c.name == name) {
                Some(i) => i,
                None => {
                    node.children.push(ResNode::new(name));
                    node.children.len() - 1
                }
            };
            node = &mut node.children[index];
            request = rest;
        }
        node
    }

    /// Append copies of all of `source`'s children (after any existing ones; nodes with
    /// the same name are not replaced). TB: tb_node_tree.cpp TBNode::CloneChildren
    pub fn clone_children_from(&mut self, source: &ResNode) {
        self.children.extend(source.children.iter().cloned());
    }

    /// The value at `path`, following `@>` references with this node as the root.
    /// TB: tb_node_tree.cpp TBNode::GetNodeFollowRef
    pub fn value_at(&self, path: &str) -> Option<&Value> {
        Lookup::new(self).value(self, path)
    }

    /// The value at `path` as an int, or `default` if there is no such node. Follows `@>`
    /// references with this node as the root. TB: tb_node_tree.cpp TBNode::GetValueInt
    pub fn int(&self, path: &str, default: i32) -> i32 {
        Lookup::new(self).int(self, path, default)
    }

    /// The value at `path` as a float, or `default`. TB: tb_node_tree.cpp TBNode::GetValueFloat
    pub fn float(&self, path: &str, default: f32) -> f32 {
        Lookup::new(self).float(self, path, default)
    }

    /// The value at `path` as a string (numbers are formatted), or `default`. Follows `@>`
    /// references with this node as the root. `@key` language strings are returned as
    /// they are; use a [`Lookup`] with a [`Language`] to translate them.
    /// TB: tb_node_tree.cpp TBNode::GetValueStringRaw
    pub fn str<'a>(&'a self, path: &str, default: &'a str) -> Cow<'a, str> {
        Lookup::new(self).str_raw(self, path, default)
    }

    /// The node at a path of child indices (an invariant of the parser: always valid).
    pub(crate) fn at(&self, path: &[usize]) -> &ResNode {
        let mut node = self;
        for &i in path {
            node = &node.children[i];
        }
        node
    }

    pub(crate) fn at_mut(&mut self, path: &[usize]) -> &mut ResNode {
        let mut node = self;
        for &i in path {
            node = &mut node.children[i];
        }
        node
    }

    /// Like [`get`](Self::get), but returns the path of child indices.
    pub(crate) fn path_of(&self, path: &str) -> Option<Vec<usize>> {
        let mut node = self;
        let mut indices = Vec::new();
        let mut request = path;
        while !request.is_empty() {
            let (name, rest) = split_request(request);
            let i = node.children.iter().position(|c| &*c.name == name)?;
            indices.push(i);
            node = &node.children[i];
            request = rest;
        }
        Some(indices)
    }

    /// Levels in this subtree, counting this node (a leaf is 1).
    pub(crate) fn height(&self) -> usize {
        1 + self.children.iter().map(ResNode::height).max().unwrap_or(0)
    }

    /// Nodes in this subtree, counting this node.
    pub(crate) fn count(&self) -> usize {
        1 + self.children.iter().map(ResNode::count).sum::<usize>()
    }
}

impl<'a> IntoIterator for &'a ResNode {
    type Item = &'a ResNode;
    type IntoIter = core::slice::Iter<'a, ResNode>;
    fn into_iter(self) -> Self::IntoIter {
        self.children.iter()
    }
}

/// What value lookups need besides the node: the document root (for `@>` references), the
/// ref trees (for `@tree>` references) and the language (for `@key` strings).
///
/// TB found these through parent pointers and globals (`g_tb_lng`, the static ref tree
/// list). Without refs, `@tree>` references stay unresolved; without a language, `@key`
/// strings are returned as they are.
#[derive(Clone, Copy)]
pub struct Lookup<'a> {
    pub root: &'a ResNode,
    pub refs: Option<&'a RefTrees>,
    pub lang: Option<&'a Language>,
}

impl<'a> Lookup<'a> {
    pub fn new(root: &'a ResNode) -> Self {
        Lookup { root, refs: None, lang: None }
    }

    pub fn with_refs(mut self, refs: &'a RefTrees) -> Self {
        self.refs = Some(refs);
        self
    }

    pub fn with_lang(mut self, lang: &'a Language) -> Self {
        self.lang = Some(lang);
        self
    }

    /// Follow `node`'s reference chain (`@>path`, `@tree>path`) to the final node. A broken
    /// reference stops at the last node reached; a loop returns `node`.
    /// TB: tb_node_ref_tree.cpp TBNodeRefTree::FollowNodeRef
    pub fn follow(&self, node: &'a ResNode) -> &'a ResNode {
        refs::follow(self.refs, self.root, node)
    }

    /// A copy of `node` whose property nodes are resolved: a property that refers
    /// elsewhere (`@tree>path`, `@>path`) is replaced by the node it refers to, and a
    /// `@key` string by the language's string. `None` if no property needs it, so the
    /// node can be used as it is. Nodes for which `is_widget` is true are copied
    /// unchanged (they are resolved when they are inflated themselves).
    ///
    /// Code that reads a node with the plain [`ResNode`] getters, like a widget's
    /// `on_inflate`, sees what TB's `GetValueString` and `GetNodeFollowRef` gave.
    pub fn resolved_properties(&self, node: &'a ResNode, is_widget: &dyn Fn(&str) -> bool) -> Option<ResNode> {
        fn refers(n: &ResNode, is_widget: &dyn Fn(&str) -> bool) -> bool {
            n.children
                .iter()
                .filter(|c| !is_widget(&c.name))
                .any(|c| matches!(&c.value, Value::Str(s) if s.starts_with('@')) || refers(c, is_widget))
        }
        if !refers(node, is_widget) {
            return None;
        }
        Some(ResNode {
            name: node.name.clone(),
            value: node.value.clone(),
            children: self.resolved_children(node, is_widget, 0),
        })
    }

    fn resolved_children(&self, node: &'a ResNode, is_widget: &dyn Fn(&str) -> bool, depth: u32) -> Vec<ResNode> {
        /// References into subtrees that refer on are followed this deep.
        const MAX_DEPTH: u32 = 16;
        node.children
            .iter()
            .map(|c| {
                if is_widget(&c.name) || depth >= MAX_DEPTH {
                    return c.clone();
                }
                let target = self.follow(c);
                let value = match (&target.value, self.lang) {
                    (Value::Str(s), Some(lang)) => {
                        let text = lang.resolve(s);
                        if *text == **s { target.value.clone() } else { Value::str(&text) }
                    }
                    (v, _) => v.clone(),
                };
                ResNode { name: c.name.clone(), value, children: self.resolved_children(target, is_widget, depth + 1) }
            })
            .collect()
    }

    /// The node at `path` under `from`, with references followed.
    /// TB: tb_node_tree.cpp TBNode::GetNodeFollowRef
    pub fn node(&self, from: &'a ResNode, path: &str) -> Option<&'a ResNode> {
        from.get(path).map(|n| self.follow(n))
    }

    /// The value at `path` under `from`, with references followed.
    pub fn value(&self, from: &'a ResNode, path: &str) -> Option<&'a Value> {
        self.node(from, path).map(|n| &n.value)
    }

    /// TB: tb_node_tree.cpp TBNode::GetValueInt
    pub fn int(&self, from: &'a ResNode, path: &str, default: i32) -> i32 {
        self.node(from, path).map_or(default, |n| n.value.as_int())
    }

    /// TB: tb_node_tree.cpp TBNode::GetValueFloat
    pub fn float(&self, from: &'a ResNode, path: &str, default: f32) -> f32 {
        self.node(from, path).map_or(default, |n| n.value.as_float())
    }

    /// The string at `path`, with references followed and a final `@key` (no `>`) looked
    /// up in the language. TB: tb_node_tree.cpp TBNode::GetValueString
    pub fn str<'s>(&self, from: &'a ResNode, path: &str, default: &'s str) -> Cow<'s, str>
    where
        'a: 's,
    {
        match self.node(from, path) {
            None => Cow::Borrowed(default),
            Some(n) => match (&n.value, self.lang) {
                (Value::Str(s), Some(lang)) => lang.resolve(s),
                (v, _) => v.as_str(),
            },
        }
    }

    /// The string at `path`, with references followed but no language lookup.
    /// TB: tb_node_tree.cpp TBNode::GetValueStringRaw
    pub fn str_raw<'s>(&self, from: &'a ResNode, path: &str, default: &'s str) -> Cow<'s, str>
    where
        'a: 's,
    {
        match self.node(from, path) {
            None => Cow::Borrowed(default),
            Some(n) => n.value.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_paths() {
        let mut root = ResNode::new("");
        root.get_or_create("a>b>c").value = Value::Int(3);
        assert_eq!(root.int("a>b>c", 0), 3);
        assert!(core::ptr::eq(root.get("").unwrap(), &root));
        assert_eq!(&*root.get("a>").unwrap().name, "a");
        assert!(root.get("a>x").is_none());
        assert_eq!(root.path_of("a>b>c"), Some(alloc::vec![0, 0, 0]));
        root.get_mut("a>b").unwrap().add(ResNode::with_value("d", 4));
        assert_eq!(root.int("a>b>d", 0), 4);
        assert_eq!(root.height(), 4);
        assert_eq!(root.count(), 5);
        // get_or_create does not duplicate existing nodes.
        root.get_or_create("a>b");
        assert_eq!(root.count(), 5);
    }
}
