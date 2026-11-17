//! Golden-image tests for the software renderer (and the painter and atlas driving it).
//! Bless with `BLESS=1 cargo test -p turbo-ferret-soft`.

mod common;

use common::assert_golden;
use turbo_ferret::geom::{Color, Point, Rect, Size};
use turbo_ferret::render::atlas::Atlas;
use turbo_ferret::render::painter::Painter;
use turbo_ferret::render::{BitmapId, Quad, Renderer};
use turbo_ferret_soft::SoftRenderer;

const BG: Color = Color::rgb(24, 24, 32);

/// A 4x3 opaque image with a distinct color per texel, so orientation, mirroring and
/// stretching are visible.
const IMG_SIZE: Size = Size::new(4, 3);
#[rustfmt::skip]
const IMG: [[u8; 3]; 12] = [
    [230, 40, 40], [40, 200, 40], [40, 80, 230], [240, 220, 40],
    [40, 210, 210], [210, 40, 210], [250, 250, 250], [240, 140, 30],
    [120, 120, 120], [20, 100, 40], [110, 40, 160], [250, 160, 190],
];
const IMG_RECT: Rect = Rect::from_size(IMG_SIZE);

fn img_rgba() -> Vec<u8> {
    IMG.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect()
}

/// White with alpha ramping 0..255 over `w` texels (premultiplied).
fn ramp_rgba(w: i32, h: i32) -> Vec<u8> {
    (0..h).flat_map(|_| (0..w).flat_map(move |x| [((x * 255) / (w - 1)) as u8; 4])).collect()
}

/// A soft disc of `d` x `d` texels (premultiplied white).
fn disc_rgba(d: i32) -> Vec<u8> {
    let r = d as f32 / 2.0;
    let mut v = Vec::new();
    for y in 0..d {
        for x in 0..d {
            let (dx, dy) = (x as f32 + 0.5 - r, y as f32 + 0.5 - r);
            let dist = (dx * dx + dy * dy).sqrt();
            let a = ((r - dist) * 128.0).clamp(0.0, 255.0) as u8;
            v.extend_from_slice(&[a, a, a, a]);
        }
    }
    v
}

fn quad(dst: Rect, src: Rect, color: Color) -> Quad {
    Quad { dst, src, color: color.premultiplied() }
}

fn fill(dst: Rect, color: Color) -> Quad {
    quad(dst, Rect::ZERO, color)
}

fn setup(w: i32, h: i32) -> (SoftRenderer, Rect) {
    let mut r = SoftRenderer::new(Size::new(w, h));
    r.clear(BG);
    (r, Rect::new(0, 0, w, h))
}

fn image(r: &mut SoftRenderer) -> BitmapId {
    r.create_bitmap(IMG_SIZE, &img_rgba()).unwrap()
}

#[test]
fn solid_fill() {
    let (mut r, full) = setup(32, 24);
    r.draw(
        None,
        full,
        &[
            fill(Rect::new(2, 2, 10, 8), Color::rgb(230, 40, 40)),
            fill(Rect::new(14, 2, 10, 8), Color::rgb(40, 200, 40)),
            fill(Rect::new(26, -4, 10, 10), Color::rgb(40, 80, 230)), // off the top right
            fill(Rect::new(40, 0, 5, 5), Color::WHITE),               // off screen
            fill(Rect::new(18, 14, 1, 1), Color::WHITE),
            fill(Rect::new(20, 14, 12, 1), Color::WHITE),
            fill(Rect::new(20, 16, 1, 8), Color::WHITE),
        ],
    );
    // Clipped by the clip rect: only x 4..16, y 12..20 is drawn.
    r.draw(None, Rect::new(0, 12, 16, 12), &[fill(Rect::new(4, 10, 20, 10), Color::rgb(240, 220, 40))]);
    assert_golden("soft_solid_fill", &r);
}

#[test]
fn blit_1to1() {
    let (mut r, full) = setup(16, 12);
    let b = image(&mut r);
    r.draw(
        Some(b),
        full,
        &[
            quad(Rect::new(2, 2, 4, 3), IMG_RECT, Color::WHITE),
            quad(Rect::new(8, 2, 2, 2), Rect::new(1, 1, 2, 2), Color::WHITE), // sub-rect
            quad(Rect::new(-2, 7, 4, 3), IMG_RECT, Color::WHITE),             // left half off screen
            quad(Rect::new(14, 7, 4, 3), IMG_RECT, Color::WHITE),             // right half off screen
        ],
    );
    assert_golden("soft_blit_1to1", &r);
    // Spot checks independent of the golden.
    assert_eq!(r.pixel(2, 2), [230, 40, 40, 255]);
    assert_eq!(r.pixel(5, 4), [250, 160, 190, 255]);
    assert_eq!(r.pixel(8, 2), [210, 40, 210, 255]);
    assert_eq!(r.pixel(0, 7), [40, 80, 230, 255]);
    assert_eq!(r.pixel(15, 9), [20, 100, 40, 255]);
}

