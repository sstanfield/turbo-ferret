//! The `Ui` context: every piece of engine state, and the public API
//! (port-design.md section 5). One `Ui`, no globals (H-05).

mod ctx;
mod dispatch;
mod focus;
mod glue;
mod input;
mod listen;
mod node;
mod ops;
mod process;
mod text;
pub(crate) mod tree;
mod value_group;
mod view;

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt;

pub use ctx::Ctx;
pub(crate) use ctx::call_hook;
pub use focus::FocusReason;
pub use input::InputState;
pub use listen::{ListenerId, UiListener};
pub use node::{Node, NodeFlags, NodeRef};
pub use view::View;

use crate::arena::WidgetId;
use crate::font::FontDesc;
use crate::geom::{Edges, Point, Rect, Size};
use crate::id::TbId;
use crate::layout::size::{Gravity, LayoutParams, Visibility};
use crate::msg::{Message, MessageQueue, Millis};
use crate::services::{Level, Services};
use crate::value::Value;
use crate::widget::{Widget, WidgetState};

use crate::lang::Language;
use crate::layout::dim::DimensionConverter;
use crate::reader::Registry;
use crate::res::RefTrees;
use listen::Listeners;
use tree::Tree;
use value_group::ValueGroup;

/// Platform metrics and tunables. TB: TBSystem::GetDPI, GetLongClickDelayMS,
/// GetPanThreshold, GetPixelsPerLine.
#[derive(Clone, Debug)]
pub struct Config {
    pub dpi: i32,
    pub long_click_delay_ms: u32,
    /// Pixels the pointer must move before panning starts.
    pub pan_threshold: i32,
    /// Pixels to scroll per wheel line.
    pub pixels_per_line: i32,
}

impl Default for Config {
    fn default() -> Self {
        Config::for_dpi(96)
    }
}

impl Config {
    pub fn for_dpi(dpi: i32) -> Config {
        Config { dpi, long_click_delay_ms: 500, pan_threshold: 5 * dpi / 96, pixels_per_line: 40 * dpi / 96 }
    }
}

/// The root node's behaviour.
#[derive(Default)]
pub struct Root;
impl Widget for Root {
    fn type_name(&self) -> &'static str {
        "Root"
    }
}

#[derive(Default)]
pub(crate) struct Pending {
    /// Detached subtrees waiting to be freed.
    pub free: Vec<WidgetId>,
    /// Widgets that started dying inside their own hook.
    pub die_when_free: Vec<WidgetId>,
}

pub struct Ui {
    pub(crate) tree: Tree,
    pub(crate) input: InputState,
    pub(crate) values: ValueGroup,
    pub(crate) msgs: MessageQueue,
    pub(crate) listeners: Listeners,
    pub(crate) services: Services,
    pub(crate) config: Config,
    pub(crate) now: Millis,
    pub(crate) pending: Pending,
    /// Nesting depth of hooks and top-level calls; flush happens when it returns to 0.
    pub(crate) depth: u32,
    pub(crate) repaint: bool,
    /// Widget states need `on_process_states` (TB: update_widget_states).
    pub(crate) states_dirty: bool,
    /// Skin elements need re-resolving for strong overrides (TB: update_skin_states).
    pub(crate) skin_dirty: bool,
    pub(crate) viewport: Size,
    pub(crate) default_font: FontDesc,
    /// Reusable buffer for snapshot walks.
    pub(crate) scratch: Vec<WidgetId>,
    pub(crate) registry: Registry,
    pub(crate) lang: Language,
    pub(crate) refs: RefTrees,
    /// Dimension conversion (set from the skin at M3).
    pub(crate) dim: DimensionConverter,
    pub(crate) skin: crate::skin::Skin,
    pub(crate) anims: crate::anim::Animations,
    pub(crate) fonts: crate::font::FontManager,
}

impl Ui {
    pub fn new(config: Config, services: Services) -> Ui {
        let root = Node::new(Box::new(Root));
        let mut ui = Ui {
            tree: Tree::new(root),
            input: InputState::default(),
            values: ValueGroup::default(),
            msgs: MessageQueue::default(),
            listeners: Listeners::default(),
            services,
            config,
            now: 0,
            pending: Pending::default(),
            depth: 0,
            repaint: true,
            states_dirty: true,
            skin_dirty: true,
            viewport: Size::ZERO,
            default_font: FontDesc::default(),
            scratch: Vec::new(),
            registry: Registry::default(),
            lang: Language::default(),
            refs: RefTrees::default(),
            dim: DimensionConverter::new(96, 96, 96),
            skin: crate::skin::Skin::default(),
            anims: Default::default(),
            fonts: crate::font::FontManager::new(),
        };
        crate::widgets::register_builtins(&mut ui);
        ui
    }

