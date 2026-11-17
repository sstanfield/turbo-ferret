//! M4: golden images of the basic widgets with the default skin and font, in their
//! main states. Bless with `BLESS=1 cargo test -p turbo-ferret-soft`.

mod common;

use common::assert_golden;
use turbo_ferret::geom::{Color, Rect, Size};
use turbo_ferret::widgets::*;
use turbo_ferret::*;
use turbo_ferret_soft::SoftRenderer;

const BG: Color = Color::rgb(40, 44, 52);

fn ui(size: Size) -> Ui {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    ui.add_font("Segoe", "default_font/segoe_white_with_shadow.tb.txt");
    assert!(ui.set_default_font(FontDesc::new(tbid!("Segoe"), 14)));
    ui.set_viewport(size);
    ui
}

fn render(ui: &mut Ui, size: Size) -> SoftRenderer {
    let mut r = SoftRenderer::new(size);
    r.clear(BG);
    ui.paint(&mut r);
    r
}

const WIDGETS: &str = r#"
TBLayout
	axis y
	position left top
	distribution-position left top
	TBLayout
		TBButton: text: "Button"
		TBButton: text: "Pressed", id: pressed
		TBButton: text: "Disabled", state: disabled
		TBButton: text: "Toggle", toggle-mode: 1, value: 1
	TBLayout
		TBCheckBox: value: 1
		TBCheckBox
		TBRadioButton: value: 1, group-id: r
		TBRadioButton: group-id: r
		TBClickLabel: text: "Click label"
			TBCheckBox
	TBLayout
		TBTextField: text: "A text field"
		TBSeparator
		TBSkinImage: skin: Icon16
		TBProgressSpinner: value: 1
	TBSlider
		lp: width: 200
		value 0.3
	TBScrollBar
		lp: width: 200
		axis x
	TBButton: text: "A very long button text that gets squeezed", squeezable: 1, lp: max-width: 160
"#;

#[test]
fn widgets_golden() {
    let size = Size::new(420, 260);
    let mut ui = ui(size);
    let root = ui.root();
    ui.inflate(root, WIDGETS).unwrap();
    let pressed = ui.find(root, tbid!("pressed")).unwrap();
    ui.set_state(pressed, WidgetState::PRESSED, true);
    let sb = ui.find_as::<ScrollBar>(root, TbId::NONE).unwrap();
    ui.with_widget::<ScrollBar, _>(sb, |w, cx| w.set_limits(cx, 0.0, 100.0, 50.0));
    ui.set_value(sb, 25.0f32);
    ui.set_time(0);
    ui.update();
    let r = render(&mut ui, size);
    assert_golden("widgets", &r);
}

#[test]
fn window_golden() {
    let size = Size::new(320, 200);
    let mut ui = ui(size);
    let root = ui.root();
    let w = ui.block_animations(|ui| ui.add(root, Window::new("Turbo Ferret")));
    ui.inflate(
        w,
        "TBLayout: axis: y\n\tTBTextField: text: \"Hello from a window\"\n\tTBLayout\n\t\tTBButton: text: OK, id: ok\n\t\tTBButton: text: Cancel\n",
    )
    .unwrap();
    ui.resize_to_fit_content(w, ResizeFit::Preferred);
    ui.center_in_parent(w);
    ui.update();
    let ok = ui.find(w, tbid!("ok")).unwrap();
    ui.set_focus(ok);
    ui.set_auto_focus_state(true);
    ui.update();
    let r = render(&mut ui, size);
    assert_golden("window", &r);
    // The window measured its content: title plus layout plus skin padding.
    let wr = ui.rect(w);
    assert!(wr.w > 100 && wr.h > 60, "{wr:?}");
}

#[test]
fn button_hover_and_press_states() {
    let size = Size::new(120, 40);
    let mut ui = ui(size);
    let root = ui.root();
    let b = ui.add(root, Button::new("Hover"));
    ui.set_rect(b, Rect::new(10, 5, 100, 30));
    ui.update();
    let normal = render(&mut ui, size);
    ui.pointer_move(Point::new(50, 20), Modifiers::NONE);
    ui.update();
    let hovered = render(&mut ui, size);
    ui.pointer_down(Point::new(50, 20), 1, Modifiers::NONE);
    ui.update();
    let pressed = render(&mut ui, size);
    assert_ne!(normal.pixels(), pressed.pixels(), "pressed state looks different");
    assert_golden("button_pressed", &pressed);
    let _ = hovered;
}
