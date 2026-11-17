//! M5: golden images of the scroll container, tab container, sections and the layout
//! overflow fade-out with the default skin and font. Bless with
//! `BLESS=1 cargo test -p turbo-ferret-soft`.

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

#[test]
fn scroll_container_golden() {
    let size = Size::new(240, 180);
    let mut ui = ui(size);
    let root = ui.root();
    let mut src = String::from("TBScrollContainer: id: sc, scroll-mode: auto\n\tTBLayout: axis: y\n");
    for i in 1..=9 {
        src += &format!("\t\tTBButton: text: \"Button number {i}\"\n");
    }
    let sc = ui.inflate(root, &src).unwrap()[0];
    ui.set_rect(sc, Rect::new(10, 10, 220, 160));
    ui.update();
    let w = ui.get::<ScrollContainer>(sc).unwrap();
    let (sbx, sby) = (w.scrollbar_x().unwrap(), w.scrollbar_y().unwrap());
    // Too high but not too wide: only the Y bar shows, and it takes real width.
    assert_eq!((ui.opacity(sbx), ui.opacity(sby)), (0.0, 1.0));
    assert!(ui.rect(sby).w > 4);
    assert_eq!(ui.padding_rect(sc).w, 220 - ui.rect(sby).w);
    let max = ui.scroll_info(sc).unwrap().max;
    assert!(max.y > 50 && max.x == 0, "{max:?}");
    ui.scroll_to(sc, Point::new(0, 45));
    ui.update();
    let r = render(&mut ui, size);
    assert_golden("scroll_container", &r);

    // What is scrolled out is clipped: the area above the container stays clear.
    let px = |r: &SoftRenderer, x: i32, y: i32| {
        let i = 4 * (y * size.w + x) as usize;
        [r.pixels()[i], r.pixels()[i + 1], r.pixels()[i + 2]]
    };
    for x in (10..230).step_by(7) {
        assert_eq!(px(&r, x, 4), [40, 44, 52], "at x {x}");
        assert_eq!(px(&r, x, 175), [40, 44, 52], "at x {x}");
    }
}

const TABS: &str = r#"
TBTabContainer: id: tc
	tabs
		TBButton: text: "First"
		TBButton: text: "Second"
		TBButton: text: "Third"
	TBLayout: axis: y
		TBTextField: text: "Page one"
		TBButton: text: "A button"
	TBTextField: text: "Page two"
	TBTextField: text: "Page three"
"#;

#[test]
fn tab_container_golden() {
    let size = Size::new(460, 320);
    let mut ui = ui(size);
    let root = ui.root();
    let mut tcs = Vec::new();
    for (align, rect) in [
        ("top", Rect::new(10, 10, 210, 140)),
        ("bottom", Rect::new(240, 10, 210, 140)),
        ("left", Rect::new(10, 170, 210, 140)),
        ("right", Rect::new(240, 170, 210, 140)),
    ] {
        let tc = ui.inflate(root, &TABS.replace("id: tc", &format!("id: tc, align: {align}"))).unwrap()[0];
        // Measured during inflation (while the container's hook runs) and after the
        // first update, the tabs get the same size: their skin depends on the
        // container's alignment.
        let before = ui.preferred_size_free(tc);
        ui.set_rect(tc, rect);
        ui.update();
        assert_eq!(ui.preferred_size_free(tc), before, "align {align}");
        tcs.push(tc);
    }
    // Show another page in two of them.
    ui.set_value(tcs[1], 1);
    ui.set_value(tcs[3], 2);
    ui.update();
    let r = render(&mut ui, size);
    assert_golden("tab_container", &r);
}

#[test]
fn sections_golden() {
    let size = Size::new(240, 270);
    let mut ui = ui(size);
    let root = ui.root();
    let src = r#"
TBLayout: axis: y, distribution-position: "left top", distribution: "available"
	TBSection: value: 1, text: "Expanded"
		TBLayout: axis: y
			TBButton: text: "Inside"
			TBCheckBox: value: 1
	TBSection: value: 0, text: "Collapsed"
		TBButton: text: "Hidden"
	TBSection: value: 1, text: "Also expanded"
		TBTextField: text: "Some text"
"#;
    let col = ui.inflate(root, src).unwrap()[0];
    ui.set_rect(col, Rect::new(10, 10, 220, 250));
    ui.update();
    assert_eq!(ui.get::<Layout>(col).unwrap().overflow(), 0, "everything fits");
    let r = render(&mut ui, size);
    assert_golden("sections", &r);
}

#[test]
fn layout_fadeout_golden() {
    let size = Size::new(240, 60);
    let scene = |fade: bool, scroll: i32| {
        let mut ui = ui(size);
        let root = ui.root();
        let mut src = String::from("TBLayout: id: l\n");
        for i in 1..=8 {
            src += &format!("\tTBButton: text: \"Item {i}\"\n");
        }
        let l = ui.inflate(root, &src).unwrap()[0];
        ui.with_widget::<Layout, _>(l, |w, _| w.paint_overflow_fadeout = fade);
        ui.set_rect(l, Rect::new(20, 10, 200, 40));
        ui.update();
        assert!(ui.get::<Layout>(l).unwrap().overflow() > 100);
        ui.scroll_to(l, Point::new(scroll, 0));
        ui.update();
        render(&mut ui, size)
    };
    let px = |r: &SoftRenderer, x: i32, y: i32| {
        let i = 4 * (y * size.w + x) as usize;
        [r.pixels()[i], r.pixels()[i + 1], r.pixels()[i + 2]]
    };
    // Scrolled to the middle: both edges fade.
    let faded = scene(true, 80);
    let plain = scene(false, 80);
    assert_golden("layout_fadeout", &faded);
    let differs = |a: &SoftRenderer, b: &SoftRenderer, xs: core::ops::Range<i32>| {
        xs.clone().any(|x| (12..48).any(|y| px(a, x, y) != px(b, x, y)))
    };
    assert!(differs(&faded, &plain, 20..36), "left edge fades");
    assert!(differs(&faded, &plain, 204..220), "right edge fades");
    assert!(!differs(&faded, &plain, 90..150), "the middle is untouched");
    // At the start only the far edge fades.
    let faded = scene(true, 0);
    let plain = scene(false, 0);
    assert!(!differs(&faded, &plain, 20..36), "nothing is hidden on the left");
    assert!(differs(&faded, &plain, 204..220));
}
