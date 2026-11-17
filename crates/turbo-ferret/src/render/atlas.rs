//! Texture atlas: packs many small premultiplied RGBA8 images into a few large
//! bitmaps ("pages") so drawing them needs fewer bitmap switches.
//!
//! Each page is sliced into horizontal rows ("shelves"); each row hands out
//! horizontal space with a first-fit-by-size space allocator. Pages keep a CPU copy
//! of their pixels and upload it through the [`Renderer`] when asked (create the
//! first time, update when dirty). Fragments are plain values (no back pointers,
//! H-03), and context loss is a call ([`Atlas::renderer_lost`], H-22).
//!
//! TB: tb_bitmap_fragment.h, tb_bitmap_fragment.cpp (TBSpaceAllocator,
//! TBFragmentSpaceAllocator, TBBitmapFragmentMap, TBBitmapFragmentManager)

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::geom::{Rect, Size};
use crate::render::{BitmapId, Renderer};

/// A fragment allocated in an [`Atlas`]: the page it lives on and its texels in that
/// page (excluding the gutter). A plain value; use `rect` as the source rect origin
/// when drawing the page bitmap.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FragmentId {
    pub page: u16,
    pub rect: Rect,
}

/// A chunk of horizontal space handed out by a [`SpaceAllocator`].
/// TB: TBSpaceAllocator::Space
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Space {
    pub x: i32,
    pub width: i32,
}

/// Allocates horizontal space out of `0..width`. Free spaces are kept sorted by `x`
/// and merged when adjacent. TB: TBSpaceAllocator
#[derive(Clone, Debug)]
pub(crate) struct SpaceAllocator {
    available: i32,
    free: Vec<Space>,
    used: Vec<Space>,
}

impl SpaceAllocator {
    pub fn new(width: i32) -> SpaceAllocator {
        // TB creates the all-covering free space lazily in GetSmallestAvailableSpace;
        // creating it up front is equivalent.
        let free = if width > 0 { vec![Space { x: 0, width }] } else { Vec::new() };
        SpaceAllocator { available: width.max(0), free, used: Vec::new() }
    }

    /// True if no allocations are live. TB: TBSpaceAllocator::IsAllAvailable
    pub fn is_all_available(&self) -> bool {
        self.used.is_empty()
    }

    /// TB: TBSpaceAllocator::HasSpace
    pub fn has_space(&self, needed_w: i32) -> bool {
        if needed_w > self.available {
            return false;
        }
        self.free.iter().any(|fs| needed_w <= fs.width)
    }

    /// Allocate `needed_w` from the smallest free space that fits.
    /// TB: TBSpaceAllocator::AllocSpace
    pub fn alloc_space(&mut self, needed_w: i32) -> Option<Space> {
        if needed_w <= 0 {
            return None;
        }
        let i = self.smallest_available_space(needed_w)?;
        let fs = &mut self.free[i];
        let new_space = Space { x: fs.x, width: needed_w };
        // Consume the used space from the available space.
        fs.x += needed_w;
        fs.width -= needed_w;
        if fs.width == 0 {
            self.free.remove(i);
        }
        self.available -= needed_w;
        self.used.push(new_space);
        Some(new_space)
    }

    /// TB: TBSpaceAllocator::GetSmallestAvailableSpace
    fn smallest_available_space(&self, needed_w: i32) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, fs) in self.free.iter().enumerate() {
            if needed_w == fs.width {
                return Some(i); // It can't be better than a perfect match.
            }
            if needed_w < fs.width && best.is_none_or(|b| fs.width < self.free[b].width) {
                best = Some(i);
            }
        }
        best
    }

    /// Free a space returned by `alloc_space`. Returns false (and does nothing) if it
    /// is not a live allocation. TB: TBSpaceAllocator::FreeSpace
    pub fn free_space(&mut self, space: Space) -> bool {
        let Some(ui) = self.used.iter().position(|s| *s == space) else {
            return false;
        };
        self.used.swap_remove(ui);
        self.available += space.width;

        // Insert sorted by x, then merge adjacent spaces (TB extends the preceding or
        // succeeding space, then runs a merge pass; the result is the same).
        let at = self.free.iter().position(|fs| fs.x > space.x).unwrap_or(self.free.len());
        self.free.insert(at, space);
        let mut i = 0;
        while i + 1 < self.free.len() {
            if self.free[i].x + self.free[i].width == self.free[i + 1].x {
                self.free[i].width += self.free[i + 1].width;
                self.free.remove(i + 1);
            } else {
                i += 1;
            }
        }
        debug_assert!(self.free.windows(2).all(|w| w[0].x + w[0].width <= w[1].x));
        true
    }
}

