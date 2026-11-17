//! A widget that shows an image of the image manager.
//! TB: image/tb_image_widget.h, .cpp (TBImageWidget)

use crate::images::ImageHandle;
use crate::layout::{PreferredSize, SizeConstraints};
use crate::paint::PaintCtx;
use crate::res::ResNode;
use crate::ui::Ctx;
use crate::widget::Widget;

/// Shows an image loaded by the image manager, at the image's size unless its skin
/// or layout parameters say otherwise. For an image from the skin, use
/// [`SkinImage`](super::SkinImage). In a resource: `TBImageWidget: filename: "a.png"`.
/// TB: TBImageWidget
#[derive(Default)]
pub struct ImageWidget {
    image: ImageHandle,
}

impl ImageWidget {
    pub fn new() -> ImageWidget {
        ImageWidget::default()
    }

    /// A widget showing `image`.
    pub fn with_image(image: ImageHandle) -> ImageWidget {
        ImageWidget { image }
    }

    /// The image shown (empty if none is set or it failed to load).
    pub fn image(&self) -> &ImageHandle {
        &self.image
    }

    /// Show `image`. The widget keeps it loaded. TB: TBImageWidget::SetImage(const TBImage &)
    pub fn set_image(&mut self, cx: &mut Ctx, image: ImageHandle) {
        if self.image == image {
            return;
        }
        let resized = self.image.size() != image.size();
        self.image = image;
        let me = cx.this();
        if resized {
            cx.invalidate_layout(me);
        }
        cx.invalidate();
    }

    /// Show the image in the file `filename` (loaded through the image manager).
    /// TB: TBImageWidget::SetImage(const char *filename)
    pub fn set_image_file(&mut self, cx: &mut Ctx, filename: &str) {
        let image = cx.image(filename);
        self.set_image(cx, image);
    }
}

impl Widget for ImageWidget {
    fn type_name(&self) -> &'static str {
        "ImageWidget"
    }

    // TB: tb_widgets_reader.cpp TBImageWidget::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        if let Some(filename) = res.get("filename") {
            let filename = filename.value.as_str();
            self.set_image_file(cx, &filename);
        }
    }

    /// The image's size, and nothing else. TB: TBImageWidget::OnCalculatePreferredContentSize
    fn measure_content(&mut self, _cx: &mut Ctx, _sc: SizeConstraints) -> PreferredSize {
        let size = self.image.size();
        PreferredSize { min: size, max: size, pref: size, ..Default::default() }
    }

    // TB: TBImageWidget::OnPaint
    fn paint(&self, p: &mut PaintCtx) {
        p.draw_image(p.padding_rect(), &self.image);
    }
}
