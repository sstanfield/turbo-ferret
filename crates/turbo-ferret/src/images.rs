//! The image manager: images loaded by file name, shared by handles, and kept in an
//! atlas of their own for as long as a handle exists.
//!
//! TB: image/tb_image_manager.h, .cpp (TBImageManager, TBImage, TBImageRep). TB's
//! `TBImage` told the manager when its last copy went away. Here a handle is an `Rc`
//! and the manager keeps a `Weak`: images nobody holds any more are dropped the
//! next time one is loaded or a frame is painted.

use alloc::collections::BTreeMap;
use alloc::rc::{Rc, Weak};
use alloc::string::String;
use core::fmt;

use crate::geom::Size;
use crate::layout::dim::DimensionConverter;
use crate::render::atlas::{Atlas, FragmentId};
use crate::services::{ImageDecoder, ResourceLoader};

/// Size of the pages small images share. A larger image gets a page of its own.
/// TB: TBBitmapFragmentManager's default map size
pub const IMAGE_PAGE_SIZE: Size = Size::new(512, 512);

/// What a handle points to. TB: TBImageRep
struct ImageRep {
    name: String,
    size: Size,
}

/// A loaded image. Clones share it; it stays loaded while any clone exists. The
/// default handle is empty. TB: TBImage
#[derive(Clone, Default)]
pub struct ImageHandle(Option<Rc<ImageRep>>);

impl ImageHandle {
    /// True if no image is set (or loading it failed). TB: TBImage::IsEmpty
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// Width in pixels, 0 if empty. TB: TBImage::Width
    pub fn width(&self) -> i32 {
        self.size().w
    }

    /// Height in pixels, 0 if empty. TB: TBImage::Height
    pub fn height(&self) -> i32 {
        self.size().h
    }

    pub fn size(&self) -> Size {
        self.0.as_ref().map_or(Size::ZERO, |r| r.size)
    }

    /// The name it was loaded under (a file name, or the name given with pixels).
    pub fn name(&self) -> &str {
        self.0.as_ref().map_or("", |r| &r.name)
    }
}

/// Two handles are equal if they share one image (or are both empty).
/// TB: TBImage::operator ==
impl PartialEq for ImageHandle {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Eq for ImageHandle {}

impl fmt::Debug for ImageHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(r) => write!(f, "ImageHandle({:?}, {}x{})", r.name, r.size.w, r.size.h),
            None => write!(f, "ImageHandle(empty)"),
        }
    }
}

/// Loads images and keeps track of which are loaded, so a file is loaded once
/// however many widgets show it. TB: TBImageManager
pub struct ImageManager {
    atlas: Atlas,
    images: BTreeMap<String, (Weak<ImageRep>, FragmentId)>,
}

impl fmt::Debug for ImageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageManager")
            .field("images", &self.images.keys())
            .field("pages", &self.atlas.page_count())
            .finish()
    }
}

impl Default for ImageManager {
    fn default() -> Self {
        ImageManager::new()
    }
}

impl ImageManager {
    pub fn new() -> ImageManager {
        ImageManager { atlas: Atlas::new(IMAGE_PAGE_SIZE), images: BTreeMap::new() }
    }

    /// The image for a file name: the one already loaded, or loaded now. With a
    /// screen DPI other than the skin's, the file for that DPI (`name@192.png`) is
    /// tried first. Empty if it cannot be loaded or decoded (and tried again next
    /// time). TB: TBImageManager::GetImage(filename)
    pub fn get(
        &mut self,
        filename: &str,
        loader: &mut dyn ResourceLoader,
        decoder: &mut dyn ImageDecoder,
        dim: &DimensionConverter,
    ) -> ImageHandle {
        if let Some(h) = self.loaded(filename) {
            return h;
        }
        let mut load = |file: &str| {
            let bytes = loader.load(file).ok()?;
            decoder.decode(&bytes).ok()
        };
        // Load a destination DPI bitmap if available.
        let dpi_image = if dim.need_conversion() { load(&dim.dst_dpi_filename(filename)) } else { None };
        match dpi_image.or_else(|| load(filename)) {
            Some(img) => self.insert(filename, img.size, img.rgba),
            None => ImageHandle::default(),
        }
    }

    /// The image called `name`: the one already there, or a new one made from
    /// straight-alpha RGBA8 pixels (`size.w * size.h * 4` bytes, row by row). Empty
    /// if the size is empty or there are too few bytes.
    /// TB: TBImageManager::GetImage(name, buffer, width, height)
    pub fn get_from_pixels(&mut self, name: &str, size: Size, rgba: &[u8]) -> ImageHandle {
        if let Some(h) = self.loaded(name) {
            return h;
        }
        self.insert(name, size, rgba.to_vec())
    }

    /// The image under `name`, if a handle to it still exists.
    pub fn loaded(&self, name: &str) -> Option<ImageHandle> {
        let rep = self.images.get(name)?.0.upgrade()?;
        Some(ImageHandle(Some(rep)))
    }

    fn insert(&mut self, name: &str, size: Size, mut rgba: alloc::vec::Vec<u8>) -> ImageHandle {
        // Make room first: images nobody holds any more.
        self.collect();
        let len = size.w.max(0) as usize * size.h.max(0) as usize * 4;
        if len == 0 || rgba.len() < len {
            return ImageHandle::default();
        }
        rgba.truncate(len);
        crate::skin::load::premultiply(&mut rgba);
        let Some(frag) = self.atlas.add(size, &rgba) else {
            return ImageHandle::default();
        };
        let rep = Rc::new(ImageRep { name: String::from(name), size });
        self.images.insert(String::from(name), (Rc::downgrade(&rep), frag));
        ImageHandle(Some(rep))
    }

    /// Forget the images no handle refers to and free their space.
    /// TB: TBImageManager::RemoveImageRep (called there when the last TBImage went)
    pub fn collect(&mut self) {
        let atlas = &mut self.atlas;
        self.images.retain(|_, (weak, frag)| {
            let alive = weak.strong_count() > 0;
            if !alive {
                atlas.remove(*frag);
            }
            alive
        });
    }

    /// Number of images that are loaded and held by a handle.
    pub fn len(&self) -> usize {
        self.images.values().filter(|(w, _)| w.strong_count() > 0).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The atlas the images are in.
    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// Where `image` is in the atlas, if it is one of this manager's images.
    pub fn fragment(&self, image: &ImageHandle) -> Option<FragmentId> {
        let rep = image.0.as_ref()?;
        let (weak, frag) = self.images.get(&rep.name)?;
        core::ptr::eq(weak.as_ptr(), Rc::as_ptr(rep)).then_some(*frag)
    }

    /// Before painting: drop unused images and create or update the page bitmaps.
    pub(crate) fn begin_frame(&mut self, r: &mut dyn crate::render::Renderer) {
        self.collect();
        self.atlas.upload_all(r);
    }

    /// TB: TBImageManager::OnContextLost
    pub fn renderer_lost(&mut self) {
        self.atlas.renderer_lost();
    }

    /// Destroy the page bitmaps (they are re-created on the next paint).
    /// TB: TBImageManager::OnContextLost (TBBitmapFragmentManager::DeleteBitmaps)
    pub fn destroy_bitmaps(&mut self, r: &mut dyn crate::render::Renderer) {
        self.atlas.destroy_bitmaps(r);
    }
}
