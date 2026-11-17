//! Render the demo headless with the software renderer and write a screenshot to
//! `target/demo_screenshot.png` (for looking at; not a golden test).

use turbo_ferret::geom::{Color, Point, Size};
use turbo_ferret::*;
use turbo_ferret_demo::{Demo, default_resource_root, open_resource_window};
use turbo_ferret_soft::SoftRenderer;

fn save(r: &SoftRenderer, name: &str) {
    let dir = format!("{}/../target", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = format!("{dir}/{name}");
    let f = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), r.size().w as u32, r.size().h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(r.pixels()).unwrap();
}

#[test]
fn demo_renders_headless() {
    let size = Size::new(1024, 700);
    let mut demo = Demo::new(&default_resource_root(), size, 96);
    let main = demo.main.expect("main window opens");
    for t in [0u64, 100, 1000] {
        demo.frame(t);
    }
    // Open a few of TB's demo layouts through the main window's buttons' handler.
    open_resource_window(&mut demo.ui, "demo01/ui_resources/test_radio_checkbox.tb.txt");
    open_resource_window(&mut demo.ui, "demo01/ui_resources/test_layout02.tb.txt");
    demo.frame(2000);
    demo.frame(3000);
    let mut r = SoftRenderer::new(size);
    r.clear(Color::BLACK);
    demo.ui.paint(&mut r);
    save(&r, "demo_screenshot.png");
    assert!(demo.ui.exists(main));
    // Clicking the "Animations" button opens a window.
    let before = demo.ui.children(demo.ui.root()).len();
    let root = demo.ui.root();
    let btn = demo.ui.find(main, tbid!("test-animations")).expect("animations button");
    let br = demo.ui.rect(btn);
    let p = demo.ui.to_root(btn, Point::new(br.w / 2, br.h / 2));
    demo.ui.activate_window(main);
    demo.frame(3100);
    demo.ui.pointer_down(p, 1, Modifiers::NONE);
    demo.ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(demo.ui.children(root).len(), before + 1, "a window opened");
}

#[test]
fn demo_test_windows_headless() {
    let size = Size::new(1024, 700);
    let mut demo = Demo::new(&default_resource_root(), size, 96);
    for f in [
        "test_skin_conditions01.tb.txt",
        "test_skin_conditions02.tb.txt",
        "test_layout01.tb.txt",
        "test_connections.tb.txt",
    ] {
        assert!(open_resource_window(&mut demo.ui, &format!("demo01/ui_resources/{f}")).is_some(), "{f}");
    }
    demo.frame(0);
    demo.frame(1000);
    let mut r = SoftRenderer::new(size);
    r.clear(Color::BLACK);
    demo.ui.paint(&mut r);
    save(&r, "demo_screenshot2.png");
}

/// Paint into `r` and save it. One renderer per `Ui`: it holds the uploaded bitmaps.
fn shot(demo: &mut Demo, r: &mut SoftRenderer, name: &str) {
    r.clear(Color::BLACK);
    demo.ui.paint(r);
    save(r, name);
}

fn click_id(demo: &mut Demo, within: WidgetId, id: TbId) {
    let w = demo.ui.find(within, id).unwrap_or_else(|| panic!("no widget {id:?}"));
    let r = demo.ui.rect(w);
    let p = demo.ui.to_root(w, Point::new(r.w / 2, r.h / 2));
    demo.ui.pointer_down(p, 1, Modifiers::NONE);
    demo.ui.pointer_up(p, Modifiers::NONE);
}

