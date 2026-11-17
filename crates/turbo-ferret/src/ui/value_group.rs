//! Value binding: widgets connected to a named value stay in sync (M-08).
//!
//! TB: tb_widget_value.cpp (TBWidgetValue, TBValueGroup, TBWidgetValueConnection).
//! `Changed` carries the new value, so no widget is ever read back while busy.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::arena::WidgetId;
use crate::id::TbId;
use crate::value::{Value, ValueKind};

use super::Ui;
use super::ctx::call_hook;

pub(crate) struct Bound {
    pub value: Value,
    pub kind: ValueKind,
    pub widgets: Vec<WidgetId>,
    pub syncing: bool,
}

impl Bound {
    fn new(kind: ValueKind) -> Bound {
        let value = match kind {
            ValueKind::Float => Value::Float(0.0),
            ValueKind::Str => Value::str(""),
            _ => Value::Int(0),
        };
        Bound { value, kind, widgets: Vec::new(), syncing: false }
    }
}

#[derive(Default)]
pub(crate) struct ValueGroup {
    pub values: BTreeMap<TbId, Bound>,
}

fn binding_kind(k: ValueKind) -> ValueKind {
    match k {
        ValueKind::Float => ValueKind::Float,
        ValueKind::Str => ValueKind::Str,
        _ => ValueKind::Int,
    }
}

impl Ui {
    /// Connect a widget to the named value (`connection:` in resources). The value is
    /// created on first use with the widget's value kind, and synced to the widget.
    /// TB: TBWidgetValueConnection::Connect
    pub fn connect_value(&mut self, id: WidgetId, name: TbId) {
        let kind = binding_kind(self.value(id).kind());
        self.connect_value_as(id, name, kind);
    }

    /// Connect with an explicit kind for a newly created value.
    pub fn connect_value_as(&mut self, id: WidgetId, name: TbId, kind: ValueKind) {
        if !self.tree.is_live(id) {
            return;
        }
        self.disconnect_value(id);
        self.tree.connections.insert(id, name);
        let b = self.values.values.entry(name).or_insert_with(|| Bound::new(binding_kind(kind)));
        b.widgets.push(id);
        self.sync_to_widget(name, id);
    }

    pub fn disconnect_value(&mut self, id: WidgetId) {
        if let Some(name) = self.tree.connections.remove(id) {
            if let Some(b) = self.values.values.get_mut(&name) {
                b.widgets.retain(|&w| w != id);
            }
        }
    }

    /// The value a widget is connected to, if any.
    pub fn connection(&self, id: WidgetId) -> Option<TbId> {
        self.tree.connections.get(id).copied()
    }

    pub fn bound_value(&self, name: TbId) -> Option<&Value> {
        self.values.values.get(&name).map(|b| &b.value)
    }

    /// Create a named value if it does not exist. TB: TBValueGroup::CreateValueIfNeeded
    pub fn create_bound_value(&mut self, name: TbId, kind: ValueKind) {
        self.values.values.entry(name).or_insert_with(|| Bound::new(binding_kind(kind)));
    }

    /// Set a named value and push it to every connected widget. TB: TBWidgetValue::SetInt...
    pub fn set_bound_value(&mut self, name: TbId, v: impl Into<Value>) {
        let v = v.into();
        let b = self.values.values.entry(name).or_insert_with(|| Bound::new(binding_kind(v.kind())));
        b.value = v.to_kind(b.kind);
        self.top(|ui| ui.sync_to_widgets(name, None));
    }

    /// TB: TBWidgetValue::SetFromWidget
    pub(crate) fn sync_from_widget(&mut self, src: WidgetId, v: Value) {
        let Some(name) = self.tree.connections.get(src).copied() else { return };
        let Some(b) = self.values.values.get_mut(&name) else { return };
        if b.syncing {
            return;
        }
        b.value = v.to_kind(b.kind);
        self.sync_to_widgets(name, Some(src));
    }

    fn sync_to_widgets(&mut self, name: TbId, exclude: Option<WidgetId>) {
        let Some(b) = self.values.values.get(&name) else { return };
        let ws: Vec<WidgetId> = b.widgets.clone();
        for w in ws {
            if Some(w) != exclude && self.tree.is_live(w) {
                self.sync_to_widget(name, w);
            }
        }
    }

    /// TB: TBWidgetValue::SyncToWidget
    fn sync_to_widget(&mut self, name: TbId, w: WidgetId) {
        let Some(b) = self.values.values.get_mut(&name) else { return };
        if b.syncing {
            return;
        }
        b.syncing = true;
        let v = b.value.clone();
        let kind = b.kind;
        call_hook(self, w, |wid, cx| {
            if kind == ValueKind::Str {
                wid.set_text(cx, &v.as_str());
            } else {
                wid.set_value(cx, &v);
            }
        });
        if let Some(b) = self.values.values.get_mut(&name) {
            b.syncing = false;
        }
    }
}