    pub fn dimension_converter(&self) -> &DimensionConverter {
        &self.dim
    }

    pub fn set_dimension_converter(&mut self, d: DimensionConverter) {
        self.dim = d;
    }

    // ------------------------------------------------------------------
    // Basics
    // ------------------------------------------------------------------

    pub fn root(&self) -> WidgetId {
        self.tree.root
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn services_mut(&mut self) -> &mut Services {
        &mut self.services
    }

    /// Size the root node to the viewport.
    pub fn set_viewport(&mut self, size: Size) {
        self.viewport = size;
        let root = self.root();
        self.set_rect(root, Rect::from_size(size));
    }

    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// Set the current time (any monotonic milliseconds). Time is data (section 10).
    pub fn set_time(&mut self, now: Millis) {
        self.now = now;
    }

    pub fn now(&self) -> Millis {
        self.now
    }

    /// Returns and clears the repaint flag.
    pub fn take_repaint(&mut self) -> bool {
        core::mem::take(&mut self.repaint)
    }

    pub fn needs_repaint(&self) -> bool {
        self.repaint
    }

    /// Request a repaint. TB: Invalidate
    pub fn invalidate(&mut self) {
        self.repaint = true;
    }

    pub(crate) fn log(&self, level: Level, args: fmt::Arguments<'_>) {
        self.services.log(level, args);
    }

    pub(crate) fn warn_busy(&self, id: WidgetId) {
        if cfg!(debug_assertions) {
            let name = self.tree.nodes.get(id).map(|n| n.type_name).unwrap_or("?");
            self.log(Level::Debug, format_args!("hook skipped: {name} {id:?} is busy (re-entrant call)"));
        }
    }

    /// Run `f` as a top-level call: frees pending subtrees at the end if not nested.
    pub(crate) fn top<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.depth += 1;
        let r = f(self);
        self.depth -= 1;
        if self.depth == 0 {
            self.flush();
        }
        r
    }

    // ------------------------------------------------------------------
    // Lookup
    // ------------------------------------------------------------------

