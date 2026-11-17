//! Widget nodes: shared widget state, with per-type behaviour in `behaviour`.
//!
//! TB: tb_widgets.h (TBWidget's data members, split from its virtual methods).

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;

use crate::arena::WidgetId;
use crate::flags::flags;
use crate::font::FontDesc;
use crate::geom::Rect;
use crate::id::TbId;
use crate::layout::size::{Gravity, LayoutParams, PreferredSize, SizeConstraints, Visibility};
use crate::value::Value;
use crate::widget::{Widget, WidgetState};

use super::tree::Tree;

flags! {
    /// Per-node framework flags (H-14).
    pub struct NodeFlags: u16 {
        const FOCUSABLE = 1;
        const CLICK_BY_KEY = 2;
        const WANT_LONG_CLICK = 4;
        const IGNORE_INPUT = 8;
        const GROUP_ROOT = 16;
        const WINDOW = 32;
        /// A window that can be activated (TB: WINDOW_SETTINGS_CAN_ACTIVATE).
        const CAN_ACTIVATE = 64;
        const ACTIVE_WINDOW = 128;
        const DYING = 256;
        /// Removed from the tree; freed at the next flush. Public lookups fail.
        const DETACHED = 512;
        const NEEDS_ARRANGE = 1024;
        /// PRESSED was set by keyboard click (TB: has_key_pressed_state).
        const KEY_PRESSED = 2048;
        /// Children inflated into this node go to the bottom (TB: SetZInflate).
        const INFLATE_BOTTOM = 4096;
    }
}

pub struct Node {
    pub(crate) parent: Option<WidgetId>,
    /// Index 0 = bottom of z-order (painted first).
    pub(crate) children: Vec<WidgetId>,
    /// Name hash from `id:` (decision 7).
    pub(crate) id: TbId,
    pub(crate) group_id: TbId,
    /// Relative to the parent's content origin.
    pub(crate) rect: Rect,
    /// Background skin element.
    pub(crate) skin: TbId,
    /// The element last resolved for `skin` (strong overrides), TB: m_skin_bg_expected.
    pub(crate) skin_expected: TbId,
    /// `None` = inherit.
    pub(crate) font: Option<FontDesc>,
    pub(crate) opacity: f32,
    pub(crate) gravity: Gravity,
    pub(crate) visibility: Visibility,
    /// Explicit states (DISABLED, SELECTED, PRESSED...). Automatic states are computed.
    pub(crate) state: WidgetState,
    pub(crate) flags: NodeFlags,
    pub(crate) measure_cache: Option<(SizeConstraints, PreferredSize)>,
    pub(crate) type_name: &'static str,
    /// User slot (TB's `data`).
    pub data: Value,
    /// `None` only while one of its hooks runs (take-out).
    pub(crate) behaviour: Option<Box<dyn Widget>>,
}

impl Node {
    pub(crate) fn new(b: Box<dyn Widget>) -> Node {
        Node {
            parent: None,
            children: Vec::new(),
            id: TbId::NONE,
            group_id: TbId::NONE,
            rect: Rect::ZERO,
            skin: TbId::NONE,
            skin_expected: TbId::NONE,
            font: None,
            opacity: 1.0,
            gravity: Gravity::DEFAULT,
            visibility: Visibility::Visible,
            state: WidgetState::NONE,
            flags: NodeFlags::NONE,
            measure_cache: None,
            type_name: b.type_name(),
            data: Value::Null,
            behaviour: Some(b),
        }
    }

    pub(crate) fn detached(&self) -> bool {
        self.flags.contains(NodeFlags::DETACHED)
    }
}

/// A read-only view of a node.
#[derive(Copy, Clone)]
pub struct NodeRef<'a> {
    pub(crate) tree: &'a Tree,
    pub(crate) wid: WidgetId,
    pub(crate) node: &'a Node,
}

