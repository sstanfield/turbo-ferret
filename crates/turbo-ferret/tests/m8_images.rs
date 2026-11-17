//! M8: the image manager (shared, reference counted images in an atlas of their
//! own) and the image widget.
//! TB: image/tb_image_manager.cpp, image/tb_image_widget.cpp

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::geom::{Rect, Size};
use turbo_ferret::layout::dim::DimensionConverter;
use turbo_ferret::widgets::ImageWidget;
use turbo_ferret::*;

/// A "file format" for the tests: the file `4x3` is a 4 by 3 image of one
/// half-transparent color.
struct SizeDecoder;

impl ImageDecoder for SizeDecoder {
    fn decode(&mut self, bytes: &[u8]) -> Result<Image, DecodeError> {
        let s = std::str::from_utf8(bytes).map_err(|_| DecodeError::UnknownFormat)?;
        let (w, h) = s.split_once('x').ok_or(DecodeError::UnknownFormat)?;
        let (w, h): (i32, i32) =
            (w.parse().map_err(|_| DecodeError::UnknownFormat)?, h.parse().map_err(|_| DecodeError::UnknownFormat)?);
        Ok(Image { size: Size::new(w, h), rgba: [200, 100, 50, 128].repeat((w * h) as usize) })
    }
}

/// A loader that records what was asked for.
struct Counting {
    files: MemLoader,
    loads: Rc<RefCell<Vec<String>>>,
}

impl ResourceLoader for Counting {
    fn load(&mut self, path: &str) -> Result<std::borrow::Cow<'static, [u8]>, LoadError> {
        self.loads.borrow_mut().push(path.to_string());
        self.files.load(path)
    }
}

fn ui_with(files: &[(&str, &str)]) -> (Ui, Rc<RefCell<Vec<String>>>) {
    let mut mem = MemLoader::new();
    for (name, content) in files {
        mem.add_owned(name, content.as_bytes().to_vec());
    }
    let loads = Rc::new(RefCell::new(Vec::new()));
    let mut services = Services::with_loader(Counting { files: mem, loads: loads.clone() });
    services.images = Box::new(SizeDecoder);
    services.log = None;
    let mut ui = Ui::new(Config::default(), services);
    ui.set_viewport(Size::new(400, 300));
    (ui, loads)
}

#[test]
fn a_file_is_loaded_once_and_shared() {
    let (mut ui, loads) = ui_with(&[("a.img", "40x30"), ("b.img", "8x8")]);
    assert!(ui.images().is_empty());
    let a = ui.image("a.img");
    assert!(!a.is_empty());
    assert_eq!((a.width(), a.height(), a.size(), a.name()), (40, 30, Size::new(40, 30), "a.img"));
    let again = ui.image("a.img");
    assert_eq!(a, again);
    assert_eq!(*loads.borrow(), ["a.img"], "loaded once");
    let b = ui.image("b.img");
    assert_ne!(a, b);
    assert_eq!(ui.images().len(), 2);
    assert_eq!(a.clone(), a);
    // Both are in the manager's atlas, apart from each other.
    let (fa, fb) = (ui.images().fragment(&a).unwrap(), ui.images().fragment(&b).unwrap());
    assert_eq!((fa.rect.size(), fb.rect.size()), (Size::new(40, 30), Size::new(8, 8)));
    assert!(fa.page != fb.page || !fa.rect.intersects(&fb.rect));
    // The pixels are premultiplied in the atlas.
    let page = ui.images().atlas().page_pixels(fa.page).unwrap();
    let at = ((fa.rect.y * ui.images().atlas().page_size(fa.page).w + fa.rect.x) * 4) as usize;
    assert_eq!(page[at..at + 4], [100, 50, 25, 128]);
}

#[test]
fn a_file_that_does_not_load_gives_an_empty_image_and_is_tried_again() {
    let (mut ui, loads) = ui_with(&[("bad.img", "not an image")]);
    for name in ["missing.img", "bad.img"] {
        let img = ui.image(name);
        assert!(img.is_empty());
        assert_eq!((img.width(), img.height(), img.name()), (0, 0, ""));
        assert_eq!(img, ImageHandle::default());
        assert!(ui.images().fragment(&img).is_none());
    }
    assert!(ui.images().is_empty());
    ui.image("missing.img");
    assert_eq!(loads.borrow().iter().filter(|p| *p == "missing.img").count(), 2, "not remembered as failed");
}