/// M5: the scroll container, page (snap), toggle and tab container windows.
#[test]
fn demo_m5_windows_headless() {
    use turbo_ferret::widgets::{ScrollContainer, TabContainer};
    let size = Size::new(1280, 800);
    let mut demo = Demo::new(&default_resource_root(), size, 96);
    let main = demo.main.unwrap();
    let root = demo.ui.root();
    let mut soft = SoftRenderer::new(size);
    // Advance the demo's clock in 16 ms frames.
    let run = |demo: &mut Demo, ms: u64| {
        for _ in 0..ms / 16 {
            let t = demo.ui.now() + 16;
            demo.frame(t);
        }
    };
    run(&mut demo, 500);
    shot(&mut demo, &mut soft, "demo_m5_start.png");

    // The main window's sections: expand "Layout tests".
    let headers: Vec<WidgetId> = {
        let mut v = Vec::new();
        let mut stack = vec![main];
        while let Some(w) = stack.pop() {
            if demo.ui.get::<turbo_ferret::widgets::SectionHeader>(w).is_some() {
                v.push(w);
            }
            stack.extend(demo.ui.children(w).iter().rev());
        }
        v
    };
    assert_eq!(headers.len(), 4);
    let r = demo.ui.rect(headers[1]);
    let p = demo.ui.to_root(headers[1], Point::new(r.w / 2, r.h / 2));
    demo.ui.pointer_down(p, 1, Modifiers::NONE);
    demo.ui.pointer_up(p, Modifiers::NONE);
    run(&mut demo, 100);

    // Scroll container window, scrolled a little by wheel.
    click_id(&mut demo, main, tbid!("test-scroll-container"));
    run(&mut demo, 400);
    let scw = *demo.ui.children(root).last().unwrap();
    let sc = demo.ui.find_as::<ScrollContainer>(scw, TbId::NONE).expect("scroll container");
    let c = demo.ui.to_root(sc, Point::new(100, 100));
    demo.ui.wheel(c, Point::new(0, 3), Modifiers::NONE);
    assert_eq!(demo.ui.scroll_info(sc).unwrap().pos.y, 120);
    run(&mut demo, 50);
    shot(&mut demo, &mut soft, "demo_m5_scroll.png");

    // The page window: a flick snaps to the next page.
    demo.ui.activate_window(main);
    run(&mut demo, 50);
    click_id(&mut demo, main, tbid!("test-page"));
    run(&mut demo, 400);
    let pw = *demo.ui.children(root).last().unwrap();
    let pager = demo.ui.find(pw, tbid!("page-scroller")).unwrap();
    let from = demo.ui.to_root(pager, Point::new(150, 150));
    demo.ui.pointer_down(from, 1, Modifiers::NONE);
    for i in 1..=6 {
        let t = demo.ui.now() + 10;
        demo.ui.set_time(t);
        demo.ui.pointer_move(from + Point::new(-12 * i, 0), Modifiers::NONE);
    }
    demo.ui.pointer_up(from + Point::new(-72, 0), Modifiers::NONE);
    run(&mut demo, 100);
    shot(&mut demo, &mut soft, "demo_m5_page_moving.png");
    run(&mut demo, 3000);
    let page_w = demo.ui.padding_rect(pager).w;
    let x = demo.ui.scroll_info(pager).unwrap().pos.x;
    assert!(x > 0 && x % page_w == 0, "snapped to a page: {x} (page {page_w})");

    // Toggle containers.
    demo.ui.activate_window(main);
    run(&mut demo, 50);
    open_resource_window(&mut demo.ui, "demo01/ui_resources/test_toggle_containers.tb.txt");
    run(&mut demo, 400);
    shot(&mut demo, &mut soft, "demo_m5_all.png");

    // Tab container: switch to the right side.
    let tc = demo.ui.find_as::<TabContainer>(root, tbid!("tabcontainer")).unwrap();
    demo.ui.with_widget::<TabContainer, _>(tc, |w, cx| w.set_alignment(cx, Align::Right));
    let tw = demo.ui.parent_window(tc).unwrap();
    demo.ui.activate_window(tw);
    demo.ui.resize_to_fit_content(tw, turbo_ferret::widgets::ResizeFit::CurrentOrNeeded);
    run(&mut demo, 400);
    shot(&mut demo, &mut soft, "demo_m5_tabs_right.png");
}

