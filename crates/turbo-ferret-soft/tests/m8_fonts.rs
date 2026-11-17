//! M8: golden image of TrueType text: widgets of the default skin in a TrueType
//! font, and a heading with a blurred copy of itself behind it as a glow.
//! Bless with `BLESS=1 cargo test -p turbo-ferret-soft`.

mod common;

use common::assert_golden;
use turbo_ferret::font::BackgroundFont;
use turbo_ferret::geom::{Color, Point, Rect, Size};
use turbo_ferret::*;
use turbo_ferret_soft::SoftRenderer;
use turbo_ferret_ttf::TtfBackend;

const BG: Color = Color::rgb(40, 44, 52);

const SRC: &str = r#"
TBLayout: axis: y, distribution: gravity, position: left
	TBTextField: id: heading, text: "TrueType: Ágætis byrjun"
		font: size: 28
	TBTextField: text: "The quick brown fox jumps over the lazy dog. 0123456789"
	TBLayout
		TBButton: text: "Button"
		TBCheckBox: value: 1
		TBTextField: text: "Résumé (100%) — “quotes” & more"
	TBEditField: text: "An edit field", gravity: left right
	TBTextField: text: "Small print at nine pixels: illegible? Not quite."
		font: size: 9
"#;

#[test]
fn truetype_text_golden() {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let size = Size::new(440, 190);
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    ui.fonts_mut().add_backend(Box::new(TtfBackend::new()));
    ui.add_font("Vera", "vera.ttf");
    // The same file under a second name, for the blurred copy.
    ui.add_font("VeraGlow", "vera.ttf");
    let (vera, glow) = (tbid!("Vera"), tbid!("VeraGlow"));
    for s in [9, 28] {
        assert!(ui.create_font_face(FontDesc::new(vera, s)));
    }
    assert!(ui.create_font_face(FontDesc::new(glow, 28)));
    assert!(ui.set_default_font(FontDesc::new(vera, 14)));
    assert!(ui.fonts_mut().set_blur_radius(FontDesc::new(glow, 28), 3));
    let background =
        BackgroundFont { desc: FontDesc::new(glow, 28), color: Color::rgb(0, 150, 255), offset: Point::new(0, 0) };
    assert!(ui.fonts_mut().set_background_font(FontDesc::new(vera, 28), Some(background)));

    ui.set_viewport(size);
    let root = ui.root();
    let ids = ui.inflate(root, SRC).unwrap();
    ui.set_rect(ids[0], Rect::new(10, 8, 420, 174));
    ui.update();

    // Measuring uses the TrueType advances.
    let face = ui.fonts().face(FontDesc::new(vera, 14));
    assert!(!face.is_box_font());
    assert_eq!((face.ascent(), face.descent(), face.height()), (13, 3, 16));
    assert_eq!(ui.text_width(FontDesc::new(vera, 14), "Button"), 47);

    let mut r = SoftRenderer::new(size);
    r.clear(BG);
    ui.paint(&mut r);
    assert_golden("truetype_text", &r);
}