#[test]
fn an_image_is_unloaded_when_its_last_handle_goes() {
    let (mut ui, loads) = ui_with(&[("a.img", "40x30"), ("b.img", "40x30")]);
    let a = ui.image("a.img");
    let copy = a.clone();
    let place = ui.images().fragment(&a).unwrap();
    drop(a);
    assert_eq!(ui.images().len(), 1, "a copy still holds it");
    assert_eq!(ui.image("a.img"), copy);
    drop(copy);
    assert_eq!(ui.images().len(), 0);
    assert!(ui.images().loaded("a.img").is_none());
    // Its space is free again: the next image of that size takes it.
    let b = ui.image("b.img");
    assert_eq!(ui.images().fragment(&b).unwrap(), place);
    // And the file is loaded again when asked for.
    let a = ui.image("a.img");
    assert!(!a.is_empty());
    assert_eq!(*loads.borrow(), ["a.img", "b.img", "a.img"]);
}

#[test]
fn images_from_pixels_are_shared_by_name() {
    let (mut ui, loads) = ui_with(&[]);
    let px = [255u8, 0, 0, 255, 0, 255, 0, 255];
    let a = ui.image_from_pixels("made", Size::new(2, 1), &px);
    assert_eq!(a.size(), Size::new(2, 1));
    // The name wins: the pixels given the second time are not used.
    let b = ui.image_from_pixels("made", Size::new(1, 1), &[0, 0, 0, 0]);
    assert_eq!(a, b);
    // It is found by `image` too, without touching the loader.
    assert_eq!(ui.image("made"), a);
    assert!(loads.borrow().is_empty());
    // Too few bytes, or no size.
    assert!(ui.image_from_pixels("short", Size::new(2, 2), &px).is_empty());
    assert!(ui.image_from_pixels("none", Size::new(0, 5), &px).is_empty());
    assert_eq!(ui.images().len(), 1);
}

#[test]
fn a_large_image_gets_a_page_of_its_own() {
    let (mut ui, _) = ui_with(&[("big.img", "600x40"), ("small.img", "10x10")]);
    let small = ui.image("small.img");
    let big = ui.image("big.img");
    let (fs, fb) = (ui.images().fragment(&small).unwrap(), ui.images().fragment(&big).unwrap());
    assert_ne!(fs.page, fb.page);
    assert_eq!(ui.images().atlas().page_size(fs.page), Size::new(512, 512));
    assert_eq!(ui.images().atlas().page_size(fb.page), Size::new(600, 40));
}

#[test]
fn the_file_for_the_screen_dpi_is_preferred() {
    let mut files = MemLoader::new();
    files.add_owned("a.img", b"10x10".to_vec());
    files.add_owned("a@192.img", b"20x20".to_vec());
    files.add_owned("b.img", b"10x10".to_vec());
    let mut decoder = SizeDecoder;
    let mut images = ImageManager::new();
    let hidpi = DimensionConverter::new(96, 192, 192);
    assert!(hidpi.need_conversion());
    assert_eq!(images.get("a.img", &mut files, &mut decoder, &hidpi).size(), Size::new(20, 20));
    // No file for that DPI: the plain one.
    assert_eq!(images.get("b.img", &mut files, &mut decoder, &hidpi).size(), Size::new(10, 10));
    let mut images = ImageManager::new();
    let same = DimensionConverter::new(96, 96, 96);
    assert_eq!(images.get("a.img", &mut files, &mut decoder, &same).size(), Size::new(10, 10));
}

/// A renderer that records the quads drawn with a bitmap.
#[derive(Default)]
struct Rec {
    next: u32,
    created: Vec<(BitmapId, Size)>,
    draws: Vec<(BitmapId, Quad)>,
}

