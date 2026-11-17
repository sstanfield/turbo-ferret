//! Software renderer for Turbo Ferret: draws quads into an RGBA8 premultiplied
//! framebuffer with nearest-neighbour sampling. Used for golden-image tests and for
//! hosts without a GPU.
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use turbo_ferret::geom::{Color, Rect, Size};
use turbo_ferret::render::{BitmapId, Quad, Renderer};

/// Counters for tests. Reset with [`SoftRenderer::reset_stats`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub frames: u32,
    pub draw_calls: u32,
    pub quads: u32,
    pub bitmaps_created: u32,
    pub bitmaps_updated: u32,
    pub bitmaps_destroyed: u32,
}

struct Bitmap {
    size: Size,
    pixels: Vec<u8>,
}

/// A [`Renderer`] that draws into memory.
///
/// The framebuffer is RGBA8, premultiplied alpha, row-major. Its size is set by
/// [`new`](SoftRenderer::new) and [`resize`](SoftRenderer::resize) (the viewport
/// passed to `begin_frame` does not change it); drawing is clipped to it.
pub struct SoftRenderer {
    size: Size,
    pixels: Vec<u8>,
    /// Slot table: `BitmapId(i)` is `bitmaps[i]`; destroyed slots are reused.
    bitmaps: Vec<Option<Bitmap>>,
    stats: Stats,
    /// Scratch: source texel column for each destination column of a quad.
    cols: Vec<usize>,
}

impl SoftRenderer {
    /// A framebuffer of `size`, cleared to transparent black.
    pub fn new(size: Size) -> SoftRenderer {
        let size = size.max(Size::ZERO);
        SoftRenderer {
            size,
            pixels: vec![0; buf_len(size)],
            bitmaps: Vec::new(),
            stats: Stats::default(),
            cols: Vec::new(),
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Change the framebuffer size. The contents are cleared to transparent black
    /// (unless the size is unchanged). Bitmaps are kept.
    pub fn resize(&mut self, size: Size) {
        let size = size.max(Size::ZERO);
        if size != self.size {
            self.size = size;
            self.pixels = vec![0; buf_len(size)];
        }
    }

    /// Fill the framebuffer with a straight-alpha color (stored premultiplied).
    pub fn clear(&mut self, color: Color) {
        let c = color.premultiplied();
        for px in self.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }

    /// The framebuffer: RGBA8, premultiplied, row-major, `w * h * 4` bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// One framebuffer pixel (premultiplied RGBA). Panics if out of bounds.
    pub fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        assert!(
            x >= 0 && y >= 0 && x < self.size.w && y < self.size.h,
            "pixel ({x}, {y}) outside {}x{}",
            self.size.w,
            self.size.h
        );
        let i = (y as usize * self.size.w as usize + x as usize) * 4;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3]]
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn reset_stats(&mut self) {
        self.stats = Stats::default();
    }

    /// Number of live bitmaps.
    pub fn bitmap_count(&self) -> usize {
        self.bitmaps.iter().flatten().count()
    }

    pub fn bitmap_size(&self, bitmap: BitmapId) -> Option<Size> {
        self.bitmap(bitmap).map(|b| b.size)
    }

    /// A bitmap's pixels (premultiplied RGBA8).
    pub fn bitmap_pixels(&self, bitmap: BitmapId) -> Option<&[u8]> {
        self.bitmap(bitmap).map(|b| b.pixels.as_slice())
    }

    fn bitmap(&self, bitmap: BitmapId) -> Option<&Bitmap> {
        self.bitmaps.get(bitmap.0 as usize)?.as_ref()
    }
}

impl Renderer for SoftRenderer {
    fn begin_frame(&mut self, _viewport: Size) {
        self.stats.frames += 1;
    }

    fn create_bitmap(&mut self, size: Size, rgba: &[u8]) -> Option<BitmapId> {
        let n = buf_len(size);
        if size.w <= 0 || size.h <= 0 || rgba.len() < n {
            return None;
        }
        let bitmap = Bitmap { size, pixels: rgba[..n].to_vec() };
        let slot = match self.bitmaps.iter().position(Option::is_none) {
            Some(i) => i,
            None => {
                self.bitmaps.push(None);
                self.bitmaps.len() - 1
            }
        };
        let id = u32::try_from(slot).ok()?;
        self.bitmaps[slot] = Some(bitmap);
        self.stats.bitmaps_created += 1;
        Some(BitmapId(id))
    }

    fn update_bitmap(&mut self, bitmap: BitmapId, rgba: &[u8]) {
        if let Some(Some(b)) = self.bitmaps.get_mut(bitmap.0 as usize) {
            let n = b.pixels.len();
            if rgba.len() >= n {
                b.pixels.copy_from_slice(&rgba[..n]);
                self.stats.bitmaps_updated += 1;
            }
        }
    }

