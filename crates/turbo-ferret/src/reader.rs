//! Widget factories and inflation from TB resource files (port-design.md section 16).
//!
//! TB: tb_widgets_reader.cpp (TBWidgetsReader, TB_WIDGET_FACTORY, TBWidget::OnInflate).

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;

use crate::arena::WidgetId;
use crate::font::FontDesc;
use crate::geom::Rect;
use crate::id::TbId;
use crate::lang::Language;
use crate::layout::size::{Gravity, LayoutParams, Visibility};
use crate::res::{Lookup, ParseCtx, ParseError, RefTrees, ResNode};
use crate::services::Level;
use crate::ui::{NodeFlags, Ui, call_hook};
use crate::value::{Value, ValueKind};
use crate::widget::{Axis, Widget, WidgetState, Z};

/// How to create a widget type from resources.
#[derive(Clone)]
pub struct Factory {
    pub name: &'static str,
    pub create: fn() -> Box<dyn Widget>,
    /// Which value kind `value:` and `connection:` use (TB: sync_type).
    pub sync: ValueKind,
    /// Where children inflated into this widget's content root go (TB: add_child_z).
    pub inflate_z: Z,
}

impl Factory {
    pub fn of<W: Widget + Default>(name: &'static str) -> Factory {
        Factory { name, create: || Box::new(W::default()), sync: ValueKind::Null, inflate_z: Z::Top }
    }
    pub fn sync(mut self, k: ValueKind) -> Factory {
        self.sync = k;
        self
    }
    pub fn inflate_z(mut self, z: Z) -> Factory {
        self.inflate_z = z;
        self
    }
}

#[derive(Default)]
pub(crate) struct Registry {
    pub map: BTreeMap<TbId, Factory>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InflateError {
    Parse(ParseError),
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InflateError::Parse(e) => write!(f, "{e}"),
        }
    }
}

impl From<ParseError> for InflateError {
    fn from(e: ParseError) -> Self {
        InflateError::Parse(e)
    }
}

/// Nodes whose names start with an uppercase letter may be widgets; others are properties.
fn is_widget_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(|c| c.is_ascii_uppercase())
}

/// TB: TBWidgetsReader::SetIDFromNode
pub fn id_from_value(v: &Value) -> TbId {
    match v {
        Value::Str(s) => TbId::new(s),
        Value::Int(i) => TbId(*i as u32),
        Value::Float(f) => TbId(*f as u32),
        _ => TbId::NONE,
    }
}

struct Doc<'r> {
    root: &'r ResNode,
    file: &'r str,
}

impl Ui {
    /// Register a widget type for inflation (`name` is the resource name).
    pub fn register<W: Widget + Default>(&mut self, name: &'static str) {
        self.register_factory(Factory::of::<W>(name));
    }

    pub fn register_factory(&mut self, f: Factory) {
        self.registry.map.insert(TbId::new(f.name), f);
    }

    pub fn factory(&self, name: &str) -> Option<&Factory> {
        self.registry.map.get(&TbId::new(name))
    }

    pub fn lang(&self) -> &Language {
        &self.lang
    }

    pub fn lang_mut(&mut self) -> &mut Language {
        &mut self.lang
    }

    /// Load a language file additively.
    pub fn load_language(&mut self, path: &str) -> Result<(), ParseError> {
        self.lang.load(&mut *self.services.loader, path)
    }

    pub fn refs(&self) -> &RefTrees {
        &self.refs
    }

    /// Named resource trees for `@tree>path` references and `@include`.
    pub fn refs_mut(&mut self) -> &mut RefTrees {
        &mut self.refs
    }

    /// Parse a resource file through the loader (with the ref trees).
    pub fn load_resource(&mut self, path: &str) -> Result<ResNode, ParseError> {
        ResNode::load(&mut *self.services.loader, path, Some(&self.refs))
    }

    /// Create widgets from resource text into `parent` (its content root).
    pub fn inflate(&mut self, parent: WidgetId, src: &str) -> Result<Vec<WidgetId>, InflateError> {
        let mut cx = ParseCtx::new("<inline>").with_loader(&mut *self.services.loader).with_refs(&self.refs);
        let node = crate::res::parse(src, &mut cx)?;
        self.inflate_doc(parent, &node, "<inline>")
    }

    /// Create widgets from a resource file into `parent`.
    pub fn inflate_file(&mut self, parent: WidgetId, path: &str) -> Result<Vec<WidgetId>, InflateError> {
        let node = ResNode::load(&mut *self.services.loader, path, Some(&self.refs))?;
        self.inflate_doc(parent, &node, path)
    }

    /// Create widgets from the children of an already parsed node.
    /// TB: TBWidgetsReader::LoadNodeTree
    pub fn inflate_node(&mut self, parent: WidgetId, node: &ResNode) -> Result<Vec<WidgetId>, InflateError> {
        self.inflate_doc(parent, node, "<node>")
    }