impl Renderer for Rec {
    fn begin_frame(&mut self, _viewport: Size) {
        self.draws.clear();
    }
    fn end_frame(&mut self) {}
    fn create_bitmap(&mut self, size: Size, _rgba: &[u8]) -> Option<BitmapId> {
        self.next += 1;
        self.created.push((BitmapId(self.next), size));
        Some(BitmapId(self.next))
    }
    fn update_bitmap(&mut self, _bitmap: BitmapId, _rgba: &[u8]) {}
    fn destroy_bitmap(&mut self, _bitmap: BitmapId) {}
    fn draw(&mut self, bitmap: Option<BitmapId>, _clip: Rect, quads: &[Quad]) {
        if let Some(b) = bitmap {
            self.draws.extend(quads.iter().map(|q| (b, *q)));
        }
    }
}

#[test]
fn the_image_widget_takes_the_size_of_its_image_and_draws_it() {
    let (mut ui, loads) = ui_with(&[("a.img", "40x30"), ("b.img", "8x8")]);
    let root = ui.root();
    let ids = ui
        .inflate(
            root,
            "TBLayout: axis: y, id: col\n\tTBImageWidget: id: one, filename: \"a.img\"\n\
             \tTBImageWidget: id: two, filename: \"a.img\"\n\tTBImageWidget: id: none\n",
        )
        .unwrap();
    let col = ids[0];
    let one = ui.find(col, tbid!("one")).unwrap();
    let two = ui.find(col, tbid!("two")).unwrap();
    let none = ui.find(col, tbid!("none")).unwrap();
    assert_eq!(*loads.borrow(), ["a.img"], "two widgets, one load");
    assert_eq!(ui.images().len(), 1);
    assert_eq!(ui.get::<ImageWidget>(one).unwrap().image(), ui.get::<ImageWidget>(two).unwrap().image());
    assert!(ui.get::<ImageWidget>(none).unwrap().image().is_empty());

    // Exactly the image's size.
    let ps = ui.preferred_size_free(one);
    assert_eq!((ps.min, ps.pref, ps.max), (Size::new(40, 30), Size::new(40, 30), Size::new(40, 30)));
    assert_eq!(ui.preferred_size_free(none).pref, Size::ZERO);
    ui.set_rect(col, Rect::new(10, 10, 200, 200));
    ui.update();
    assert_eq!(ui.rect(one).size(), Size::new(40, 30));

    // It draws the image's part of the atlas into its rect.
    let mut r = Rec::default();
    ui.paint(&mut r);
    let image = ui.get::<ImageWidget>(one).unwrap().image().clone();
    let frag = ui.images().fragment(&image).unwrap();
    assert_eq!(r.created.len(), 1, "one atlas page");
    assert_eq!(r.draws.len(), 2, "the two widgets with an image");
    let (bitmap, quad) = r.draws[0];
    assert_eq!(bitmap, r.created[0].0);
    assert_eq!(quad.src, frag.rect);
    assert_eq!(quad.dst, Rect::from_origin_size(ui.to_root(one, Point::ZERO), Size::new(40, 30)));
    drop(image);

    // Another image: the widget asks for a new layout and the old image stays for
    // the other widget.
    ui.with_widget::<ImageWidget, _>(one, |w, cx| w.set_image_file(cx, "b.img"));
    ui.update();
    assert_eq!(ui.rect(one).size(), Size::new(8, 8));
    assert_eq!(ui.images().len(), 2);

    // Removing the widgets unloads their images.
    ui.remove(col);
    ui.update();
    ui.paint(&mut r);
    assert_eq!(ui.images().len(), 0);
    assert!(r.draws.is_empty());
}

#[test]
fn an_image_can_be_set_from_code() {
    let (mut ui, _) = ui_with(&[]);
    let image = ui.image_from_pixels("dot", Size::new(3, 2), &[255; 24]);
    let root = ui.root();
    let w = ui.add(root, ImageWidget::with_image(image.clone()));
    assert_eq!(ui.preferred_size_free(w).pref, Size::new(3, 2));
    ui.with_widget::<ImageWidget, _>(w, |w, cx| w.set_image(cx, ImageHandle::default()));
    assert_eq!(ui.preferred_size_free(w).pref, Size::ZERO);
    drop(image);
    assert_eq!(ui.images().len(), 0);
}
