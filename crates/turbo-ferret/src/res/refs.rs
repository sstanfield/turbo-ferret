//! Ref trees: named node trees that resources reference with `@tree>path`, plus the
//! reference following and `@if`/`@else` resolution that use them.
//!
//! TB: tb_node_ref_tree.h, tb_node_ref_tree.cpp (TBNodeRefTree). TB kept every tree in a
//! static list; here they are a value the caller owns and passes in (H-05). Trees are
//! keyed by the `TbId` of their name, so lookups match whole names (TB compared only a
//! prefix). Change listeners (`TBNodeRefTreeListener`) are deferred until a widget needs
//! them (port-design.md section 16).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::id::TbId;
use crate::services::ResourceLoader;
use crate::value::Value;

use super::node::ResNode;
use super::parser::{self, ParseCtx, ParseError};

/// A reference chain longer than this many hops is treated as a loop.
/// (TB marked visited nodes with a cycle id instead.)
pub const MAX_REF_HOPS: usize = 64;

/// The named ref trees. TB: TBNodeRefTree (one object per tree, in a static list).
#[derive(Clone, Debug, Default)]
pub struct RefTrees {
    trees: BTreeMap<TbId, ResNode>,
}

impl RefTrees {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set (or replace) the tree `name`. Returns the tree it replaced.
    pub fn set(&mut self, name: &str, tree: ResNode) -> Option<ResNode> {
        self.trees.insert(TbId::new(name), tree)
    }

    /// Parse `src` into the tree `name` (replacing it). Includes and conditions in `src`
    /// can use the trees already set. Warnings are dropped.
    /// TB: tb_node_ref_tree.h TBNodeRefTree::ReadData
    pub fn set_from_str(&mut self, name: &str, src: &str) -> Result<(), ParseError> {
        let tree = parser::parse(src, &mut ParseCtx::new("{data}").with_refs(self))?;
        self.set(name, tree);
        Ok(())
    }

    /// Load and parse `path` into the tree `name` (replacing it). Warnings are dropped.
    /// TB: tb_node_ref_tree.h TBNodeRefTree::ReadFile
    pub fn set_from_file(&mut self, name: &str, loader: &mut dyn ResourceLoader, path: &str) -> Result<(), ParseError> {
        let tree = parser::parse_file(&mut ParseCtx::new(path).with_loader(loader).with_refs(self))?;
        self.set(name, tree);
        Ok(())
    }

    /// TB: tb_node_ref_tree.cpp TBNodeRefTree::GetRefTree
    pub fn get(&self, name: &str) -> Option<&ResNode> {
        self.trees.get(&TbId::new(name))
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ResNode> {
        self.trees.get_mut(&TbId::new(name))
    }

    pub fn get_by_id(&self, id: TbId) -> Option<&ResNode> {
        self.trees.get(&id)
    }

    pub fn remove(&mut self, name: &str) -> Option<ResNode> {
        self.trees.remove(&TbId::new(name))
    }

    pub fn len(&self) -> usize {
        self.trees.len()
    }

    pub fn is_empty(&self) -> bool {
        self.trees.is_empty()
    }

    /// Set the value at `request` in the tree `tree`, creating the tree and any missing
    /// nodes. (TB also notified the tree's listeners here.)
    /// TB: tb_node_ref_tree.cpp TBNodeRefTree::SetValue
    pub fn set_value(&mut self, tree: &str, request: &str, value: Value) {
        let root = self.trees.entry(TbId::new(tree)).or_default();
        root.get_or_create(request).value = value;
    }

    /// The value at `request` in the tree `tree`, with references followed. `None` if the
    /// tree or the node does not exist. TB: tb_node_ref_tree.cpp TBNodeRefTree::GetValue
    pub fn value(&self, tree: &str, request: &str) -> Option<&Value> {
        let root = self.get(tree)?;
        let node = root.get(request)?;
        Some(&follow(Some(self), root, node).value)
    }

    /// The value a `"@tree>request"` string refers to, with references followed. `None` if
    /// the string is not a reference, or the reference is broken or loops.
    /// TB: tb_node_ref_tree.cpp TBNodeRefTree::GetValueFromTree
    pub fn value_from_tree(&self, request: &str) -> Option<&Value> {
        self.follow_value(&Value::str(request)).map(|n| &n.value)
    }

    /// The node a reference value (`"@tree>path"`) leads to, following further references
    /// from there. `None` if `value` is not a reference, or it is broken or loops.
    pub fn follow_value(&self, value: &Value) -> Option<&ResNode> {
        follow_from(Some(self), None, value)
    }

    /// Resolve a value that may be a `@tree>path` reference: the value it leads to, or
    /// `value` itself if it is not a reference or the reference is broken.
    pub fn resolve<'a>(&'a self, value: &'a Value) -> &'a Value {
        self.follow_value(value).map_or(value, |n| &n.value)
    }

    /// Follow `node`'s reference chain. `root` is the root of the tree `node` is in (for
    /// `@>` references). TB: tb_node_ref_tree.cpp TBNodeRefTree::FollowNodeRef
    pub fn follow<'a>(&'a self, root: &'a ResNode, node: &'a ResNode) -> &'a ResNode {
        follow(Some(self), root, node)
    }
}