    /// Apply `node`'s properties to an existing widget: its own `on_inflate`, then the
    /// generic ones (id, skin, gravity, `lp` ...) and the font. For widgets with
    /// internal parts that resources can configure, like a tab container's layouts.
    /// TB: calling OnInflate on an existing widget
    pub fn inflate_properties(&mut self, w: WidgetId, node: &ResNode) {
        let doc = Doc { root: node, file: "<node>" };
        self.top(|ui| {
            call_hook(ui, w, |b, cx| b.on_inflate(cx, node));
            ui.apply_generic(&doc, w, node, ValueKind::Null);
            ui.apply_font(&doc, w, node);
        });
    }

    fn inflate_doc(&mut self, parent: WidgetId, node: &ResNode, file: &str) -> Result<Vec<WidgetId>, InflateError> {
        let doc = Doc { root: node, file };
        self.top(|ui| {
            let mut out = Vec::new();
            for child in node.children() {
                if let Some(w) = ui.create_from_node(&doc, parent, child)? {
                    out.push(w);
                }
            }
            Ok(out)
        })
    }

    /// TB: TBWidgetsReader::CreateWidget
    fn create_from_node(
        &mut self,
        doc: &Doc,
        target: WidgetId,
        node: &ResNode,
    ) -> Result<Option<WidgetId>, InflateError> {
        if !is_widget_name(&node.name) {
            return Ok(None);
        }
        let Some(f) = self.registry.map.get(&TbId::new(&node.name)).cloned() else {
            // TB skips nodes without a factory (resources also hold data such as
            // "WindowInfo"); a capitalized name is probably a missing factory, so warn.
            self.log(Level::Warn, format_args!("{}: no widget factory for \"{}\"", doc.file, node.name));
            return Ok(None);
        };
        let target = self.content_root(target);
        let w = self.create_boxed((f.create)());
        // Children inflated into this widget's content root go at the factory's z.
        let cr = self.content_root(w);
        if f.inflate_z == Z::Bottom {
            if let Some(n) = self.tree.live_mut(cr) {
                n.flags.insert(NodeFlags::INFLATE_BOTTOM);
            }
        }
        // Custom properties first (TB: subclass OnInflate before TBWidget::OnInflate).
        call_hook(self, w, |b, cx| b.on_inflate(cx, node));
        self.apply_generic(doc, w, node, f.sync);
        if self.tree.parent(w).is_none() {
            let z = if self.has_flag(target, NodeFlags::INFLATE_BOTTOM) { Z::Bottom } else { Z::Top };
            self.attach(target, w, z);
        }
        self.apply_font(doc, w, node);
        for child in node.children() {
            self.create_from_node(doc, w, child)?;
        }
        // TB: OnInflateChild sizes the child to its preferred size within the padding
        // rect (after its own children exist, so the size includes them).
        let rect = self.lookup(doc).value(node, "rect").cloned();
        match rect.as_ref().and_then(|v| v.as_array()) {
            Some(a) if a.len() == 4 => {
                let d = &self.dim;
                let r = Rect::new(
                    d.px_from_value(&a[0], 0),
                    d.px_from_value(&a[1], 0),
                    d.px_from_value(&a[2], 0),
                    d.px_from_value(&a[3], 0),
                );
                self.set_rect(w, r);
            }
            _ => {
                if self.tree.parent(w) == Some(target) && self.rect(w).is_empty() {
                    self.place_child(target, w);
                }
            }
        }
        if self.lookup(doc).int(node, "autofocus", 0) != 0 {
            self.set_focus(w);
        }
        Ok(Some(w))
    }

    fn lookup<'a>(&'a self, doc: &'a Doc) -> Lookup<'a> {
        Lookup::new(doc.root).with_refs(&self.refs).with_lang(&self.lang)
    }

