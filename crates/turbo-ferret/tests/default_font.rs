//! The built-in font (feature `default-font`): a new `Ui` shows readable text
//! without loading anything.
#![cfg(feature = "default-font")]

use turbo_ferret::font::{BOX_FONT, DEFAULT_FONT, DEFAULT_FONT_NAME, DEFAULT_FONT_SIZES};
use turbo_ferret::widgets::TextField;
use turbo_ferret::*;

fn ui_at(dpi: i32) -> Ui {
    let mut ui = Ui::new(Config::for_dpi(dpi), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(400, 300));
    ui
}

#[test]
fn a_new_ui_has_a_font() {
    let ui = ui_at(96);
    let fd = ui.default_font();
    assert_eq!(fd, FontDesc::new(DEFAULT_FONT, 14));
    assert_eq!(DEFAULT_FONT, TbId::new(DEFAULT_FONT_NAME));
    let face = ui.fonts().face(fd);
    assert!(!face.is_box_font());
    assert_eq!(face.desc(), fd);
    // DejaVu Sans at 14 pixels.
    assert_eq!((face.ascent(), face.descent(), face.height()), (13, 3, 16));
    assert_eq!(ui.text_width(fd, "Button"), 47);
    assert_eq!(ui.text_width(fd, "A B"), ui.text_width(fd, "A") + ui.text_width(fd, " ") + ui.text_width(fd, "B"));
    assert!(ui.text_width(fd, "Ünïcödé") > ui.text_width(fd, "Un"), "Latin-1 is there");
}

#[test]
fn the_size_follows_the_dpi() {
    for (dpi, size, strip) in [(96, 14u16, 16), (144, 21, 24), (192, 28, 33), (120, 18, 24), (288, 42, 33)] {
        let ui = ui_at(dpi);
        let fd = ui.default_font();
        assert_eq!(fd, FontDesc::new(DEFAULT_FONT, size), "at {dpi} DPI");
        // The nearest of the three bitmap sizes is used (its line height tells).
        assert_eq!(ui.fonts().face(fd).height(), strip, "at {dpi} DPI");
    }
    assert_eq!(DEFAULT_FONT_SIZES, [14, 21, 28]);
}

#[test]
fn widgets_measure_with_it() {
    let mut ui = ui_at(96);
    let root = ui.root();
    let t = ui.add(root, TextField::new("Hello, world"));
    let ps = ui.preferred_size_free(t);
    assert_eq!(ps.pref.h, 16);
    assert_eq!(ps.pref.w, ui.text_width(ui.default_font(), "Hello, world"));
    assert!(ps.pref.w > 60 && ps.pref.w < 100, "{}", ps.pref.w);
}

#[test]
fn an_application_can_replace_it_or_go_without() {
    let mut ui = ui_at(96);
    // The box font, as without the feature.
    assert!(ui.set_default_font(BOX_FONT));
    assert!(ui.fonts().face(ui.default_font()).is_box_font());
    // The built-in font again, at any size.
    assert!(ui.set_default_font(FontDesc::new(DEFAULT_FONT, 30)));
    assert_eq!(ui.fonts().face(ui.default_font()).height(), 33);
    // A font registered under its name takes its place: new faces come from there
    // (and here that file does not exist).
    ui.add_font(DEFAULT_FONT_NAME, "fonts/missing.tb.txt");
    assert!(!ui.create_font_face(FontDesc::new(DEFAULT_FONT, 50)));
}
