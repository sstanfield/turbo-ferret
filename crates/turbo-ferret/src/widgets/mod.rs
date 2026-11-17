//! Built-in widgets, one file each.

pub mod layout;

pub use layout::Layout;

use crate::reader::Factory;
use crate::ui::{Ctx, Ui};
use crate::widget::Widget;

/// A plain widget with no skin (TB: TBWidget).
#[derive(Default)]
pub struct Plain;

impl Widget for Plain {
    fn type_name(&self) -> &'static str {
        "Widget"
    }
}

/// A plain widget with the "TBContainer" skin. TB: TBContainer
#[derive(Default)]
pub struct Container;

impl Widget for Container {
    fn type_name(&self) -> &'static str {
        "Container"
    }
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBContainer"));
    }
}

/// Register a factory under both its TB name ("TBLayout") and its short name ("Layout").
pub(crate) fn register_both(ui: &mut Ui, tb_name: &'static str, short: &'static str, f: Factory) {
    let mut a = f.clone();
    a.name = tb_name;
    ui.register_factory(a);
    let mut b = f;
    b.name = short;
    ui.register_factory(b);
}

/// Register every built-in widget (called by `Ui::new`, H-06).
pub(crate) fn register_builtins(ui: &mut Ui) {
    register_both(ui, "TBWidget", "Widget", Factory::of::<Plain>("Widget"));
    register_both(ui, "TBContainer", "Container", Factory::of::<Container>("Container"));
    register_both(ui, "TBLayout", "Layout", Factory::of::<Layout>("Layout"));
}