/// One row of a page. TB: TBFragmentSpaceAllocator
#[derive(Clone, Debug)]
struct Row {
    y: i32,
    height: i32,
    space: SpaceAllocator,
}

/// One bitmap's worth of packed fragments. TB: TBBitmapFragmentMap
struct Page {
    size: Size,
    /// Premultiplied RGBA8, row-major.
    pixels: Vec<u8>,
    /// Rows sorted top to bottom, covering the whole page.
    rows: Vec<Row>,
    bitmap: Option<BitmapId>,
    dirty: bool,
    allocated_pixels: i64,
    /// Created for one oversized image; never shared, freed when emptied.
    dedicated: bool,
}

impl fmt::Debug for Page {
    // Everything but the pixels.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Page")
            .field("size", &self.size)
            .field("rows", &self.rows)
            .field("bitmap", &self.bitmap)
            .field("dirty", &self.dirty)
            .field("allocated_pixels", &self.allocated_pixels)
            .field("dedicated", &self.dedicated)
            .finish_non_exhaustive()
    }
}

impl Page {
    fn new(size: Size, dedicated: bool) -> Page {
        Page {
            size,
            pixels: vec![0; size.w as usize * size.h as usize * 4],
            rows: Vec::new(),
            bitmap: None,
            dirty: true,
            allocated_pixels: 0,
            dedicated,
        }
    }

    /// Find room for a `frag` sized image, copy it in and return its rect.
    /// TB: TBBitmapFragmentMap::CreateNewFragment
    fn add(&mut self, frag: Size, rgba: &[u8], gutter: i32) -> Option<Rect> {
        // Finding available space works like this: the page is sliced up horizontally
        // in rows (initially one row covering the whole page). A new fragment goes in
        // the row with the smallest height that fits it. If that row is empty, it is
        // sliced to make a row only as high as needed.
        //
        // Stretched drawing with filtering reads texels just outside the source
        // rect, which would come from neighbouring fragments, so a gutter is kept
        // around each fragment, except when one fragment uses the whole page.
        let border = if frag == self.size { 0 } else { gutter };
        let needed_w = frag.w + border * 2;
        let needed_h = frag.h + border * 2;
        if needed_w > self.size.w || needed_h > self.size.h {
            return None;
        }

        if self.rows.is_empty() {
            self.rows.push(Row { y: 0, height: self.size.h, space: SpaceAllocator::new(self.size.w) });
        }
        // Get the smallest row where we fit.
        let mut best: Option<usize> = None;
        for (i, row) in self.rows.iter().enumerate() {
            if best.is_none_or(|b| row.height < self.rows[b].height)
                && needed_h <= row.height
                && row.space.has_space(needed_w)
            {
                best = Some(i);
                if needed_h == row.height {
                    break; // We can't find a smaller row.
                }
            }
        }
        let bi = best?;
        // If the row is unused, split it so it only consumes the needed height.
        if self.rows[bi].space.is_all_available() && needed_h < self.rows[bi].height {
            let row = &mut self.rows[bi];
            let rest =
                Row { y: row.y + needed_h, height: row.height - needed_h, space: SpaceAllocator::new(self.size.w) };
            row.height = needed_h;
            self.rows.insert(bi + 1, rest);
        }
        let row = &mut self.rows[bi];
        let space = row.space.alloc_space(needed_w)?;
        let row_y = row.y;
        let row_h = row.height;
        let rect = Rect::new(space.x + border, row_y + border, frag.w, frag.h);
        self.copy_in(rect, rgba, border);
        self.dirty = true;
        self.allocated_pixels += space.width as i64 * row_h as i64;
        Some(rect)
    }