/// M6: lists, dropdown and popup menus, message windows.
#[test]
fn demo_m6_windows_headless() {
    use turbo_ferret::widgets::{MenuWindow, MessageWindow, SelectDropdown, SelectItemSource, SelectList};
    let size = Size::new(1280, 800);
    let mut demo = Demo::new(&default_resource_root(), size, 96);
    let main = demo.main.unwrap();
    let root = demo.ui.root();
    let mut soft = SoftRenderer::new(size);
    let run = |demo: &mut Demo, ms: u64| {
        for _ in 0..ms / 16 {
            let t = demo.ui.now() + 16;
            demo.frame(t);
        }
    };
    let click = |demo: &mut Demo, w: WidgetId| {
        let r = demo.ui.rect(w);
        let p = demo.ui.to_root(w, Point::new(r.w / 2, r.h / 2));
        demo.ui.pointer_down(p, 1, Modifiers::NONE);
        demo.ui.pointer_up(p, Modifiers::NONE);
    };
    let top = |demo: &Demo| *demo.ui.children(root).last().unwrap();
    let item = |demo: &Demo, list: WidgetId, index: i32| {
        demo.ui.get::<SelectList>(list).unwrap().item_widget(&demo.ui, index).expect("item")
    };
    run(&mut demo, 500);
    shot(&mut demo, &mut soft, "demo_m6_start.png");

    // The two list windows are open and filled from the shared sources.
    let lists: Vec<WidgetId> =
        demo.ui.children(root).iter().filter_map(|&w| demo.ui.find_as::<SelectList>(w, tbid!("list"))).collect();
    assert_eq!(lists.len(), 2);
    let layout = demo.ui.get::<SelectList>(lists[0]).unwrap().item_layout().unwrap();
    assert_eq!(demo.ui.children(layout).len(), 100);
    assert_eq!(demo.ui.text(demo.ui.children(layout)[0]), Some("Adam"));

    // Checking a customized item changes the shared source.
    let row = item(&demo, lists[1], 0);
    let check = demo.ui.find(row, tbid!("check")).unwrap();
    click(&mut demo, check);
    assert!(demo.sources.advanced.borrow().item(0).unwrap().checked);
    let n = demo.sources.advanced.borrow().num_items();
    let del = demo.ui.find(row, tbid!("delete")).unwrap();
    click(&mut demo, del);
    assert_eq!(demo.sources.advanced.borrow().num_items(), n - 1);
    run(&mut demo, 100);

    // The scroll container window: a dropdown opens its menu below it.
    click_id(&mut demo, main, tbid!("test-scroll-container"));
    run(&mut demo, 400);
    let scw = top(&demo);
    let drop = demo.ui.find_as::<SelectDropdown>(scw, tbid!("name dropdown")).unwrap();
    assert_eq!(demo.ui.text(drop), Some("Maja"), "the source's first item");
    click(&mut demo, drop);
    run(&mut demo, 300);
    let menu = top(&demo);
    let list = demo.ui.get::<MenuWindow>(menu).expect("a menu window").list().unwrap();
    shot(&mut demo, &mut soft, "demo_m6_dropdown.png");
    // The list is scrolled to the selected name (the last visible one); choose the
    // one before it.
    let layout = demo.ui.get::<SelectList>(list).unwrap().item_layout().unwrap();
    let selected = item(&demo, list, 0);
    let kids = demo.ui.children(layout).to_vec();
    let next = kids[kids.iter().position(|&c| c == selected).unwrap() - 1];
    let name = demo.ui.text(next).unwrap().to_string();
    assert_ne!(name, "Maja");
    click(&mut demo, next);
    assert_eq!(demo.ui.text(drop), Some(name.as_str()));
    run(&mut demo, 400);
    assert!(!demo.ui.exists(menu));

    // The popup menu with a sub menu; choosing in the sub menu shows a message.
    let button = demo.ui.find(scw, tbid!("showpopupmenu1")).unwrap();
    click(&mut demo, button);
    run(&mut demo, 300);
    let menu = top(&demo);
    let list = demo.ui.get::<MenuWindow>(menu).expect("the popup menu").list().unwrap();
    let long = item(&demo, list, 4);
    click(&mut demo, long);
    run(&mut demo, 300);
    let submenu = top(&demo);
    assert!(submenu != menu);
    shot(&mut demo, &mut soft, "demo_m6_menu.png");
    let sublist = demo.ui.get::<MenuWindow>(submenu).unwrap().list().unwrap();
    let layout = demo.ui.get::<SelectList>(sublist).unwrap().item_layout().unwrap();
    let first = demo.ui.children(layout)[0];
    click(&mut demo, first);
    run(&mut demo, 400);
    assert!(!demo.ui.exists(menu) && !demo.ui.exists(submenu));
    let msg = top(&demo);
    assert!(demo.ui.get::<MessageWindow>(msg).is_some());
    assert_eq!(demo.ui.text(msg), Some("Info"));
    shot(&mut demo, &mut soft, "demo_m6_message.png");
    click_id(&mut demo, msg, tbid!("TBMessageWindow.ok"));
    run(&mut demo, 400);
    assert!(!demo.ui.exists(msg));

    // Closing the main window asks first, behind a dimmer.
    demo.ui.activate_window(main);
    run(&mut demo, 50);
    click_id(&mut demo, main, tbid!("TBWindow.close"));
    run(&mut demo, 400);
    let confirm = top(&demo);
    assert_eq!(demo.ui.text(confirm), Some("Are you sure?"));
    shot(&mut demo, &mut soft, "demo_m6_confirm.png");
    click_id(&mut demo, confirm, tbid!("TBMessageWindow.no"));
    run(&mut demo, 400);
    assert!(demo.ui.exists(main) && !demo.ui.exists(confirm));
    click_id(&mut demo, main, tbid!("TBWindow.close"));
    run(&mut demo, 400);
    let confirm = top(&demo);
    click_id(&mut demo, confirm, tbid!("TBMessageWindow.yes"));
    run(&mut demo, 600);
    assert!(!demo.ui.exists(main));
}