#[test]
fn blit_stretched() {
    let (mut r, full) = setup(40, 24);
    let b = image(&mut r);
    r.draw(
        Some(b),
        full,
        &[
            quad(Rect::new(1, 1, 16, 12), IMG_RECT, Color::WHITE), // 4x
            quad(Rect::new(19, 1, 10, 7), IMG_RECT, Color::WHITE), // 2.5x by 2.33x
            quad(Rect::new(31, 1, 2, 2), IMG_RECT, Color::WHITE),  // shrunk
            quad(Rect::new(19, 10, 8, 12), Rect::new(1, 0, 2, 3), Color::WHITE), // sub-rect 4x
            quad(Rect::new(30, 10, 9, 3), IMG_RECT, Color::WHITE), // horizontal only
        ],
    );
    assert_golden("soft_blit_stretched", &r);
    assert_eq!(r.pixel(4, 4), [230, 40, 40, 255]);
    assert_eq!(r.pixel(5, 5), [210, 40, 210, 255]);
    assert_eq!(r.pixel(16, 12), [250, 160, 190, 255]);
}

#[test]
fn blit_mirrored() {
    let (mut r, full) = setup(32, 16);
    let b = image(&mut r);
    r.draw(
        Some(b),
        full,
        &[
            quad(Rect::new(1, 1, 4, 3), IMG_RECT, Color::WHITE),
            quad(Rect::new(6, 1, 4, 3), Rect::new(4, 0, -4, 3), Color::WHITE), // mirror x
            quad(Rect::new(11, 1, 4, 3), Rect::new(0, 3, 4, -3), Color::WHITE), // mirror y
            quad(Rect::new(16, 1, 4, 3), Rect::new(4, 3, -4, -3), Color::WHITE), // both
            quad(Rect::new(1, 6, 8, 6), Rect::new(4, 0, -4, 3), Color::WHITE), // mirror x, 2x
            quad(Rect::new(11, 6, 2, 2), Rect::new(3, 1, -2, 2), Color::WHITE), // mirrored sub-rect
            quad(Rect::new(14, 6, 6, 6), Rect::new(4, 3, -4, -3), Color::WHITE), // both, 1.5x
            quad(Rect::new(28, 6, 8, 6), Rect::new(4, 0, -4, 3), Color::WHITE), // mirror x, off screen
        ],
    );
    assert_golden("soft_blit_mirrored", &r);
    assert_eq!(r.pixel(6, 1), [240, 220, 40, 255]);
    assert_eq!(r.pixel(9, 1), [230, 40, 40, 255]);
    assert_eq!(r.pixel(11, 1), [120, 120, 120, 255]);
    assert_eq!(r.pixel(16, 1), [250, 160, 190, 255]);
    assert_eq!(r.pixel(11, 6), [250, 250, 250, 255]);
    assert_eq!(r.pixel(12, 6), [210, 40, 210, 255]);
    assert_eq!(r.pixel(28, 6), [240, 220, 40, 255]);
}

#[test]
fn blit_clipped() {
    let (mut r, full) = setup(32, 24);
    let b = image(&mut r);
    // Outline of the clip rect, to show where it is.
    r.draw(None, full, &[fill(Rect::new(5, 3, 10, 8), Color::rgb(90, 90, 110))]);
    r.draw(
        Some(b),
        Rect::new(6, 4, 8, 6),
        &[
            quad(Rect::new(2, 2, 16, 12), IMG_RECT, Color::WHITE),
            quad(Rect::new(20, 0, 4, 3), IMG_RECT, Color::WHITE), // outside the clip
        ],
    );
    r.draw(Some(b), full, &[quad(Rect::new(-4, 13, 12, 9), Rect::new(4, 0, -4, 3), Color::WHITE)]);
    r.draw(Some(b), Rect::new(22, 0, 10, 24), &[quad(Rect::new(18, 12, 12, 9), IMG_RECT, Color::WHITE)]);
    assert_golden("soft_blit_clipped", &r);
    assert_eq!(r.pixel(5, 4), [90, 90, 110, 255]); // just outside the clip
    assert_eq!(r.pixel(6, 4), [40, 200, 40, 255]); // texel (1, 0) of the 4x quad at (2, 2)
    assert_eq!(r.pixel(13, 9), [250, 250, 250, 255]);
    assert_eq!(r.pixel(21, 12), BG.premultiplied().into_array());
}