    /// TB: TBBitmapFragmentMap::CopyData
    fn copy_in(&mut self, rect: Rect, rgba: &[u8], border: i32) {
        let stride = self.size.w as usize * 4;
        let row_bytes = rect.w as usize * 4;
        for j in 0..rect.h as usize {
            let d = (rect.y as usize + j) * stride + rect.x as usize * 4;
            self.pixels[d..d + row_bytes].copy_from_slice(&rgba[j * row_bytes..(j + 1) * row_bytes]);
        }
        // TB fills the border with the clamped edge colour at alpha 0 (straight alpha).
        // In premultiplied alpha that is transparent black, so the gutter is cleared:
        // filtering then fades the edge out exactly as in TB, without colour fringes.
        if border > 0 {
            let outer = rect.expand_xy(border, border);
            for y in outer.y..outer.bottom() {
                let row_start = y as usize * stride;
                if y < rect.y || y >= rect.bottom() {
                    let d = row_start + outer.x as usize * 4;
                    self.pixels[d..d + outer.w as usize * 4].fill(0);
                } else {
                    let l = row_start + outer.x as usize * 4;
                    self.pixels[l..l + border as usize * 4].fill(0);
                    let r = row_start + rect.right() as usize * 4;
                    self.pixels[r..r + border as usize * 4].fill(0);
                }
            }
        }
    }

    /// Free the fragment at `rect`. TB: TBBitmapFragmentMap::FreeFragmentSpace
    fn remove(&mut self, rect: Rect, gutter: i32) -> bool {
        let border = if rect.size() == self.size { 0 } else { gutter };
        let (x, y, w) = (rect.x - border, rect.y - border, rect.w + border * 2);
        let Some(ri) = self.rows.iter().position(|r| r.y == y && rect.h + border * 2 <= r.height) else {
            return false;
        };
        let row = &mut self.rows[ri];
        if !row.space.free_space(Space { x, width: w }) {
            return false;
        }
        self.allocated_pixels -= w as i64 * row.height as i64;

        // If the row is now empty, merge empty rows so larger fragments have a chance
        // of allocating the space.
        if row.space.is_all_available() {
            let mut i = 0;
            while i + 1 < self.rows.len() {
                if self.rows[i].space.is_all_available() && self.rows[i + 1].space.is_all_available() {
                    self.rows[i].height += self.rows[i + 1].height;
                    self.rows.remove(i + 1);
                } else {
                    i += 1;
                }
            }
        }
        true
    }

    fn is_empty(&self) -> bool {
        self.allocated_pixels == 0
    }

    /// Create or update the bitmap if needed. TB: TBBitmapFragmentMap::ValidateBitmap
    fn upload(&mut self, r: &mut dyn Renderer) -> Option<BitmapId> {
        match self.bitmap {
            None => {
                self.bitmap = r.create_bitmap(self.size, &self.pixels);
                if self.bitmap.is_some() {
                    self.dirty = false;
                }
            }
            Some(b) if self.dirty => {
                r.update_bitmap(b, &self.pixels);
                self.dirty = false;
            }
            Some(_) => {}
        }
        self.bitmap
    }
}

/// Packs images into pages. TB: TBBitmapFragmentManager
///
/// Pages are created on demand at the default page size; an image that does not fit
/// a default page gets a dedicated page sized exactly to it. Pages keep their index
/// until `clear` (so `FragmentId`s stay valid); an emptied default page is kept for
/// reuse, an emptied dedicated page is released and its index reused later.
///
/// Bitmaps are only created, updated and destroyed in the calls that take a
/// renderer. Bitmaps released by `remove` or `clear` are destroyed on the next such
/// call. Not `Clone`: two atlases must not own the same `BitmapId`s.
#[derive(Debug)]
pub struct Atlas {
    pages: Vec<Option<Page>>,
    page_size: Size,
    gutter: i32,
    /// Bitmaps of released pages, destroyed on the next call that has a renderer.
    pending_destroy: Vec<BitmapId>,
}

impl Atlas {
    /// An atlas with a 1 pixel transparent gutter around each fragment (TB's
    /// `SetAddBorder(true)`, as the skin uses), so stretched drawing with linear
    /// filtering does not bleed between fragments.
    pub fn new(page_size: Size) -> Atlas {
        Atlas::with_gutter(page_size, true)
    }

