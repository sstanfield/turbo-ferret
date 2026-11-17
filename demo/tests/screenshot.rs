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