/// Follow `start`'s reference chain to the final node. `root` is the root of the tree
/// `start` is in, used for `@>` references. A broken reference stops at the last node
/// reached; a loop returns `start`. TB: tb_node_ref_tree.cpp TBNodeRefTree::FollowNodeRef
pub(crate) fn follow<'a>(refs: Option<&'a RefTrees>, root: &'a ResNode, start: &'a ResNode) -> &'a ResNode {
    follow_from(refs, Some(root), &start.value).unwrap_or(start)
}

/// Follow the reference chain that starts at the value `value`, which lives in the tree
/// `root` (`None` for a value outside any tree: its `@>` references find nothing).
/// Returns the last node reached, or `None` if the first hop fails or the chain loops.
// TB: tb_node_ref_tree.cpp TBNodeRefTree::FollowNodeRef
pub(crate) fn follow_from<'a>(
    refs: Option<&'a RefTrees>,
    mut root: Option<&'a ResNode>,
    value: &Value,
) -> Option<&'a ResNode> {
    let mut reached: Option<&'a ResNode> = None;
    let mut value = value;
    // One more round than hops, so a chain of exactly MAX_REF_HOPS hops can finish.
    for _ in 0..=MAX_REF_HOPS {
        // Only strings starting with '@' and containing a '>' are references; "@key"
        // without '>' is a language string.
        let Value::Str(s) = value else { return reached };
        let Some((tree, request)) = s.strip_prefix('@').and_then(|r| r.split_once('>')) else {
            return reached;
        };
        let next_root = if tree.is_empty() {
            // "@>request": a look-up from the root of the current tree.
            root
        } else {
            refs.and_then(|r| r.get(tree))
        };
        let Some(next_root) = next_root else { return reached };
        let Some(next) = next_root.get(request) else { return reached };
        root = Some(next_root);
        reached = Some(next);
        value = &next.value;
    }
    None
}

/// Resolve `@if`/`@else` branches in the whole tree: a true branch's children replace
/// it, a false branch is removed. `@if` values follow references (`@if @tree>path`).
/// TB: tb_node_ref_tree.cpp TBNodeRefTree::ResolveConditions
pub(crate) fn resolve_conditions(root: &mut ResNode, refs: Option<&RefTrees>) {
    let mut path = Vec::new();
    resolve_level(root, &mut path, refs);
}

/// Resolve the conditions among the children of the node at `path` (and below).
// TB: tb_node_ref_tree.cpp TBNodeRefTree::ResolveConditions
fn resolve_level(root: &mut ResNode, path: &mut Vec<usize>, refs: Option<&RefTrees>) {
    let mut condition = false;
    let mut i = 0;
    loop {
        let parent = root.at(path);
        let Some(node) = parent.children.get(i) else { break };
        let is_if = &*node.name == "@if";
        if !is_if && &*node.name != "@else" {
            path.push(i);
            resolve_level(root, path, refs);
            path.pop();
            i += 1;
            continue;
        }
        condition = if is_if { follow(refs, root, node).value.as_int() != 0 } else { !condition };
        if condition {
            // Resolve the branch first, since its content is skipped once moved up.
            path.push(i);
            resolve_level(root, path, refs);
            path.pop();
            let parent = root.at_mut(path);
            let branch = parent.children.remove(i);
            let moved = branch.children.len();
            parent.children.splice(i..i, branch.children);
            i += moved;
        } else {
            root.at_mut(path).children.remove(i);
        }
    }
}