    /// Like [`Atlas::new`], choosing whether fragments get a gutter (the glyph cache,
    /// which only draws 1:1, does not need one). TB: SetAddBorder
    pub fn with_gutter(page_size: Size, gutter: bool) -> Atlas {
        Atlas {
            pages: Vec::new(),
            page_size: page_size.max(Size::new(1, 1)),
            gutter: gutter as i32,
            pending_destroy: Vec::new(),
        }
    }

    /// The size of new (non-dedicated) pages.
    pub fn default_page_size(&self) -> Size {
        self.page_size
    }

    /// Number of page slots (live pages have indices below this).
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Copy premultiplied RGBA8 (`size.w * size.h * 4` bytes, row-major) into a free
    /// spot, creating a new page if needed. An image larger than the page size gets a
    /// page of its own sized to fit. `None` if the size is empty, `rgba` is too short
    /// or the page index space (`u16`) is exhausted.
    /// TB: TBBitmapFragmentManager::CreateNewFragment
    pub fn add(&mut self, size: Size, rgba: &[u8]) -> Option<FragmentId> {
        if size.w <= 0 || size.h <= 0 || rgba.len() < size.w as usize * size.h as usize * 4 {
            return None;
        }
        let ps = self.page_size;
        let g = self.gutter * 2;
        let fits_default = size == ps || (size.w.saturating_add(g) <= ps.w && size.h.saturating_add(g) <= ps.h);
        if fits_default {
            // Try existing pages in creation order, to maximize fragments per page.
            for (i, page) in self.pages.iter_mut().enumerate() {
                let Some(p) = page.as_mut().filter(|p| !p.dedicated) else { continue };
                if let Some(rect) = p.add(size, rgba, self.gutter) {
                    return Some(FragmentId { page: i as u16, rect });
                }
            }
        }
        // Create a new page where we know it will fit.
        let (page_size, dedicated) = if fits_default { (ps, false) } else { (size, true) };
        let slot = match self.pages.iter().position(Option::is_none) {
            Some(i) => i,
            None if self.pages.len() <= u16::MAX as usize => {
                self.pages.push(None);
                self.pages.len() - 1
            }
            None => return None,
        };
        let mut page = Page::new(page_size, dedicated);
        let rect = page.add(size, rgba, self.gutter)?;
        self.pages[slot] = Some(page);
        Some(FragmentId { page: slot as u16, rect })
    }

    /// Free the fragment's space for reuse. The page pixels may stay (and are not
    /// re-uploaded). Removing an unknown or already removed fragment does nothing.
    /// TB: TBBitmapFragmentManager::FreeFragment
    pub fn remove(&mut self, f: FragmentId) {
        let Some(page) = self.pages.get_mut(f.page as usize).and_then(Option::as_mut) else {
            return;
        };
        if page.remove(f.rect, self.gutter) && page.dedicated && page.is_empty() {
            if let Some(b) = page.bitmap {
                self.pending_destroy.push(b);
            }
            self.pages[f.page as usize] = None;
        }
    }

    /// Drop every fragment and page. Their bitmaps are destroyed on the next call
    /// that takes a renderer. TB: TBBitmapFragmentManager::Clear
    pub fn clear(&mut self) {
        for page in self.pages.drain(..).flatten() {
            if let Some(b) = page.bitmap {
                self.pending_destroy.push(b);
            }
        }
    }

    /// Size of a page (`Size::ZERO` if there is no such page).
    pub fn page_size(&self, page: u16) -> Size {
        self.page(page).map_or(Size::ZERO, |p| p.size)
    }

    /// Premultiplied RGBA8 pixels of a page (the CPU copy).
    pub fn page_pixels(&self, page: u16) -> Option<&[u8]> {
        self.page(page).map(|p| p.pixels.as_slice())
    }

    /// The bitmap for a page, creating it (the first time) or updating it (when
    /// dirty) through the renderer. `None` if there is no such page or the renderer
    /// could not create the bitmap. TB: TBBitmapFragmentMap::GetBitmap
    pub fn page_bitmap(&mut self, page: u16, r: &mut dyn Renderer) -> Option<BitmapId> {
        self.destroy_pending(r);
        self.pages.get_mut(page as usize)?.as_mut()?.upload(r)
    }