/// M7: the text window, list filters and the resource editor.
#[test]
fn demo_m7_windows_headless() {
    use turbo_ferret::widgets::{EditField, SelectItemSource, SelectList};
    let size = Size::new(1280, 800);
    let mut demo = Demo::new(&default_resource_root(), size, 96);
    let main = demo.main.unwrap();
    let root = demo.ui.root();
    let mut soft = SoftRenderer::new(size);
    let run = |demo: &mut Demo, ms: u64| {
        for _ in 0..ms / 16 {
            let t = demo.ui.now() + 16;
            demo.frame(t);
        }
    };
    let click = |demo: &mut Demo, w: WidgetId| {
        let r = demo.ui.rect(w);
        let p = demo.ui.to_root(w, Point::new(r.w / 2, r.h / 2));
        demo.ui.pointer_down(p, 1, Modifiers::NONE);
        demo.ui.pointer_up(p, Modifiers::NONE);
    };
    let type_text = |demo: &mut Demo, text: &str| {
        for c in text.chars() {
            demo.ui.key(Some(c), SpecialKey::None, Modifiers::NONE, true);
            demo.ui.key(Some(c), SpecialKey::None, Modifiers::NONE, false);
        }
    };
    run(&mut demo, 500);
    shot(&mut demo, &mut soft, "demo_m7_start.png");

    // The text window: styled text with embedded widgets.
    let edit = demo.ui.find_as::<EditField>(root, tbid!("editfield")).expect("the text window's field");
    let text_win = demo.ui.parent_window(edit).unwrap();
    assert!(demo.ui.text(edit).unwrap().starts_with("Turbo Ferret - a Rust port of Turbo Badger"));
    assert!(demo.ui.get::<EditField>(edit).unwrap().is_styling());
    let undo = demo.ui.find(text_win, tbid!("undo")).unwrap();
    assert!(demo.ui.has_state(undo, WidgetState::DISABLED), "nothing to undo yet");
    // Click into the text and type: the undo button wakes up, and undoes.
    let r = demo.ui.rect(edit);
    let p = demo.ui.to_root(edit, Point::new(r.w / 2, 12));
    demo.ui.pointer_down(p, 1, Modifiers::NONE);
    demo.ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(demo.ui.focused(), Some(edit));
    let before = demo.ui.text(edit).unwrap().to_string();
    type_text(&mut demo, "Hello! ");
    run(&mut demo, 50);
    assert!(demo.ui.text(edit).unwrap().contains("Hello! "));
    assert!(!demo.ui.has_state(undo, WidgetState::DISABLED));
    let info = demo.ui.find(text_win, tbid!("info")).unwrap();
    assert!(demo.ui.text(info).unwrap().starts_with("Caret ofs: "));
    shot(&mut demo, &mut soft, "demo_m7_text.png");
    click(&mut demo, undo);
    click(&mut demo, undo);
    run(&mut demo, 50);
    assert_eq!(demo.ui.text(edit), Some(before.as_str()));

    // Typing in a filter field filters its list.
    let filters: Vec<WidgetId> =
        demo.ui.children(root).iter().filter_map(|&w| demo.ui.find_as::<EditField>(w, tbid!("filter"))).collect();
    assert_eq!(filters.len(), 2);
    let filter = filters[0];
    let list_win = demo.ui.parent_window(filter).unwrap();
    let list = demo.ui.find_as::<SelectList>(list_win, tbid!("list")).unwrap();
    demo.ui.activate_window(list_win);
    click(&mut demo, filter);
    type_text(&mut demo, "ma");
    run(&mut demo, 50);
    let layout = demo.ui.get::<SelectList>(list).unwrap().item_layout().unwrap();
    let shown: Vec<String> =
        demo.ui.children(layout).iter().map(|&c| demo.ui.text(c).unwrap_or("").to_string()).collect();
    assert!(shown[0].starts_with("Showing "), "{shown:?}");
    assert!(shown.len() > 2 && shown.len() < 30);
    assert!(shown[1..].iter().all(|n| n.to_lowercase().contains("ma")), "{shown:?}");

    // The customized list adds the typed name.
    let add_name = demo.ui.find_as::<EditField>(root, tbid!("add_name")).unwrap();
    let adv_win = demo.ui.parent_window(add_name).unwrap();
    demo.ui.activate_window(adv_win);
    click(&mut demo, add_name);
    type_text(&mut demo, "Zebedee");
    let n = demo.sources.advanced.borrow().num_items();
    click_id(&mut demo, adv_win, tbid!("add"));
    assert_eq!(demo.sources.advanced.borrow().num_items(), n + 1);
    run(&mut demo, 50);
    shot(&mut demo, &mut soft, "demo_m7_lists.png");

    // The resource editor: the source on the left builds the widgets on the right.
    demo.ui.activate_window(main);
    run(&mut demo, 50);
    click_id(&mut demo, main, tbid!("test-resource-edit"));
    run(&mut demo, 400);
    let res_win = *demo.ui.children(root).last().unwrap();
    let source = demo.ui.find_as::<EditField>(res_win, tbid!("source_edit")).unwrap();
    let widget_list = demo.ui.find_as::<SelectList>(res_win, tbid!("widget_list")).unwrap();
    let sc = demo.ui.find(res_win, tbid!("scroll_container")).unwrap();
    let build = demo.ui.content_root(sc);
    assert!(demo.ui.text(source).unwrap().contains("TBLayout"));
    let built = demo.ui.children(build).len();
    assert!(built > 0);
    let items = |demo: &Demo| {
        let layout = demo.ui.get::<SelectList>(widget_list).unwrap().item_layout().unwrap();
        demo.ui.children(layout).len()
    };
    let listed = items(&demo);
    assert!(listed > 3, "the widget list shows the built widgets: {listed}");
    shot(&mut demo, &mut soft, "demo_m7_resource_edit.png");
    // Add a button at the end of the source: it appears, and the list grows.
    assert_eq!(demo.ui.focused(), Some(source));
    demo.ui.key(None, SpecialKey::End, Modifiers::CTRL, true);
    demo.ui.key(None, SpecialKey::Enter, Modifiers::NONE, true);
    type_text(&mut demo, "TBButton: text: \"Typed in\", id: typed");
    assert!(demo.ui.text(source).unwrap().contains("\nTBButton: text: \"Typed in\", id: typed"));
    run(&mut demo, 100);
    assert!(demo.ui.find(build, tbid!("typed")).is_some(), "the typed widget was built");
    assert!(items(&demo) > listed);
    // A press on a built widget selects it instead of clicking it.
    let typed = demo.ui.find(build, tbid!("typed")).unwrap();
    let p = demo.ui.to_root(typed, Point::new(3, 3));
    if demo.ui.widget_at(root, p).is_some_and(|w| demo.ui.is_ancestor_of(build, w)) {
        demo.ui.pointer_down(p, 1, Modifiers::NONE);
        demo.ui.pointer_up(p, Modifiers::NONE);
        run(&mut demo, 50);
        assert!(demo.ui.value(widget_list).as_int() >= 0, "selected in the list");
    }
    shot(&mut demo, &mut soft, "demo_m7_resource_edit2.png");
}