impl<'a> NodeRef<'a> {
    pub fn widget_id(&self) -> WidgetId {
        self.wid
    }
    /// The `id:` name.
    pub fn id(&self) -> TbId {
        self.node.id
    }
    pub fn group_id(&self) -> TbId {
        self.node.group_id
    }
    pub fn rect(&self) -> Rect {
        self.node.rect
    }
    pub fn parent(&self) -> Option<WidgetId> {
        self.node.parent
    }
    pub fn parent_ref(&self) -> Option<NodeRef<'a>> {
        self.node.parent.and_then(|p| self.tree.node_ref(p))
    }
    pub fn children(&self) -> &'a [WidgetId] {
        &self.node.children
    }
    pub fn child_refs(&self) -> impl Iterator<Item = NodeRef<'a>> + 'a {
        let tree = self.tree;
        self.node.children.iter().filter_map(move |c| tree.node_ref(*c))
    }
    pub fn first_child(&self) -> Option<WidgetId> {
        self.node.children.first().copied()
    }
    pub fn last_child(&self) -> Option<WidgetId> {
        self.node.children.last().copied()
    }
    pub fn state(&self) -> WidgetState {
        self.node.state
    }
    pub fn has_state(&self, s: WidgetState) -> bool {
        self.node.state.contains(s)
    }
    pub fn flags(&self) -> NodeFlags {
        self.node.flags
    }
    pub fn opacity(&self) -> f32 {
        self.node.opacity
    }
    pub fn visibility(&self) -> Visibility {
        self.node.visibility
    }
    pub fn gravity(&self) -> Gravity {
        self.node.gravity
    }
    pub fn skin(&self) -> TbId {
        self.node.skin
    }
    pub fn font(&self) -> Option<FontDesc> {
        self.node.font
    }
    pub fn data(&self) -> &'a Value {
        &self.node.data
    }
    pub fn type_name(&self) -> &'static str {
        self.node.type_name
    }
    pub fn layout_params(&self) -> Option<&'a LayoutParams> {
        self.tree.layout_params.get(self.wid)
    }
    pub fn is_focusable(&self) -> bool {
        self.node.flags.contains(NodeFlags::FOCUSABLE)
    }
    pub fn is_window(&self) -> bool {
        self.node.flags.contains(NodeFlags::WINDOW)
    }
    pub fn is_active_window(&self) -> bool {
        self.node.flags.contains(NodeFlags::ACTIVE_WINDOW)
    }
    /// The behaviour, if not currently taken out by a running hook.
    pub fn behaviour(&self) -> Option<&'a dyn Widget> {
        self.node.behaviour.as_deref()
    }
    pub fn widget<T: Widget>(&self) -> Option<&'a T> {
        let b: &dyn Any = self.node.behaviour.as_deref()?;
        b.downcast_ref::<T>()
    }
    pub fn is<T: Widget>(&self) -> bool {
        self.widget::<T>().is_some()
    }
    /// This node or an ancestor is dying. TB: GetIsDying
    pub fn is_dying(&self) -> bool {
        self.tree.ancestors_or_self(self.wid).any(|n| n.flags.contains(NodeFlags::DYING))
    }
    /// This node or an ancestor is disabled. TB: GetDisabled
    pub fn is_disabled(&self) -> bool {
        self.tree.ancestors_or_self(self.wid).any(|n| n.state.contains(WidgetState::DISABLED))
    }
    /// Visible with non-zero opacity, and so are all ancestors. TB: GetVisibilityCombined
    pub fn is_visible_combined(&self) -> bool {
        self.tree.ancestors_or_self(self.wid).all(|n| n.opacity != 0.0 && n.visibility == Visibility::Visible)
    }
    /// TB: GetIsInteractable
    pub fn is_interactable(&self) -> bool {
        let n = self.node;
        !(n.opacity == 0.0
            || n.flags.contains(NodeFlags::IGNORE_INPUT)
            || n.state.contains(WidgetState::DISABLED)
            || self.is_dying()
            || n.visibility != Visibility::Visible)
    }
    pub fn is_ancestor_of(&self, other: WidgetId) -> bool {
        self.tree.is_ancestor_of(self.wid, other)
    }
    /// Get another node by id.
    pub fn get(&self, id: WidgetId) -> Option<NodeRef<'a>> {
        self.tree.node_ref(id)
    }
}