    /// Create or update every page that needs it.
    /// TB: TBBitmapFragmentManager::ValidateBitmaps
    pub fn upload_all(&mut self, r: &mut dyn Renderer) {
        self.destroy_pending(r);
        for page in self.pages.iter_mut().flatten() {
            page.upload(r);
        }
    }

    /// The page's current bitmap, without uploading (it may be missing or stale if
    /// the page is dirty).
    pub fn bitmap(&self, page: u16) -> Option<BitmapId> {
        self.page(page)?.bitmap
    }

    /// True if the page has changes that are not uploaded yet.
    pub fn is_dirty(&self, page: u16) -> bool {
        self.page(page).is_some_and(|p| p.dirty || p.bitmap.is_none())
    }

    /// The renderer lost its bitmaps: forget every `BitmapId` and mark all pages
    /// dirty, so the next upload re-creates them (H-22).
    pub fn renderer_lost(&mut self) {
        self.pending_destroy.clear();
        for page in self.pages.iter_mut().flatten() {
            page.bitmap = None;
            page.dirty = true;
        }
    }

    /// Destroy every page bitmap. Pages keep their pixels and are re-created by the
    /// next upload. TB: TBBitmapFragmentManager::DeleteBitmaps
    pub fn destroy_bitmaps(&mut self, r: &mut dyn Renderer) {
        self.destroy_pending(r);
        for page in self.pages.iter_mut().flatten() {
            if let Some(b) = page.bitmap.take() {
                r.destroy_bitmap(b);
            }
            page.dirty = true;
        }
    }

    fn page(&self, page: u16) -> Option<&Page> {
        self.pages.get(page as usize)?.as_ref()
    }