/// M8: TrueType fonts in the text window, the image window, the debug window.
#[test]
fn demo_m8_windows_headless() {
    use turbo_ferret::debug::{DebugSettings, DebugWindow};
    use turbo_ferret::widgets::{EditField, ImageWidget, MenuWindow, SelectList};
    let size = Size::new(1280, 800);
    let mut demo = Demo::new(&default_resource_root(), size, 96);
    let main = demo.main.unwrap();
    let root = demo.ui.root();
    let mut soft = SoftRenderer::new(size);
    let run = |demo: &mut Demo, ms: u64| {
        for _ in 0..ms / 16 {
            let t = demo.ui.now() + 16;
            demo.frame(t);
        }
    };
    let click = |demo: &mut Demo, w: WidgetId| {
        let r = demo.ui.rect(w);
        let p = demo.ui.to_root(w, Point::new(r.w / 2, r.h / 2));
        demo.ui.pointer_down(p, 1, Modifiers::NONE);
        demo.ui.pointer_up(p, Modifiers::NONE);
    };
    let top = |demo: &Demo| *demo.ui.children(root).last().unwrap();
    run(&mut demo, 500);

    // The text window's menu: a TrueType font, then the same with a glow.
    let edit = demo.ui.find_as::<EditField>(root, tbid!("editfield")).expect("the text window's field");
    let text_win = demo.ui.parent_window(edit).unwrap();
    demo.ui.activate_window(text_win);
    let segoe = demo.ui.calculated_font(edit);
    let segoe_width = demo.ui.text_width(segoe, "Turbo Ferret");
    for (item_id, font, shot_name) in [
        (tbid!("ttf font Vera"), tbid!("Vera"), "demo_m8_truetype.png"),
        (tbid!("ttf font VeraGlow"), tbid!("VeraGlowText"), "demo_m8_truetype_glow.png"),
    ] {
        click_id(&mut demo, text_win, tbid!("menu"));
        run(&mut demo, 300);
        let menu = top(&demo);
        let list = demo.ui.get::<MenuWindow>(menu).expect("the text window's menu").list().unwrap();
        let index = {
            let source = demo.ui.get::<SelectList>(list).unwrap().source().unwrap();
            let source = source.borrow();
            (0..source.num_items()).find(|&i| source.item_id(i) == item_id).expect("the menu item")
        };
        let item = demo.ui.get::<SelectList>(list).unwrap().item_widget(&demo.ui, index as i32).unwrap();
        click(&mut demo, item);
        run(&mut demo, 400);
        assert!(!demo.ui.exists(menu));
        let fd = demo.ui.calculated_font(edit);
        assert_eq!(fd.id, font);
        let face = demo.ui.fonts().face(fd);
        assert_eq!(face.desc(), fd, "the TrueType face was created");
        assert!(!face.is_box_font());
        assert_ne!(demo.ui.text_width(fd, "Turbo Ferret"), segoe_width, "another font, other widths");
        shot(&mut demo, &mut soft, shot_name);
    }
    // The glow: a blurred face behind the text face.
    let fd = demo.ui.calculated_font(edit);
    let background = demo.ui.fonts().face(fd).background().expect("a background font");
    assert_eq!(demo.ui.fonts().face(background.desc).effect().blur_radius(), 3);

    // The image window: nine images from files, each loaded once.
    assert!(demo.ui.images().is_empty());
    click_id(&mut demo, main, tbid!("test-image"));
    run(&mut demo, 400);
    let image_win = top(&demo);
    assert_eq!(demo.ui.text(image_win), Some("ImageWidget"));
    let images = |demo: &Demo| -> Vec<WidgetId> {
        let mut found = Vec::new();
        let mut stack = vec![image_win];
        while let Some(w) = stack.pop() {
            if demo.ui.get::<ImageWidget>(w).is_some() {
                found.push(w);
            }
            stack.extend(demo.ui.children(w).iter().rev());
        }
        found
    };
    let widgets = images(&demo);
    assert_eq!(widgets.len(), 9);
    assert_eq!(demo.ui.images().len(), 9);
    let first = demo.ui.get::<ImageWidget>(widgets[0]).unwrap().image().clone();
    assert_eq!(first.size(), Size::new(256, 171));
    assert_eq!(first.name(), "demo01/images/image_1.png");
    drop(first);
    shot(&mut demo, &mut soft, "demo_m8_images.png");
    // The button on an image removes it, and the image is unloaded.
    click_id(&mut demo, widgets[0], tbid!("remove"));
    run(&mut demo, 100);
    shot(&mut demo, &mut soft, "demo_m8_images_removed.png");
    assert_eq!(images(&demo).len(), 8);
    assert_eq!(demo.ui.images().len(), 8);
    // Closing the window unloads the rest.
    demo.ui.close_window(image_win);
    run(&mut demo, 600);
    shot(&mut demo, &mut soft, "demo_m8_images_closed.png");
    assert!(!demo.ui.exists(image_win));
    assert!(demo.ui.images().is_empty());

    // The debug window.
    click_id(&mut demo, main, tbid!("debug settings"));
    run(&mut demo, 400);
    let debug_win = top(&demo);
    assert!(demo.ui.get::<DebugWindow>(debug_win).is_some());
    assert_eq!(demo.ui.text(debug_win), Some("Debug settings"));
    click_id(&mut demo, debug_win, tbid!("debug.layout_bounds"));
    click_id(&mut demo, debug_win, tbid!("debug.font_bitmap_fragments"));
    let s = demo.ui.debug_settings();
    assert_eq!(s, DebugSettings { layout_bounds: true, font_bitmap_fragments: true, ..Default::default() });
    // Something happens elsewhere: it shows in the event log.
    demo.ui.activate_window(main);
    click_id(&mut demo, main, tbid!("test-connections"));
    run(&mut demo, 100);
    let output = demo.ui.find(debug_win, tbid!("output")).unwrap();
    let log = demo.ui.text(output).unwrap().to_string();
    assert!(log.contains("CLICK (Button) id: \"test-connections\""), "{log}");
    demo.ui.activate_window(debug_win);
    run(&mut demo, 400);
    shot(&mut demo, &mut soft, "demo_m8_debug.png");
    click_id(&mut demo, debug_win, tbid!("debug.layout_bounds"));
    click_id(&mut demo, debug_win, tbid!("debug.font_bitmap_fragments"));
    click_id(&mut demo, debug_win, tbid!("debug.render_batches"));
    run(&mut demo, 50);
    shot(&mut demo, &mut soft, "demo_m8_debug_batches.png");
    let stats = demo.ui.debug_frame_stats();
    assert!(stats.batches > 0 && stats.triangles > 1000, "{stats:?}");
}

