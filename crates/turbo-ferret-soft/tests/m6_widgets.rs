//! M6: golden images of the select list, dropdown with its menu, message window and
//! inline select with the default skin and font. Bless with
//! `BLESS=1 cargo test -p turbo-ferret-soft`.

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
    ui.load_language("language/lng_en.tb.txt").unwrap();
    // With the demo skin on top, for its icons ("Icon16", "Icon48").
    ui.load_skin("default_skin/skin.tb.txt", Some("demo01/skin/skin.tb.txt")).unwrap();
    ui.add_font("DejaVuShadow", "default_font/dejavu_white_with_shadow.tb.txt");
    assert!(ui.set_default_font(FontDesc::new(tbid!("DejaVuShadow"), 14)));
    ui.set_viewport(size);
    ui
}

fn render(ui: &mut Ui, size: Size) -> SoftRenderer {
    let mut r = SoftRenderer::new(size);
    r.clear(BG);
    ui.paint(&mut r);
    r
}

const LIST: &str = r#"
TBSelectList: id: list, value: 2
	items
		item: text: "Apple"
		item: text: "Banana"
		item: text: "Cherry"
		item: text: "Date"
		item: text: "Elderberry"
		item: text: "Fig"
		item: text: "Grape"
"#;

#[test]
fn select_list_golden() {
    let size = Size::new(360, 150);
    let mut ui = ui(size);
    let root = ui.root();
    let a = ui.inflate(root, LIST).unwrap()[0];
    ui.set_rect(a, Rect::new(10, 10, 160, 130));
    let b = ui.inflate(root, LIST).unwrap()[0];
    ui.set_rect(b, Rect::new(190, 10, 160, 130));
    ui.with_widget::<SelectList, _>(b, |w, cx| w.set_filter(cx, "e"));
    ui.update();
    // The selected item is highlighted; the filtered list has a header.
    let w = ui.get::<SelectList>(a).unwrap();
    let cherry = w.item_widget(&ui, 2).unwrap();
    assert!(ui.has_state(cherry, WidgetState::SELECTED));
    let sc = w.scroll_container().unwrap();
    assert!(ui.scroll_info(sc).unwrap().max.y > 0, "seven items do not fit");
    let layout = ui.get::<SelectList>(b).unwrap().item_layout().unwrap();
    assert_eq!(ui.text(ui.children(layout)[0]), Some("Showing 5 of 7"));
    let r = render(&mut ui, size);
    assert_golden("select_list", &r);
}

#[test]
fn dropdown_menu_golden() {
    let size = Size::new(300, 320);
    let mut ui = ui(size);
    let root = ui.root();
    let sub: SourceRef = StringItemSource::shared();
    let src = StringItemSource::shared();
    {
        let mut s = src.borrow_mut();
        s.add(StringItem::new("Plain item"));
        s.add(StringItem::new("With an image").image(tbid!("Icon16")));
        s.add(StringItem::new("-"));
        s.add(StringItem::with_sub_source("With a sub menu", &sub));
    }
    let d = ui.add(root, SelectDropdown::default());
    ui.set_rect(d, Rect::new(10, 10, 150, 30));
    let source: SourceRef = src.clone();
    ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.set_source(cx, Some(source)));
    ui.set_value(d, 1);
    let i = ui.inflate(root, "TBInlineSelect: min: 0, max: 10, value: 7\n").unwrap()[0];
    ui.set_rect(i, Rect::new(180, 10, 110, 30));
    ui.set_focus(d);
    ui.update();
    assert_eq!(ui.text(d), Some("With an image"));
    // Open the menu (without the fade, so the image is stable).
    ui.block_animations(|ui| ui.with_widget::<SelectDropdown, _>(d, |w, cx| w.open_window(cx)));
    ui.update();
    let menu = *ui.children(root).last().unwrap();
    let list = ui.get::<MenuWindow>(menu).expect("the menu").list().unwrap();
    // Below the dropdown, at least as wide as it, and as high as its four items.
    let mr = ui.rect(menu);
    assert_eq!((mr.x, mr.y), (10, 40));
    assert!(mr.w >= 150);
    let layout = ui.get::<SelectList>(list).unwrap().item_layout().unwrap();
    let last = *ui.children(layout).last().unwrap();
    let bottom = ui.to_root(last, turbo_ferret::geom::Point::new(0, ui.rect(last).h)).y;
    assert!(bottom <= mr.bottom() && bottom > mr.bottom() - 8, "items end at {bottom}, menu at {}", mr.bottom());
    // The current item is selected in the menu.
    let current = ui.get::<SelectList>(list).unwrap().item_widget(&ui, 1).unwrap();
    assert!(ui.has_state(current, WidgetState::SELECTED));
    let r = render(&mut ui, size);
    assert_golden("dropdown_menu", &r);
}

#[test]
fn message_window_golden() {
    let size = Size::new(360, 240);
    let mut ui = ui(size);
    let root = ui.root();
    let b = ui.add(root, Button::new("Behind the dimmer"));
    ui.set_rect(b, Rect::new(10, 10, 160, 30));
    let mut settings = MessageSettings::new(MessageButtons::YesNo, tbid!("Icon48"));
    settings.dimmer = true;
    settings.styling = true;
    let text = "Really <color #0794f8>close</color> the window?\n\nThis text is long enough to wrap to more lines.";
    let win = ui.block_animations(|ui| ui.show_message(b, tbid!("confirm"), "Are you sure?", text, &settings)).unwrap();
    ui.update();
    // The content is inside the window's padding rect, below the title.
    let content = ui.children(win).iter().copied().find(|&c| ui.get::<Layout>(c).is_some()).unwrap();
    assert_eq!(ui.rect(content), ui.padding_rect(win));
    assert!(ui.padding_rect(win).y > 20, "there is a title bar");
    let yes = ui.find(win, tbid!("TBMessageWindow.yes")).unwrap();
    assert_eq!(ui.focused(), Some(yes));
    let wr = ui.rect(win);
    assert!((wr.x + wr.w / 2 - 180).abs() <= 1 && (wr.y + wr.h / 2 - 120).abs() <= 1, "centered: {wr:?}");
    let r = render(&mut ui, size);
    assert_golden("message_window", &r);
}