    /// TB: TBWidget::OnInflate (generic properties).
    fn apply_generic(&mut self, doc: &Doc, w: WidgetId, node: &ResNode, sync: ValueKind) {
        #[cfg(feature = "debug")]
        {
            // Remember what the ids were called, for the debug window.
            let lk = self.lookup(doc);
            let names: Vec<alloc::string::String> = ["id", "group-id", "skin"]
                .iter()
                .filter_map(|key| match lk.value(node, key) {
                    Some(Value::Str(s)) => Some(alloc::string::String::from(&**s)),
                    _ => None,
                })
                .collect();
            for name in names {
                self.name_id(&name);
            }
        }
        let lk = self.lookup(doc);
        let id = lk.value(node, "id").map(id_from_value);
        let group = lk.value(node, "group-id").map(id_from_value);
        let value = lk.value(node, "value").cloned();
        let data = lk.value(node, "data").cloned();
        let flag = |name: &str| lk.value(node, name).map(|v| v.as_int() != 0);
        let group_root = flag("is-group-root");
        let focusable = flag("is-focusable");
        let long_click = flag("want-long-click");
        let ignore = flag("ignore-input");
        let opacity = lk.value(node, "opacity").map(|v| v.as_float());
        let text = lk.node(node, "text").map(|_| lk.str(node, "text", "").into_owned());
        let connection = lk.node(node, "connection").map(|_| lk.str_raw(node, "connection", "").into_owned());
        let axis = lk.node(node, "axis").map(|_| Axis::parse(&lk.str(node, "axis", "x")));
        let gravity = lk.node(node, "gravity").map(|_| Gravity::parse(&lk.str(node, "gravity", "")));
        let visibility = lk.node(node, "visibility").map(|_| Visibility::parse(&lk.str(node, "visibility", "")));
        let disabled = lk.node(node, "state").map(|_| lk.str(node, "state", "").contains("disabled"));
        let skin = lk.node(node, "skin").map(|_| TbId::new(&lk.str(node, "skin", "")));
        let lp = lk.node(node, "lp").map(|lpn| {
            let mut lp = self.tree.layout_params.get(w).copied().unwrap_or_default();
            let px = |name: &str| lk.node(lpn, name).map(|_| self.dim.px_from_str(&lk.str(lpn, name, ""), 0));
            if let Some(v) = px("width") {
                lp.set_width(v);
            }
            if let Some(v) = px("height") {
                lp.set_height(v);
            }
            if let Some(v) = px("min-width") {
                lp.min_w = Some(v);
            }
            if let Some(v) = px("max-width") {
                lp.max_w = Some(v);
            }
            if let Some(v) = px("pref-width") {
                lp.pref_w = Some(v);
            }
            if let Some(v) = px("min-height") {
                lp.min_h = Some(v);
            }
            if let Some(v) = px("max-height") {
                lp.max_h = Some(v);
            }
            if let Some(v) = px("pref-height") {
                lp.pref_h = Some(v);
            }
            lp
        });

        if let Some(id) = id {
            self.set_id(w, id);
        }
        if let Some(g) = group {
            self.set_group_id(w, g);
        }
        if let Some(v) = value {
            let v = if sync == ValueKind::Float { Value::Float(v.as_float()) } else { Value::Int(v.as_int()) };
            self.set_value(w, v);
        }
        if let Some(d) = data {
            self.set_data(w, d);
        }
        if let Some(b) = group_root {
            self.set_group_root(w, b);
        }
        if let Some(b) = focusable {
            self.set_focusable(w, b);
        }
        if let Some(b) = long_click {
            self.set_want_long_click(w, b);
        }
        if let Some(b) = ignore {
            self.set_ignore_input(w, b);
        }
        if let Some(o) = opacity {
            self.set_opacity(w, o);
        }
        if let Some(t) = text {
            self.set_text(w, &t);
        }
        if let Some(c) = connection {
            let name = TbId::new(&c);
            if self.bound_value(name).is_some() {
                self.connect_value(w, name);
            } else {
                // TB: a new value takes the widget's current value.
                let kind = match sync {
                    ValueKind::Null => ValueKind::Int,
                    k => k,
                };
                self.create_bound_value(name, kind);
                let v = self.value(w);
                self.set_bound_value(name, v);
                self.connect_value_as(w, name, kind);
            }
        }
        if let Some(a) = axis {
            self.with_behaviour(w, |b, cx| b.set_axis(cx, a));
            self.skin_dirty = true;
            self.invalidate_layout(w);
        }
        if let Some(g) = gravity {
            self.set_gravity(w, g);
        }
        if let Some(v) = visibility {
            self.set_visibility(w, v);
        }
        if disabled == Some(true) {
            self.set_state(w, WidgetState::DISABLED, true);
        }
        if let Some(s) = skin {
            self.set_skin(w, s);
        }
        if let Some(lp) = lp {
            self.set_layout_params(w, Some(lp));
        }
    }

    /// TB: TBWidget::OnInflate (font part)
    fn apply_font(&mut self, doc: &Doc, w: WidgetId, node: &ResNode) {
        let lk = self.lookup(doc);
        let Some(fnode) = lk.node(node, "font") else { return };
        let mut fd: FontDesc = self.calculated_font(w);
        if lk.node(fnode, "size").is_some() {
            let s = lk.str(fnode, "size", "");
            fd.size = self.dim.px_from_str(&s, fd.size as i32).clamp(1, u16::MAX as i32) as u16;
        }
        if lk.node(fnode, "name").is_some() {
            fd.id = TbId::new(&lk.str(fnode, "name", ""));
        }
        self.set_font(w, Some(fd));
    }
}

/// Convenience for widgets' `on_inflate`: dimension strings to pixels.
impl Ui {
    pub fn px(&self, s: &str, default: i32) -> i32 {
        self.dim.px_from_str(s, default)
    }

    pub fn layout_params_or_default(&self, id: WidgetId) -> LayoutParams {
        self.layout_params(id).unwrap_or_default()
    }
}