/// What the demo's host adds, as TB's demo does: the mouse as a finger while a
/// modifier is held, the skin reload timing, and the frame counter.
#[test]
fn demo_host_extras_headless() {
    use turbo_ferret::widgets::{EditField, MenuWindow, MessageWindow};
    let size = Size::new(1280, 800);
    let mut demo = Demo::new(&default_resource_root(), size, 96);
    let main = demo.main.unwrap();
    let root = demo.ui.root();
    let mut soft = SoftRenderer::new(size);
    let run = |demo: &mut Demo, ms: u64| {
        for _ in 0..ms / 16 {
            let t = demo.ui.now() + 16;
            demo.frame(t);
        }
    };
    let top = |demo: &Demo| *demo.ui.children(root).last().unwrap();
    let center = |demo: &Demo, w: WidgetId| {
        let r = demo.ui.rect(w);
        demo.ui.to_root(w, Point::new(r.w / 2, r.h / 2))
    };
    run(&mut demo, 500);

    // Shift, ctrl or alt make the mouse a finger; nothing else does.
    assert!(Demo::emulates_touch(Modifiers::SHIFT) && Demo::emulates_touch(Modifiers::ALT | Modifiers::CTRL));
    assert!(!Demo::emulates_touch(Modifiers::NONE) && !Demo::emulates_touch(Modifiers::SUPER));

    // A finger that is not down does not hover (and the plain mouse is not a finger).
    let button = demo.ui.find(main, tbid!("test-connections")).unwrap();
    let p = center(&demo, button);
    assert!(!demo.finger_move(p, Modifiers::NONE), "no modifier: the mouse handles it");
    assert!(demo.finger_move(p, Modifiers::SHIFT), "swallowed");
    assert_eq!(demo.ui.hovered(), None);
    assert!(!demo.finger_up(p, Modifiers::SHIFT), "it was not down");

    // A tap clicks.
    let windows = demo.ui.children(root).len();
    assert!(demo.finger_down(p, 1, Modifiers::SHIFT));
    assert!(demo.finger_move(p, Modifiers::SHIFT));
    assert!(demo.finger_up(p, Modifiers::NONE), "released as a finger although the key is up by now");
    run(&mut demo, 300);
    assert_eq!(demo.ui.children(root).len(), windows + 1, "the connections window opened");
    demo.ui.close_window(top(&demo));
    run(&mut demo, 600);

    // Holding a finger on a text field opens its context menu (the mouse needs the
    // right button for that).
    let edit = demo.ui.find_as::<EditField>(root, tbid!("editfield")).unwrap();
    let text_win = demo.ui.parent_window(edit).unwrap();
    demo.ui.activate_window(text_win);
    let p = demo.ui.to_root(edit, Point::new(40, 12));
    assert!(demo.finger_down(p, 1, Modifiers::ALT));
    run(&mut demo, 1200);
    let menu = top(&demo);
    assert!(demo.ui.get::<MenuWindow>(menu).is_some(), "the long press opened the context menu");
    demo.finger_up(p, Modifiers::ALT);
    demo.ui.close_window(menu);
    run(&mut demo, 600);

    // Reloading the skin's bitmaps reports how long it took, and the skin still draws.
    demo.ui.activate_window(main);
    run(&mut demo, 100);
    shot(&mut demo, &mut soft, "demo_extras_before_reload.png");
    let before: Vec<u8> = soft.pixels().to_vec();
    let reload = demo.ui.find(main, tbid!("reload skin bitmaps")).unwrap();
    demo.ui.invoke_event(Event::new(EventKind::Click, reload));
    run(&mut demo, 100);
    let msg = top(&demo);
    assert!(demo.ui.get::<MessageWindow>(msg).is_some());
    assert_eq!(demo.ui.text(msg), Some("GFX load performance"));
    let texts: Vec<String> = demo
        .ui
        .children(demo.ui.content_root(msg))
        .iter()
        .filter_map(|&c| demo.ui.find_as::<EditField>(c, TbId::NONE).or(Some(c)))
        .filter_map(|c| demo.ui.text(c).map(str::to_string))
        .collect();
    assert!(texts.iter().any(|t| t.starts_with("Reloading the skin graphics 10 times took ")), "{texts:?}");
    demo.ui.close_window(msg);
    run(&mut demo, 800);
    shot(&mut demo, &mut soft, "demo_extras_after_reload.png");
    // The same picture, apart from the frame counter in the corner.
    let differing = before
        .chunks_exact(4)
        .zip(soft.pixels().chunks_exact(4))
        .enumerate()
        .filter(|(i, (a, b))| a != b && !((i % 1280) < 200 && (i / 1280) < 30))
        .count();
    assert_eq!(differing, 0, "the skin looks the same after reloading");
    let corner = before.chunks_exact(4).zip(soft.pixels().chunks_exact(4)).filter(|(a, b)| a != b).count();
    assert!(corner > 0, "the frame number changed");
}
