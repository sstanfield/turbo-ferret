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