    fn destroy_pending(&mut self, r: &mut dyn Renderer) {
        for b in self.pending_destroy.drain(..) {
            r.destroy_bitmap(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    // TB: tests/test_tb_space_allocator.cpp free_when_full
    #[test]
    fn space_allocator_free_when_full() {
        let mut spa = SpaceAllocator::new(30);
        assert!(spa.has_space(30));
        let s1 = spa.alloc_space(10).unwrap();
        let s2 = spa.alloc_space(10).unwrap();
        let s3 = spa.alloc_space(10).unwrap();
        assert!(!spa.has_space(1));

        // Free middle
        assert!(spa.free_space(s2));
        assert!(spa.has_space(10));
        // Allocate middle (again)
        let _s2 = spa.alloc_space(10).unwrap();
        assert!(!spa.has_space(10));

        // Free first
        spa.free_space(s1);
        assert!(spa.has_space(10));
        // Allocate first (again)
        let _s1 = spa.alloc_space(10).unwrap();
        assert!(!spa.has_space(10));

        // Free last
        spa.free_space(s3);
        assert!(spa.has_space(10));
        // Allocate last (again)
        let _s3 = spa.alloc_space(10).unwrap();
        assert!(!spa.has_space(10));
    }

    // TB: tests/test_tb_space_allocator.cpp free_all
    #[test]
    fn space_allocator_free_all() {
        let mut spa = SpaceAllocator::new(30);
        let s = spa.alloc_space(30).unwrap();
        spa.free_space(s);
        assert!(spa.has_space(30));

        let orders: [[usize; 3]; 4] = [
            [0, 1, 2], // Free in order
            [2, 1, 0], // Free in backwards order
            [1, 0, 2], // Free middle first (in order)
            [1, 2, 0], // Free middle first (in backwards order)
        ];
        for order in orders {
            let s = [spa.alloc_space(10).unwrap(), spa.alloc_space(10).unwrap(), spa.alloc_space(10).unwrap()];
            for i in order {
                assert!(spa.free_space(s[i]));
            }
            assert!(spa.has_space(30));
            assert!(spa.is_all_available());
        }
    }

    // TB: tests/test_tb_space_allocator.cpp free_scattered
    #[test]
    fn space_allocator_free_scattered() {
        let mut spa = SpaceAllocator::new(50);
        let s1 = spa.alloc_space(10).unwrap();
        let s2 = spa.alloc_space(10).unwrap();
        let s3 = spa.alloc_space(10).unwrap();
        let s4 = spa.alloc_space(10).unwrap();
        let s5 = spa.alloc_space(10).unwrap();

        // Free all middle space
        spa.free_space(s2);
        spa.free_space(s4);
        spa.free_space(s3);
        assert!(spa.has_space(30));

        let _s2 = spa.alloc_space(10).unwrap();
        let s3 = spa.alloc_space(10).unwrap();
        let _s4 = spa.alloc_space(10).unwrap();

        // Free edges, then middle
        spa.free_space(s1);
        spa.free_space(s5);
        spa.free_space(s3);
        assert!(!spa.has_space(30)); // We should not have 30 of continuous space.
        assert!(spa.alloc_space(10).is_some()); // We should have 3 * 10 spaces though.
        assert!(spa.alloc_space(10).is_some());
        assert!(spa.alloc_space(10).is_some());
    }

    #[test]
    fn space_allocator_picks_smallest_fit_and_rejects_bad_frees() {
        let mut spa = SpaceAllocator::new(100);
        let a = spa.alloc_space(10).unwrap();
        let _b = spa.alloc_space(10).unwrap();
        let c = spa.alloc_space(5).unwrap();
        let _d = spa.alloc_space(10).unwrap();
        spa.free_space(a); // free: [0,10) and [35,100)
        spa.free_space(c); // free: [0,10), [20,25), [35,100)
        assert_eq!(spa.alloc_space(4), Some(Space { x: 20, width: 4 }));
        assert_eq!(spa.alloc_space(10), Some(Space { x: 0, width: 10 })); // perfect match
        assert!(!spa.free_space(Space { x: 0, width: 3 }));
        assert!(!spa.free_space(c));
    }

    fn solid(size: Size, v: u8) -> Vec<u8> {
        vec![v; size.w as usize * size.h as usize * 4]
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Create(Size),
        Update(BitmapId),
        Destroy(BitmapId),
    }

    #[derive(Default)]
    struct Rec {
        calls: Vec<Call>,
        next: u32,
    }

    impl Renderer for Rec {
        fn create_bitmap(&mut self, size: Size, rgba: &[u8]) -> Option<BitmapId> {
            assert_eq!(rgba.len(), size.w as usize * size.h as usize * 4);
            self.calls.push(Call::Create(size));
            self.next += 1;
            Some(BitmapId(self.next))
        }
        fn update_bitmap(&mut self, bitmap: BitmapId, _rgba: &[u8]) {
            self.calls.push(Call::Update(bitmap));
        }
        fn destroy_bitmap(&mut self, bitmap: BitmapId) {
            self.calls.push(Call::Destroy(bitmap));
        }
        fn draw(&mut self, _: Option<BitmapId>, _: Rect, _: &[crate::render::Quad]) {}
    }

    #[test]
    fn add_copies_pixels_with_gutter() {
        let mut atlas = Atlas::new(Size::new(16, 16));
        let rgba: Vec<u8> = (0..2 * 2 * 4).map(|i| i as u8 + 1).collect();
        let f = atlas.add(Size::new(2, 2), &rgba).unwrap();
        assert_eq!(f, FragmentId { page: 0, rect: Rect::new(1, 1, 2, 2) });
        let px = atlas.page_pixels(0).unwrap();
        let at = |x: usize, y: usize| &px[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4];
        assert_eq!(at(1, 1), &rgba[0..4]);
        assert_eq!(at(2, 1), &rgba[4..8]);
        assert_eq!(at(1, 2), &rgba[8..12]);
        assert_eq!(at(2, 2), &rgba[12..16]);
        assert_eq!(at(0, 1), &[0, 0, 0, 0]);
        assert_eq!(at(3, 3), &[0, 0, 0, 0]);

        // Second fragment goes next to it in the same row (after the 4 px wide space).
        let g = atlas.add(Size::new(2, 2), &rgba).unwrap();
        assert_eq!(g.rect, Rect::new(5, 1, 2, 2));
        // A taller one does not fit the 4 px row and gets the next row.
        let h = atlas.add(Size::new(2, 4), &solid(Size::new(2, 4), 9)).unwrap();
        assert_eq!(h.rect, Rect::new(1, 5, 2, 4));
    }

    #[test]
    fn add_without_gutter() {
        let mut atlas = Atlas::with_gutter(Size::new(8, 8), false);
        let a = atlas.add(Size::new(4, 4), &solid(Size::new(4, 4), 1)).unwrap();
        let b = atlas.add(Size::new(4, 4), &solid(Size::new(4, 4), 2)).unwrap();
        assert_eq!(a.rect, Rect::new(0, 0, 4, 4));
        assert_eq!(b.rect, Rect::new(4, 0, 4, 4));
    }

    #[test]
    fn add_rejects_bad_input() {
        let mut atlas = Atlas::new(Size::new(8, 8));
        assert_eq!(atlas.add(Size::new(0, 4), &[]), None);
        assert_eq!(atlas.add(Size::new(2, 2), &[0; 15]), None);
        assert_eq!(atlas.page_count(), 0);
    }

    #[test]
    fn remove_and_reuse() {
        let mut atlas = Atlas::new(Size::new(16, 16));
        let s = Size::new(6, 6); // 8x8 with gutter: four fit a page
        let frags: Vec<FragmentId> = (0..4).map(|i| atlas.add(s, &solid(s, i)).unwrap()).collect();
        assert!(frags.iter().all(|f| f.page == 0));
        assert_eq!(atlas.add(s, &solid(s, 7)).unwrap().page, 1); // full: new page

        atlas.remove(frags[1]);
        let again = atlas.add(s, &solid(s, 8)).unwrap();
        assert_eq!(again, frags[1]); // reuses the freed spot on page 0

        // Removing everything lets a page-sized image use the whole (kept) page 0.
        for f in [frags[0], again, frags[2], frags[3]] {
            atlas.remove(f);
        }
        atlas.remove(frags[0]); // double remove is a no-op
        let big = atlas.add(Size::new(16, 16), &solid(Size::new(16, 16), 3)).unwrap();
        assert_eq!(big, FragmentId { page: 0, rect: Rect::new(0, 0, 16, 16) });
        assert_eq!(atlas.page_count(), 2);
    }

    #[test]
    fn rows_split_and_merge() {
        let mut atlas = Atlas::with_gutter(Size::new(10, 10), false);
        let a = atlas.add(Size::new(10, 3), &solid(Size::new(10, 3), 1)).unwrap();
        let b = atlas.add(Size::new(10, 3), &solid(Size::new(10, 3), 1)).unwrap();
        assert_eq!((a.rect.y, b.rect.y), (0, 3));
        // 4 rows left (6 px); an 8 px tall image needs a new page.
        assert_eq!(atlas.add(Size::new(2, 8), &solid(Size::new(2, 8), 1)).unwrap().page, 1);
        atlas.remove(b);
        // Freed row merges with the empty remainder: 7 px.
        let c = atlas.add(Size::new(2, 7), &solid(Size::new(2, 7), 1)).unwrap();
        assert_eq!(c, FragmentId { page: 0, rect: Rect::new(0, 3, 2, 7) });
    }

    #[test]
    fn oversize_image_gets_dedicated_page() {
        let mut atlas = Atlas::new(Size::new(16, 16));
        let small = atlas.add(Size::new(4, 4), &solid(Size::new(4, 4), 1)).unwrap();
        let big_size = Size::new(40, 10);
        let big = atlas.add(big_size, &solid(big_size, 2)).unwrap();
        assert_eq!(big, FragmentId { page: 1, rect: Rect::new(0, 0, 40, 10) });
        assert_eq!(atlas.page_size(1), big_size);
        // 15x15 needs 17x17 with gutter: also dedicated, without gutter.
        let almost = atlas.add(Size::new(15, 15), &solid(Size::new(15, 15), 3)).unwrap();
        assert_eq!(almost, FragmentId { page: 2, rect: Rect::new(0, 0, 15, 15) });
        // Small images never go into dedicated pages.
        assert_eq!(atlas.add(Size::new(4, 4), &solid(Size::new(4, 4), 1)).unwrap().page, 0);
        assert_eq!(small.page, 0);

        // Removing the oversize fragment releases its page; its bitmap is destroyed
        // on the next call with a renderer, and the slot is reused.
        let mut r = Rec::default();
        atlas.upload_all(&mut r);
        let big_bitmap = atlas.bitmap(1).unwrap();
        atlas.remove(big);
        assert_eq!(atlas.page_size(1), Size::ZERO);
        assert_eq!(atlas.bitmap(1), None);
        r.calls.clear();
        atlas.upload_all(&mut r);
        assert_eq!(r.calls, [Call::Destroy(big_bitmap)]);
        let big2 = atlas.add(big_size, &solid(big_size, 2)).unwrap();
        assert_eq!(big2.page, 1);
    }

    #[test]
    fn page_bitmap_creates_once_then_updates_when_dirty() {
        let mut atlas = Atlas::new(Size::new(16, 16));
        let mut r = Rec::default();
        assert_eq!(atlas.page_bitmap(0, &mut r), None); // no page yet
        let s = Size::new(2, 2);
        atlas.add(s, &solid(s, 1)).unwrap();
        assert!(atlas.is_dirty(0));
        assert_eq!(atlas.bitmap(0), None);

        let b = atlas.page_bitmap(0, &mut r).unwrap();
        assert_eq!(r.calls, [Call::Create(Size::new(16, 16))]);
        assert_eq!(atlas.bitmap(0), Some(b));
        assert!(!atlas.is_dirty(0));

        // Clean: no calls.
        assert_eq!(atlas.page_bitmap(0, &mut r), Some(b));
        atlas.upload_all(&mut r);
        assert_eq!(r.calls.len(), 1);

        // Dirty: one update, then clean again.
        let f = atlas.add(s, &solid(s, 2)).unwrap();
        assert_eq!(atlas.page_bitmap(0, &mut r), Some(b));
        assert_eq!(atlas.page_bitmap(0, &mut r), Some(b));
        assert_eq!(r.calls, [Call::Create(Size::new(16, 16)), Call::Update(b)]);

        // Removing does not dirty the page.
        atlas.remove(f);
        assert!(!atlas.is_dirty(0));
    }

    #[test]
    fn renderer_lost_recreates() {
        let mut atlas = Atlas::new(Size::new(16, 16));
        let mut r = Rec::default();
        let s = Size::new(2, 2);
        atlas.add(s, &solid(s, 1)).unwrap();
        atlas.add(Size::new(20, 20), &solid(Size::new(20, 20), 1)).unwrap();
        atlas.upload_all(&mut r);
        assert_eq!(r.calls, [Call::Create(Size::new(16, 16)), Call::Create(Size::new(20, 20))]);

        atlas.renderer_lost();
        assert_eq!(atlas.bitmap(0), None);
        assert!(atlas.is_dirty(0) && atlas.is_dirty(1));
        let mut r2 = Rec { calls: Vec::new(), next: 100 };
        assert_eq!(atlas.page_bitmap(1, &mut r2), Some(BitmapId(101)));
        atlas.upload_all(&mut r2);
        assert_eq!(r2.calls, [Call::Create(Size::new(20, 20)), Call::Create(Size::new(16, 16))]);
    }

    #[test]
    fn destroy_bitmaps_and_clear() {
        let mut atlas = Atlas::new(Size::new(16, 16));
        let mut r = Rec::default();
        let s = Size::new(2, 2);
        atlas.add(s, &solid(s, 1)).unwrap();
        atlas.upload_all(&mut r);
        let b = atlas.bitmap(0).unwrap();

        atlas.destroy_bitmaps(&mut r);
        assert_eq!(r.calls.last(), Some(&Call::Destroy(b)));
        assert_eq!(atlas.bitmap(0), None);
        let b2 = atlas.page_bitmap(0, &mut r).unwrap();
        assert_eq!(r.calls.last(), Some(&Call::Create(Size::new(16, 16))));

        atlas.clear();
        assert_eq!(atlas.page_count(), 0);
        assert_eq!(atlas.bitmap(0), None);
        atlas.destroy_bitmaps(&mut r);
        assert_eq!(r.calls.last(), Some(&Call::Destroy(b2)));
        let n = r.calls.len();
        atlas.destroy_bitmaps(&mut r);
        assert_eq!(r.calls.len(), n);
    }
}