    fn destroy_bitmap(&mut self, bitmap: BitmapId) {
        if let Some(slot @ Some(_)) = self.bitmaps.get_mut(bitmap.0 as usize) {
            *slot = None;
            self.stats.bitmaps_destroyed += 1;
        }
    }

    fn draw(&mut self, bitmap: Option<BitmapId>, clip: Rect, quads: &[Quad]) {
        self.stats.draw_calls += 1;
        self.stats.quads += quads.len() as u32;
        let SoftRenderer { size, pixels, bitmaps, cols, .. } = self;
        let clip = clip.clip(&Rect::from_size(*size));
        if clip.is_empty() {
            return;
        }
        let stride = size.w as usize * 4;
        let texture = match bitmap {
            None => None,
            Some(id) => match bitmaps.get(id.0 as usize) {
                Some(Some(b)) => Some(b),
                _ => return, // unknown bitmap: draw nothing
            },
        };
        for q in quads {
            let area = q.dst.clip(&clip);
            if area.is_empty() {
                continue;
            }
            let color = [q.color.r as u32, q.color.g as u32, q.color.b as u32, q.color.a as u32];
            let x0 = area.x as usize * 4;
            let x1 = area.right() as usize * 4;
            match texture {
                None => {
                    if color == [0; 4] {
                        continue;
                    }
                    for y in area.y..area.bottom() {
                        let row = &mut pixels[y as usize * stride..][x0..x1];
                        for d in row.chunks_exact_mut(4) {
                            blend(d, color);
                        }
                    }
                }
                Some(tex) => {
                    let (tw, th) = (tex.size.w, tex.size.h);
                    cols.clear();
                    cols.extend((area.x..area.right()).map(|x| texel(x - q.dst.x, q.src.x, q.src.w, q.dst.w, tw)));
                    let white = color == [255; 4];
                    for y in area.y..area.bottom() {
                        let ty = texel(y - q.dst.y, q.src.y, q.src.h, q.dst.h, th);
                        let src_row = &tex.pixels[ty * tw as usize * 4..][..tw as usize * 4];
                        let row = &mut pixels[y as usize * stride..][x0..x1];
                        for (d, &tx) in row.chunks_exact_mut(4).zip(cols.iter()) {
                            let t = &src_row[tx * 4..tx * 4 + 4];
                            let s = if white {
                                [t[0] as u32, t[1] as u32, t[2] as u32, t[3] as u32]
                            } else {
                                [
                                    mul255(t[0] as u32, color[0]),
                                    mul255(t[1] as u32, color[1]),
                                    mul255(t[2] as u32, color[2]),
                                    mul255(t[3] as u32, color[3]),
                                ]
                            };
                            blend(d, s);
                        }
                    }
                }
            }
        }
    }
}

fn buf_len(size: Size) -> usize {
    size.w.max(0) as usize * size.h.max(0) as usize * 4
}

/// Texel index for destination pixel `i` (relative to the unclipped quad), as
/// documented on [`Quad::src`]: `floor(src_x + (i + 0.5) * src_w / dst_w)`, clamped
/// to the bitmap (like clamp-to-edge). A negative `src_w` mirrors.
fn texel(i: i32, src_x: i32, src_w: i32, dst_w: i32, size: i32) -> usize {
    let num = 2 * src_x as i64 * dst_w as i64 + (2 * i as i64 + 1) * src_w as i64;
    let t = num.div_euclid(2 * dst_w as i64);
    t.clamp(0, size as i64 - 1) as usize
}

/// `round(a * b / 255)` for `a, b` in `0..=255`.
fn mul255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}

