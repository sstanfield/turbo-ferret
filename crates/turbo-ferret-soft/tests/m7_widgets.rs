//! M7: golden image of edit fields with the default skin and font: text with a
//! selection and the caret, a placeholder, the search type, a password, and styled
//! multi line text. Bless with `BLESS=1 cargo test -p turbo-ferret-soft`.

mod common;

use common::assert_golden;
use turbo_ferret::geom::{Color, Point, Rect, Size};
use turbo_ferret::widgets::*;
use turbo_ferret::*;
use turbo_ferret_soft::SoftRenderer;

const BG: Color = Color::rgb(40, 44, 52);

fn ui(size: Size) -> Ui {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_language("language/lng_en.tb.txt").unwrap();
    ui.load_skin("default_skin/skin.tb.txt", Some("demo01/skin/skin.tb.txt")).unwrap();
    ui.add_font("DejaVuShadow", "default_font/dejavu_white_with_shadow.tb.txt");
    assert!(ui.set_default_font(FontDesc::new(tbid!("DejaVuShadow"), 14)));
    ui.set_viewport(size);
    ui
}

const FIELDS: &str = r#"
TBEditField: id: plain, text: "Hello world"
TBEditField: id: search, type: search, placeholder: "@search"
TBEditField: id: password, type: password, text: "secret"
TBEditField: id: readonly, readonly: 1, text: "Read only"
TBEditField: id: styled, multiline: 1, styling: 1
	text: "<color #0794f8>Styled</color> text with <u>underline</u>,\n" \
		"a line that is long enough to wrap around at the edge of the field, and\n" \
		" • a widget: <widget TBButton: text: 'Hi'>\n" \
		"line 5\nline 6\nline 7\nline 8"
"#;

#[test]
fn edit_fields_golden() {
    let size = Size::new(420, 190);
    let mut ui = ui(size);
    let root = ui.root();
    let ids = ui.inflate(root, FIELDS).unwrap();
    for (i, &id) in ids[..4].iter().enumerate() {
        ui.set_rect(id, Rect::new(10, 10 + i as i32 * 36, 150, 28));
    }
    ui.set_rect(ids[4], Rect::new(175, 10, 235, 170));
    ui.set_time(0);
    ui.update();

    // Select "lo wo" by dragging; the field has the focus and shows its caret.
    let plain = ids[0];
    let face = ui.default_font();
    let x = |chars: &str| ui.text_width(face, chars);
    let origin = ui.to_root(plain, ui.padding_rect(plain).origin());
    let (from, to) = (origin + Point::new(x("Hel") + 1, 8), origin + Point::new(x("Hello wo") + 1, 8));
    ui.pointer_down(from, 1, Modifiers::NONE);
    ui.pointer_move(to, Modifiers::NONE);
    ui.pointer_up(to, Modifiers::NONE);
    let w = ui.get::<EditField>(plain).unwrap();
    assert_eq!(w.style_edit().selected_text(), "lo wo");
    assert!(w.style_edit().caret.on);
    assert_eq!(ui.focused(), Some(plain));

    // The search field got its own skin (room for the icon) from its type.
    assert!(ui.padding(ids[1]).left > ui.padding(plain).left + 10);
    // The styled field wraps, scrolls, and holds a real button.
    let styled = ui.get::<EditField>(ids[4]).unwrap();
    assert!(styled.overflow_y() > 0);
    assert!(styled.style_edit().blocks()[1].height > styled.style_edit().blocks()[0].height, "line 2 wraps");
    let hi = ui.find_as::<Button>(ids[4], TbId::NONE).expect("the embedded button");
    assert_eq!(ui.text(hi), Some("Hi"));
    ui.scroll_to(ids[4], Point::new(0, 12));
    ui.update();

    let mut r = SoftRenderer::new(size);
    r.clear(BG);
    ui.paint(&mut r);
    assert_golden("edit_fields", &r);

    // Half a second later the caret is off: the image differs only there.
    ui.set_time(500);
    ui.update();
    assert!(!ui.get::<EditField>(plain).unwrap().style_edit().caret.on);
    let before = r.pixels().to_vec();
    r.clear(BG);
    ui.paint(&mut r);
    let changed: Vec<usize> = before
        .chunks_exact(4)
        .zip(r.pixels().chunks_exact(4))
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(!changed.is_empty() && changed.len() < 120, "{} pixels changed", changed.len());
    let (w, plain_rect) = (size.w as usize, ui.rect(plain));
    assert!(changed.iter().all(|&i| plain_rect.contains(Point::new((i % w) as i32, (i / w) as i32))));
}