    /// A read-only view of a node. `None` if the id is stale or the node was removed.
    pub fn node(&self, id: WidgetId) -> Option<NodeRef<'_>> {
        self.tree.node_ref(id)
    }

    pub fn exists(&self, id: WidgetId) -> bool {
        self.tree.is_live(id)
    }

    /// Typed access to a behaviour. Returns `None` inside the widget's own hooks
    /// (the behaviour is taken out; use `self` there).
    pub fn get<T: Widget>(&self, id: WidgetId) -> Option<&T> {
        let b: &dyn Any = self.tree.live(id)?.behaviour.as_deref()?;
        b.downcast_ref::<T>()
    }

    /// Plain mutable borrow of a behaviour, for setting fields. No side effects; use
    /// `with_widget` for anything that needs the `Ui`.
    pub fn get_mut<T: Widget>(&mut self, id: WidgetId) -> Option<&mut T> {
        let b: &mut dyn Any = self.tree.live_mut(id)?.behaviour.as_deref_mut()?;
        b.downcast_mut::<T>()
    }

    /// Take the behaviour out and call `f` with it and a `Ctx` (section 7).
    pub fn with_widget<T: Widget, R>(&mut self, id: WidgetId, f: impl FnOnce(&mut T, &mut Ctx) -> R) -> Option<R> {
        if !self.tree.is_live(id) {
            return None;
        }
        call_hook(self, id, |w, cx| {
            let any: &mut dyn Any = w;
            any.downcast_mut::<T>().map(|t| f(t, cx))
        })
        .flatten()
    }

    /// Call `f` with the untyped behaviour.
    pub fn with_behaviour<R>(&mut self, id: WidgetId, f: impl FnOnce(&mut dyn Widget, &mut Ctx) -> R) -> Option<R> {
        if !self.tree.is_live(id) {
            return None;
        }
        call_hook(self, id, f)
    }

    /// Depth-first search of `root`'s subtree for a name. TB: GetWidgetByID
    pub fn find(&self, root: WidgetId, name: TbId) -> Option<WidgetId> {
        self.tree.find(root, name)
    }

    /// Find by name and type. TB: GetWidgetByIDAndType
    pub fn find_as<T: Widget>(&self, root: WidgetId, name: TbId) -> Option<WidgetId> {
        self.find_where(root, &|n| n.id() == name && n.is::<T>())
    }

    fn find_where(&self, root: WidgetId, pred: &dyn Fn(NodeRef<'_>) -> bool) -> Option<WidgetId> {
        let n = self.node(root)?;
        if pred(n) {
            return Some(root);
        }
        n.children().iter().find_map(|&c| self.find_where(c, pred))
    }

    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.tree.parent(id)
    }

    pub fn children(&self, id: WidgetId) -> &[WidgetId] {
        self.tree.live(id).map(|n| n.children.as_slice()).unwrap_or(&[])
    }

    /// Where children of `id` go (follows `Widget::content_root`). TB: GetContentRoot
    pub fn content_root(&self, id: WidgetId) -> WidgetId {
        let mut cur = id;
        for _ in 0..16 {
            match self.tree.live(cur).and_then(|n| n.behaviour.as_deref()).and_then(|b| b.content_root()) {
                Some(c) if c != cur && self.tree.is_live(c) => cur = c,
                _ => break,
            }
        }
        cur
    }

    /// Nearest window containing `id` (self included).
    pub fn parent_window(&self, id: WidgetId) -> Option<WidgetId> {
        self.tree.parent_window(id)
    }

    pub fn is_ancestor_of(&self, a: WidgetId, b: WidgetId) -> bool {
        self.tree.is_ancestor_of(a, b)
    }

    // ------------------------------------------------------------------
    // Node properties
    // ------------------------------------------------------------------

    pub fn rect(&self, id: WidgetId) -> Rect {
        self.tree.live(id).map(|n| n.rect).unwrap_or_default()
    }

    /// Set the rect. If the size changed, `arrange(old, new)` runs immediately.
    /// TB: SetRect / OnResized
    pub fn set_rect(&mut self, id: WidgetId, r: Rect) {
        let Some(n) = self.tree.live_mut(id) else { return };
        if n.rect == r {
            return;
        }
        let old = n.rect;
        n.rect = r;
        self.repaint = true;
        if old.size() != r.size() {
            call_hook(self, id, |w, cx| w.arrange(cx, old.size(), r.size()));
        }
    }

    pub fn set_position(&mut self, id: WidgetId, p: Point) {
        let r = self.rect(id);
        self.set_rect(id, Rect::from_origin_size(p, r.size()));
    }

    /// TB: SetID
    pub fn set_id(&mut self, id: WidgetId, name: TbId) {
        if let Some(n) = self.tree.live_mut(id) {
            n.id = name;
            self.skin_dirty = true;
        }
    }

    pub fn set_group_id(&mut self, id: WidgetId, g: TbId) {
        if let Some(n) = self.tree.live_mut(id) {
            n.group_id = g;
        }
    }

    pub fn state(&self, id: WidgetId) -> WidgetState {
        self.tree.live(id).map(|n| n.state).unwrap_or_default()
    }

    pub fn has_state(&self, id: WidgetId, s: WidgetState) -> bool {
        self.state(id).contains(s)
    }

    /// TB: SetStateRaw
    pub fn set_state_raw(&mut self, id: WidgetId, s: WidgetState) {
        let Some(n) = self.tree.live_mut(id) else { return };
        if n.state == s {
            return;
        }
        n.state = s;
        self.repaint = true;
        self.skin_dirty = true;
    }

    /// Turn state bits on or off. TB: SetState
    pub fn set_state(&mut self, id: WidgetId, s: WidgetState, on: bool) {
        let mut st = self.state(id);
        st.set(s, on);
        self.set_state_raw(id, st);
    }

    pub fn set_disabled(&mut self, id: WidgetId, disabled: bool) {
        self.set_state(id, WidgetState::DISABLED, disabled);
    }

    pub fn opacity(&self, id: WidgetId) -> f32 {
        self.tree.live(id).map(|n| n.opacity).unwrap_or(0.0)
    }

    /// TB: SetOpacity
    pub fn set_opacity(&mut self, id: WidgetId, o: f32) {
        let o = o.clamp(0.0, 1.0);
        if let Some(n) = self.tree.live_mut(id) {
            if n.opacity != o {
                n.opacity = o;
                self.repaint = true;
            }
        }
    }

    pub fn visibility(&self, id: WidgetId) -> Visibility {
        self.tree.live(id).map(|n| n.visibility).unwrap_or(Visibility::Gone)
    }

    /// TB: SetVisibility
    pub fn set_visibility(&mut self, id: WidgetId, v: Visibility) {
        let Some(n) = self.tree.live_mut(id) else { return };
        if n.visibility == v {
            return;
        }
        let old = n.visibility;
        n.visibility = v;
        self.repaint = true;
        if v == Visibility::Gone || old == Visibility::Gone {
            if let Some(p) = self.parent(id) {
                self.invalidate_layout(p);
            }
            self.invalidate_layout(id);
        }
        call_hook(self, id, |w, cx| w.on_visibility_changed(cx));
    }

    /// TB: SetGravity
    pub fn set_gravity(&mut self, id: WidgetId, g: Gravity) {
        let Some(n) = self.tree.live_mut(id) else { return };
        if n.gravity != g {
            n.gravity = g;
            self.invalidate_layout(id);
        }
    }

    pub fn gravity(&self, id: WidgetId) -> Gravity {
        self.tree.live(id).map(|n| n.gravity).unwrap_or_default()
    }

    pub fn skin(&self, id: WidgetId) -> TbId {
        self.tree.live(id).map(|n| n.skin).unwrap_or_default()
    }

    /// Set the background skin element. TB: SetSkinBg
    pub fn set_skin(&mut self, id: WidgetId, skin: TbId) {
        let Some(n) = self.tree.live_mut(id) else { return };
        if n.skin == skin {
            return;
        }
        n.skin = skin;
        n.skin_expected = skin;
        self.repaint = true;
        self.skin_dirty = true;
        self.invalidate_layout(id);
        call_hook(self, id, |w, cx| w.on_skin_changed(cx));
    }

    /// Set the skin without calling `on_skin_changed` (for use in `on_create`).
    pub fn set_skin_quiet(&mut self, id: WidgetId, skin: TbId) {
        if let Some(n) = self.tree.live_mut(id) {
            n.skin = skin;
            n.skin_expected = skin;
            self.skin_dirty = true;
        }
        self.invalidate_layout(id);
    }

    pub fn font(&self, id: WidgetId) -> Option<FontDesc> {
        self.tree.live(id).and_then(|n| n.font)
    }

    /// The font in effect: the nearest explicit one, else the default.
    /// TB: GetCalculatedFontDescription
    pub fn calculated_font(&self, id: WidgetId) -> FontDesc {
        self.tree.ancestors_or_self(id).find_map(|n| n.font).unwrap_or(self.default_font)
    }

    pub fn default_font(&self) -> FontDesc {
        self.default_font
    }

    /// TB: SetFontDescription
    pub fn set_font(&mut self, id: WidgetId, font: Option<FontDesc>) {
        let Some(n) = self.tree.live(id) else { return };
        if n.font == font {
            return;
        }
        if let Some(f) = font {
            self.create_font_face(f);
        }
        if let Some(n) = self.tree.live_mut(id) {
            n.font = font;
        }
        self.invoke_font_changed(id);
    }

    /// TB: InvokeFontChanged
    pub(crate) fn invoke_font_changed(&mut self, id: WidgetId) {
        call_hook(self, id, |w, cx| w.on_font_changed(cx));
        self.invalidate_layout(id);
        let kids: Vec<WidgetId> = self.children(id).to_vec();
        for c in kids {
            if self.tree.live(c).is_some_and(|n| n.font.is_none()) {
                self.invoke_font_changed(c);
            }
        }
    }

    pub fn data(&self, id: WidgetId) -> Value {
        self.tree.live(id).map(|n| n.data.clone()).unwrap_or_default()
    }

    pub fn set_data(&mut self, id: WidgetId, v: Value) {
        if let Some(n) = self.tree.live_mut(id) {
            n.data = v;
        }
    }

    fn set_flag(&mut self, id: WidgetId, f: NodeFlags, on: bool) {
        if let Some(n) = self.tree.live_mut(id) {
            n.flags.set(f, on);
        }
    }

    pub fn has_flag(&self, id: WidgetId, f: NodeFlags) -> bool {
        self.tree.live(id).is_some_and(|n| n.flags.contains(f))
    }

    pub fn set_focusable(&mut self, id: WidgetId, on: bool) {
        self.set_flag(id, NodeFlags::FOCUSABLE, on);
    }
    pub fn set_click_by_key(&mut self, id: WidgetId, on: bool) {
        self.set_flag(id, NodeFlags::CLICK_BY_KEY, on);
    }
    pub fn set_want_long_click(&mut self, id: WidgetId, on: bool) {
        self.set_flag(id, NodeFlags::WANT_LONG_CLICK, on);
    }
    pub fn set_ignore_input(&mut self, id: WidgetId, on: bool) {
        self.set_flag(id, NodeFlags::IGNORE_INPUT, on);
    }
    pub fn set_group_root(&mut self, id: WidgetId, on: bool) {
        self.set_flag(id, NodeFlags::GROUP_ROOT, on);
    }

    /// Mark a node as a window: it activates on pointer down, keeps its last focus,
    /// and bounds Tab navigation. `can_activate`: TB WINDOW_SETTINGS_CAN_ACTIVATE.
    pub fn set_window(&mut self, id: WidgetId, is_window: bool, can_activate: bool) {
        self.set_flag(id, NodeFlags::WINDOW, is_window);
        self.set_flag(id, NodeFlags::CAN_ACTIVATE, can_activate);
    }

    pub fn layout_params(&self, id: WidgetId) -> Option<LayoutParams> {
        self.tree.layout_params.get(id).copied()
    }

    /// TB: SetLayoutParams
    pub fn set_layout_params(&mut self, id: WidgetId, lp: Option<LayoutParams>) {
        if !self.tree.is_live(id) {
            return;
        }
        match lp {
            Some(lp) => {
                self.tree.layout_params.insert(id, lp);
            }
            None => {
                self.tree.layout_params.remove(id);
            }
        }
        self.invalidate_layout(id);
    }

    /// The skin padding plus the widget's extra padding (M3 adds the skin part).
    pub fn padding(&self, id: WidgetId) -> Edges {
        self.view().padding(id)
    }

    /// The content area inside padding, in the node's local coordinates. TB: GetPaddingRect
    pub fn padding_rect(&self, id: WidgetId) -> Rect {
        Rect::from_size(self.rect(id).size()).shrink_edges(self.padding(id))
    }

    // ------------------------------------------------------------------
    // Generic properties (H-07)
    // ------------------------------------------------------------------

    pub fn value(&self, id: WidgetId) -> Value {
        self.tree.live(id).and_then(|n| n.behaviour.as_deref()).map(|b| b.value()).unwrap_or_default()
    }

    pub fn set_value(&mut self, id: WidgetId, v: impl Into<Value>) {
        let v = v.into();
        self.with_behaviour(id, |w, cx| w.set_value(cx, &v));
    }

    pub fn text(&self, id: WidgetId) -> Option<&str> {
        self.tree.live(id)?.behaviour.as_deref()?.text()
    }

    pub fn set_text(&mut self, id: WidgetId, text: &str) {
        self.with_behaviour(id, |w, cx| w.set_text(cx, text));
    }

    /// Text of the first widget named `name` under `root`. TB: GetTextByID
    pub fn text_by_name(&self, root: WidgetId, name: TbId) -> Option<&str> {
        self.text(self.find(root, name)?)
    }

    /// Value of the first widget named `name` under `root`. TB: GetValueByID
    pub fn value_by_name(&self, root: WidgetId, name: TbId) -> Value {
        self.find(root, name).map(|w| self.value(w)).unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Messages (section 10)
    // ------------------------------------------------------------------

    /// Deliver `id` to `target` on the next `update`.
    pub fn post_message(&mut self, target: WidgetId, id: TbId, data: Value) {
        self.msgs.post(target, Message { id, data });
    }

    pub fn post_message_delayed(&mut self, target: WidgetId, id: TbId, data: Value, delay_ms: u32) {
        let at = self.now + delay_ms as Millis;
        self.msgs.post_at(target, Message { id, data }, at);
    }

    pub fn post_message_at(&mut self, target: WidgetId, id: TbId, data: Value, at: Millis) {
        self.msgs.post_at(target, Message { id, data }, at);
    }

    /// Cancel queued messages for `target` (all, or only `id`).
    pub fn cancel_messages(&mut self, target: WidgetId, id: Option<TbId>) {
        self.msgs.cancel(target, id);
    }

    pub fn has_message(&self, target: WidgetId, id: TbId) -> bool {
        self.msgs.has(target, id)
    }

    // ------------------------------------------------------------------
    // Misc
    // ------------------------------------------------------------------

    /// Wrap a payload for `Event::payload`.
    pub fn payload<T: Any>(v: T) -> Rc<dyn Any> {
        Rc::new(v)
    }
}
