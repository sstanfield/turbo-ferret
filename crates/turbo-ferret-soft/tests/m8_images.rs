//! M8: golden image of image widgets: TB's demo images in the demo skin's frame,
//! one at its own size and one squeezed by its layout parameters.
//! Bless with `BLESS=1 cargo test -p turbo-ferret-soft`.

mod common;

use common::assert_golden;
use turbo_ferret::geom::{Color, Rect, Size};
use turbo_ferret::widgets::ImageWidget;
use turbo_ferret::*;
use turbo_ferret_soft::SoftRenderer;

const SRC: &str = r#"
TBLayout: spacing: 12
	TBImageWidget: id: full, filename: "demo01/images/image_1.png", skin: ImageFrame
	TBLayout: axis: y, spacing: 12
		TBImageWidget: id: small, filename: "demo01/images/image_9.png", skin: ImageFrame
			lp: width: 60, height: 90
		TBImageWidget: id: again, filename: "demo01/images/image_1.png"
			lp: width: 96, height: 64
"#;

#[test]
fn image_widgets_golden() {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let size = Size::new(410, 200);
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", Some("demo01/skin/skin.tb.txt")).unwrap();
    ui.set_viewport(size);
    let root = ui.root();
    let ids = ui.inflate(root, SRC).unwrap();
    let r = ui.fit_content_rect(ids[0], widgets::ResizeFit::Preferred);
    ui.set_rect(ids[0], Rect::new(14, 14, r.w, r.h));
    ui.update();

    let full = ui.find(root, tbid!("full")).unwrap();
    let again = ui.find(root, tbid!("again")).unwrap();
    assert_eq!(ui.rect(full).size(), Size::new(256, 171), "the image's own size");
    assert_eq!(ui.images().len(), 2, "two files for three widgets");
    assert_eq!(ui.get::<ImageWidget>(full).unwrap().image(), ui.get::<ImageWidget>(again).unwrap().image());

    let mut r = SoftRenderer::new(size);
    r.clear(Color::rgb(40, 44, 52));
    ui.paint(&mut r);
    assert_golden("image_widgets", &r);
}