/// Premultiplied source over: `d = s + d * (1 - s.a)`.
fn blend(d: &mut [u8], s: [u32; 4]) {
    let inv = 255 - s[3].min(255);
    if inv == 0 {
        for c in 0..4 {
            d[c] = s[c].min(255) as u8;
        }
    } else if s != [0; 4] {
        for c in 0..4 {
            d[c] = (s[c] + mul255(d[c] as u32, inv)).min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul255_is_exact_rounding() {
        for a in 0..=255u32 {
            for b in 0..=255u32 {
                let exact = (a * b * 2 + 255) / 510; // round half up
                assert_eq!(mul255(a, b), exact, "{a} * {b}");
            }
        }
    }

    #[test]
    fn texel_mapping() {
        // 1:1
        assert_eq!((0..4).map(|i| texel(i, 0, 4, 4, 4)).collect::<Vec<_>>(), [0, 1, 2, 3]);
        // Mirrored: x is the right edge.
        assert_eq!((0..4).map(|i| texel(i, 4, -4, 4, 4)).collect::<Vec<_>>(), [3, 2, 1, 0]);
        // 2x up, and down
        assert_eq!((0..8).map(|i| texel(i, 0, 4, 8, 4)).collect::<Vec<_>>(), [0, 0, 1, 1, 2, 2, 3, 3]);
        assert_eq!((0..2).map(|i| texel(i, 0, 4, 2, 4)).collect::<Vec<_>>(), [1, 3]);
        // Offset source and clamping
        assert_eq!((0..3).map(|i| texel(i, 2, 3, 3, 4)).collect::<Vec<_>>(), [2, 3, 3]);
        assert_eq!(texel(0, -5, 1, 1, 4), 0);
    }

    #[test]
    fn solid_fill_and_blend() {
        let mut r = SoftRenderer::new(Size::new(4, 4));
        r.clear(Color::BLACK);
        let half_white = Color::rgba(255, 255, 255, 128).premultiplied();
        r.draw(None, Rect::new(0, 0, 4, 4), &[Quad { dst: Rect::new(1, 1, 2, 2), src: Rect::ZERO, color: half_white }]);
        assert_eq!(r.pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(r.pixel(1, 1), [128, 128, 128, 255]);
        assert_eq!(r.pixel(2, 2), [128, 128, 128, 255]);
        assert_eq!(r.pixel(3, 3), [0, 0, 0, 255]);
        // Over transparent: stays premultiplied.
        r.clear(Color::TRANSPARENT);
        r.draw(None, Rect::new(0, 0, 4, 4), &[Quad { dst: Rect::new(0, 0, 1, 1), src: Rect::ZERO, color: half_white }]);
        assert_eq!(r.pixel(0, 0), [128, 128, 128, 128]);
    }

    #[test]
    fn bitmap_times_color() {
        let mut r = SoftRenderer::new(Size::new(2, 1));
        r.clear(Color::BLACK);
        let b = r.create_bitmap(Size::new(1, 1), &[200, 100, 50, 255]).unwrap();
        let tint = Color::rgba(255, 128, 0, 255).premultiplied();
        r.draw(
            Some(b),
            Rect::new(0, 0, 2, 1),
            &[
                Quad { dst: Rect::new(0, 0, 1, 1), src: Rect::new(0, 0, 1, 1), color: Color::WHITE },
                Quad { dst: Rect::new(1, 0, 1, 1), src: Rect::new(0, 0, 1, 1), color: tint },
            ],
        );
        assert_eq!(r.pixel(0, 0), [200, 100, 50, 255]);
        assert_eq!(r.pixel(1, 0), [200, 50, 0, 255]);
    }

    #[test]
    fn clip_and_framebuffer_bounds() {
        let mut r = SoftRenderer::new(Size::new(4, 4));
        r.clear(Color::BLACK);
        let red = Color::rgb(255, 0, 0);
        r.draw(
            None,
            Rect::new(-10, 1, 100, 2),
            &[Quad { dst: Rect::new(-5, -5, 20, 20), src: Rect::ZERO, color: red }],
        );
        for y in 0..4 {
            for x in 0..4 {
                let want = if (1..3).contains(&y) { [255, 0, 0, 255] } else { [0, 0, 0, 255] };
                assert_eq!(r.pixel(x, y), want, "({x}, {y})");
            }
        }
    }

    #[test]
    fn bitmap_slots_and_stats() {
        let mut r = SoftRenderer::new(Size::new(1, 1));
        let px = [1u8; 16];
        let a = r.create_bitmap(Size::new(2, 2), &px).unwrap();
        let b = r.create_bitmap(Size::new(1, 1), &px).unwrap();
        assert_ne!(a, b);
        assert_eq!(r.create_bitmap(Size::new(3, 3), &px), None); // too little data
        assert_eq!(r.create_bitmap(Size::new(0, 3), &px), None);
        r.update_bitmap(a, &[2u8; 16]);
        assert_eq!(r.bitmap_pixels(a), Some(&[2u8; 16][..]));
        r.destroy_bitmap(a);
        r.destroy_bitmap(a); // already gone: not counted
        assert_eq!(r.bitmap_size(a), None);
        let c = r.create_bitmap(Size::new(1, 1), &px).unwrap();
        assert_eq!(c, a); // slot reused
        assert_eq!(r.bitmap_count(), 2);
        r.begin_frame(Size::new(1, 1));
        r.draw(None, Rect::new(0, 0, 1, 1), &[]);
        r.draw(Some(BitmapId(99)), Rect::new(0, 0, 1, 1), &[]);
        assert_eq!(
            r.stats(),
            Stats { frames: 1, draw_calls: 2, quads: 0, bitmaps_created: 3, bitmaps_updated: 1, bitmaps_destroyed: 1 }
        );
        r.reset_stats();
        assert_eq!(r.stats(), Stats::default());
    }

    #[test]
    fn resize_clears() {
        let mut r = SoftRenderer::new(Size::new(2, 2));
        r.clear(Color::WHITE);
        r.resize(Size::new(2, 2));
        assert_eq!(r.pixel(1, 1), [255; 4]);
        r.resize(Size::new(3, 1));
        assert_eq!(r.size(), Size::new(3, 1));
        assert_eq!(r.pixels(), &[0u8; 12][..]);
    }
}