#[test]
fn blend_semi_transparent() {
    let (mut r, full) = setup(32, 28);
    r.draw(None, full, &[fill(Rect::new(16, 0, 16, 28), Color::rgb(200, 200, 200))]);
    r.draw(
        None,
        full,
        &[
            fill(Rect::new(2, 2, 14, 10), Color::rgba(255, 0, 0, 128)),
            fill(Rect::new(8, 5, 14, 10), Color::rgba(0, 255, 0, 128)),
            fill(Rect::new(14, 8, 14, 8), Color::rgba(0, 0, 255, 64)),
        ],
    );
    let ramp = r.create_bitmap(Size::new(8, 1), &ramp_rgba(8, 1)).unwrap();
    r.draw(
        Some(ramp),
        full,
        &[
            quad(Rect::new(0, 18, 32, 4), Rect::new(0, 0, 8, 1), Color::WHITE),
            quad(Rect::new(0, 23, 32, 4), Rect::new(0, 0, 8, 1), Color::rgba(255, 128, 0, 200)), // tinted
        ],
    );
    assert_golden("soft_blend", &r);
    // Half red (premultiplied 128, 0, 0, 128) over (24, 24, 32): s + d * 127 / 255.
    let bg = BG.premultiplied();
    assert_eq!(r.pixel(3, 3), [128 + 12, 12, 16, 255]);
    // Ramp: first texel transparent (background shows), last opaque white.
    assert_eq!(r.pixel(0, 18), bg.into_array());
    assert_eq!(r.pixel(31, 18), [255, 255, 255, 255]);
}

#[test]
fn painter_scene() {
    let size = Size::new(64, 48);
    let mut r = SoftRenderer::new(size);
    r.clear(BG);

    let mut atlas = Atlas::new(Size::new(32, 32));
    let img = atlas.add(IMG_SIZE, &img_rgba()).unwrap();
    let disc = atlas.add(Size::new(12, 12), &disc_rgba(12)).unwrap();
    let bar = atlas.add(Size::new(40, 4), &ramp_rgba(40, 4)).unwrap(); // dedicated page
    assert_eq!((img.page, disc.page, bar.page), (0, 0, 1));
    atlas.upload_all(&mut r);
    assert_eq!(r.stats().bitmaps_created, 2);
    let page0 = atlas.bitmap(0).unwrap();
    let page1 = atlas.bitmap(1).unwrap();
    r.reset_stats();

    let mut p = Painter::new(&mut r, size);
    p.set_translation(Point::new(4, 4));
    let panel = Rect::new(0, 0, 56, 40);
    p.fill_rect(panel, Color::rgb(50, 50, 70));
    p.draw_rect(panel, Color::rgb(200, 200, 220));

    // Images from the atlas: 1:1, 3x, mirrored 2x.
    p.draw_bitmap(Rect::new(3, 3, 4, 3), img.rect, page0, Color::WHITE);
    p.draw_bitmap(Rect::new(9, 3, 12, 9), img.rect, page0, Color::WHITE);
    let mirrored = Rect::new(img.rect.right(), img.rect.y, -img.rect.w, img.rect.h);
    p.draw_bitmap(Rect::new(23, 3, 8, 6), mirrored, page0, Color::WHITE);
    p.draw_bitmap(Rect::new(40, 3, 12, 12), disc.rect, page0, Color::WHITE);
    p.draw_bitmap(Rect::new(40, 17, 12, 12), disc.rect, page0, Color::rgb(100, 200, 255));

    // A half transparent group, translated further.
    p.set_translation(Point::new(4 + 3, 4 + 15));
    p.set_opacity(0.5);
    p.fill_rect(Rect::new(0, 0, 30, 10), Color::rgb(230, 40, 40));
    p.draw_bitmap(Rect::new(2, 1, 12, 9), img.rect, page0, Color::WHITE);
    p.draw_rect(Rect::new(16, 1, 12, 8), Color::WHITE);
    p.set_opacity(1.0);

    // A clipped region (screen space) with the bar from the dedicated page.
    p.set_translation(Point::new(4, 4));
    let old = p.set_clip(Rect::new(10, 34, 40, 6));
    p.draw_bitmap(Rect::new(0, 28, 52, 8), bar.rect, page1, Color::WHITE);
    p.draw_rect(Rect::new(4, 30, 20, 20), Color::rgb(240, 220, 40));
    p.set_clip(old);
    p.fill_rect(Rect::new(54, 30, 4, 4), Color::rgb(40, 200, 40)); // straddles the panel edge
    p.finish();

    assert_golden("painter_scene", &r);
    // Batches: panel fills (solid), page0 images, half-transparent fill, page0 image,
    // rect (solid), bar (page1, clipped), clipped rect (solid), final fill (solid,
    // unclipped).
    assert_eq!(r.stats().draw_calls, 8);
    assert_eq!(r.stats().frames, 1);
}

trait IntoArray {
    fn into_array(self) -> [u8; 4];
}

impl IntoArray for Color {
    fn into_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}
