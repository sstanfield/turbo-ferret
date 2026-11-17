//! Built-in widgets, one file each.

pub mod button;
pub mod edit_field;
pub mod inline_select;
pub mod layout;
pub mod message_window;
pub mod misc;
pub mod popup;
pub mod scroll;
pub mod scroll_container;
pub mod select;
pub mod select_item;
pub mod tab_container;
pub mod text;
pub mod toggle;
pub mod window;

pub use button::{Button, ClickLabel};
pub use edit_field::{EditField, EditFieldScrollRoot, EditType};
pub use inline_select::InlineSelect;
pub use layout::Layout;
pub use message_window::{MessageButtons, MessageSettings, MessageWindow};
pub use misc::{CheckBox, Dimmer, Mover, ProgressSpinner, RadioButton, Resizer, Separator, SkinImage};
pub use popup::{MenuWindow, PopupAlignment, PopupWindow};
pub use scroll::{ScrollBar, Slider};
pub use scroll_container::{ScrollBarVisibility, ScrollContainer, ScrollContainerRoot, ScrollMode};
pub use select::{SelectDropdown, SelectList};
pub use select_item::{
    SelectItemSource, SelectItemWidget, Sort, SourceChanges, SourceRef, SourceUpdate, StringItem, StringItemSource,
    WeakSourceRef, create_default_item_widget,
};
pub use tab_container::TabContainer;
pub use text::{TextAlign, TextField};
pub use toggle::{Section, SectionHeader, Toggle, ToggleContainer};
pub use window::{ResizeFit, Window, WindowSettings};

use crate::reader::Factory;
use crate::ui::{Ctx, Ui};
use crate::value::ValueKind;
use crate::widget::{Widget, Z};

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
    register_both(ui, "TBWindow", "Window", Factory::of::<Window>("Window"));
    register_both(ui, "TBButton", "Button", Factory::of::<Button>("Button").inflate_z(Z::Bottom));
    register_both(
        ui,
        "TBClickLabel",
        "ClickLabel",
        Factory::of::<ClickLabel>("ClickLabel").sync(ValueKind::Str).inflate_z(Z::Bottom),
    );
    register_both(ui, "TBTextField", "TextField", Factory::of::<TextField>("TextField").sync(ValueKind::Str));
    register_both(ui, "TBSkinImage", "SkinImage", Factory::of::<SkinImage>("SkinImage"));
    register_both(ui, "TBSeparator", "Separator", Factory::of::<Separator>("Separator"));
    register_both(
        ui,
        "TBProgressSpinner",
        "ProgressSpinner",
        Factory::of::<ProgressSpinner>("ProgressSpinner").sync(ValueKind::Int),
    );
    register_both(ui, "TBCheckBox", "CheckBox", Factory::of::<CheckBox>("CheckBox").sync(ValueKind::Int));
    register_both(ui, "TBRadioButton", "RadioButton", Factory::of::<RadioButton>("RadioButton").sync(ValueKind::Int));
    register_both(ui, "TBScrollBar", "ScrollBar", Factory::of::<ScrollBar>("ScrollBar").sync(ValueKind::Float));
    register_both(ui, "TBSlider", "Slider", Factory::of::<Slider>("Slider").sync(ValueKind::Float));
    register_both(ui, "TBMover", "Mover", Factory::of::<Mover>("Mover"));
    register_both(ui, "TBResizer", "Resizer", Factory::of::<Resizer>("Resizer"));
    register_both(ui, "TBDimmer", "Dimmer", Factory::of::<Dimmer>("Dimmer"));
    register_both(ui, "TBScrollContainer", "ScrollContainer", Factory::of::<ScrollContainer>("ScrollContainer"));
    register_both(ui, "TBTabContainer", "TabContainer", Factory::of::<TabContainer>("TabContainer"));
    register_both(
        ui,
        "TBToggleContainer",
        "ToggleContainer",
        Factory::of::<ToggleContainer>("ToggleContainer").sync(ValueKind::Int),
    );
    register_both(
        ui,
        "TBSectionHeader",
        "SectionHeader",
        Factory::of::<SectionHeader>("SectionHeader").sync(ValueKind::Int),
    );
    register_both(ui, "TBSection", "Section", Factory::of::<Section>("Section").sync(ValueKind::Int));
    register_both(ui, "TBEditField", "EditField", Factory::of::<EditField>("EditField").sync(ValueKind::Str));
    register_both(ui, "TBSelectList", "SelectList", Factory::of::<SelectList>("SelectList").sync(ValueKind::Int));
    register_both(
        ui,
        "TBSelectDropdown",
        "SelectDropdown",
        Factory::of::<SelectDropdown>("SelectDropdown").sync(ValueKind::Int),
    );
    register_both(
        ui,
        "TBInlineSelect",
        "InlineSelect",
        Factory::of::<InlineSelect>("InlineSelect").sync(ValueKind::Int),
    );
}
