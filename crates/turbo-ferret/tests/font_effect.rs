//! The font blur effect: glyphs are blurred as they go into the glyph cache.
//! TB: tb_font_renderer.cpp (TBFontEffect)

use turbo_ferret::font::{
    BOX_FONT, FontBackend, FontDesc, FontManager, FontMetrics, GlyphFormat, GlyphImage, GlyphMetrics, GlyphSource,
    LoadCtx, MAX_BLUR_RADIUS,
};
use turbo_ferret::services::NoImages;
use turbo_ferret::{MemLoader, Size, TbId};

/// A font of solid blocks: `.` is one pixel, `#` is 3x3, `c` is a 2x2 color glyph.
struct Blocks;

impl FontBackend for Blocks {
    fn open(&self, path: &str, _data: &[u8], _size: u16, _cx: &mut LoadCtx) -> Option<Box<dyn GlyphSource>> {
        path.ends_with(".blocks").then(|| Box::new(Blocks) as Box<dyn GlyphSource>)
    }
}

impl GlyphSource for Blocks {
    fn metrics(&self) -> FontMetrics {
        FontMetrics { ascent: 8, descent: 2, height: 10 }
    }
    fn glyph_metrics(&mut self, c: char) -> Option<GlyphMetrics> {
        let side = match c {
            '.' => 1,
            '#' => 3,
            'c' => 2,
            _ => return None,
        };
        Some(GlyphMetrics { advance: side + 1, x: 0, y: -side, w: side, h: side })
    }
    fn rasterize(&mut self, c: char, out: &mut GlyphImage) -> bool {
        let Some(m) = self.glyph_metrics(c) else { return false };
        out.size = Size::new(m.w, m.h);
        if c == 'c' {
            out.format = GlyphFormat::Rgba;
            out.color = true;
            out.data = [200, 100, 50, 255].repeat(4);
        } else {
            out.data = vec![255; (m.w * m.h) as usize];
        }
        true
    }
}

const FONT: FontDesc = FontDesc::new(TbId::new("Blocks"), 10);

fn manager() -> FontManager {
    let mut fm = FontManager::new();
    fm.add_backend(Box::new(Blocks));
    fm.add_font("Blocks", "a.blocks");
    let mut loader = MemLoader::new();
    loader.add_owned("a.blocks", vec![0]);
    let mut images = NoImages;
    assert!(fm.create_face(FONT, &mut LoadCtx::new(&mut loader, &mut images)));
    fm
}

/// The alpha of a cached glyph's pixels, row by row, with its metrics.
fn glyph(fm: &mut FontManager, c: char) -> (GlyphMetrics, Vec<u8>) {
    assert!(fm.render_glyphs(FONT, &c.to_string()));
    let g = *fm.glyph_cache().glyph(FONT, c).expect("cached");
    let frag = g.frag.expect("has an image");
    let atlas = fm.glyph_cache().atlas();
    let page = atlas.page_pixels(frag.page).unwrap();
    let pw = atlas.page_size(frag.page).w;
    let mut alpha = Vec::new();
    for y in frag.rect.y..frag.rect.y + frag.rect.h {
        for x in frag.rect.x..frag.rect.x + frag.rect.w {
            alpha.push(page[((y * pw + x) * 4 + 3) as usize]);
        }
    }
    assert_eq!((g.metrics.w, g.metrics.h), (frag.rect.w, frag.rect.h));
    (g.metrics, alpha)
}

#[test]
fn no_effect_by_default() {
    let mut fm = manager();
    assert_eq!(fm.face(FONT).effect().blur_radius(), 0);
    let (m, alpha) = glyph(&mut fm, '#');
    assert_eq!(m, GlyphMetrics { advance: 4, x: 0, y: -3, w: 3, h: 3 });
    assert_eq!(alpha, [255; 9]);
}

#[test]
fn blur_spreads_a_pixel_into_a_bell() {
    let mut fm = manager();
    assert!(fm.set_blur_radius(FONT, 2));
    assert_eq!(fm.face(FONT).effect().blur_radius(), 2);
    let (m, a) = glyph(&mut fm, '.');
    // The glyph grows by the radius on each side and moves up and left by it; the
    // advance stays.
    assert_eq!(m, GlyphMetrics { advance: 2, x: -2, y: -3, w: 5, h: 5 });
    let at = |x: usize, y: usize| a[y * 5 + x];
    // A bell: highest in the middle, falling off, the same in every direction.
    assert!(at(2, 2) > at(1, 2) && at(1, 2) > at(0, 2) && at(1, 2) > at(1, 1) && at(1, 1) > at(0, 0));
    for y in 0..5 {
        for x in 0..5 {
            assert_eq!(at(x, y), at(4 - x, y));
            assert_eq!(at(x, y), at(x, 4 - y));
            assert_eq!(at(x, y), at(y, x));
        }
    }
    // The kernel is a Gaussian with deviation 1 over five taps, normalized:
    // exp(-x²/2) for x = -2..2 is 0.1353, 0.6065, 1, 0.6065, 0.1353 (sum 2.4837).
    let k = [0.1353f32 / 2.4837, 0.6065 / 2.4837, 1.0 / 2.4837];
    for (x, kx) in k.iter().enumerate() {
        for (y, ky) in k.iter().enumerate() {
            let want = 255.0 * kx * ky;
            assert!((f32::from(at(x, y)) - want).abs() <= 1.0, "pixel {x},{y}: {} vs {want}", at(x, y));
        }
    }
    // The ink is spread, not lost.
    let sum: u32 = a.iter().map(|&v| u32::from(v)).sum();
    assert!(sum.abs_diff(255) <= 12, "{sum}");
}

#[test]
fn blur_keeps_the_inside_of_a_large_glyph_and_softens_its_edge() {
    let mut fm = manager();
    fm.set_blur_radius(FONT, 1);
    let (m, a) = glyph(&mut fm, '#');
    assert_eq!(m, GlyphMetrics { advance: 4, x: -1, y: -4, w: 5, h: 5 });
    assert!(a[2 * 5 + 2] >= 253, "the middle stays solid: {}", a[2 * 5 + 2]);
    assert!(a[0] > 0 && a[0] < a[5 + 1] && a[5 + 1] < a[2 * 5 + 2]);
    let sum: u32 = a.iter().map(|&v| u32::from(v)).sum();
    assert!(sum.abs_diff(9 * 255) <= 25, "{sum}");
}

#[test]
fn changing_the_radius_renders_the_glyphs_again() {
    let mut fm = manager();
    let (plain, _) = glyph(&mut fm, '#');
    assert_eq!(plain.w, 3);
    assert!(fm.set_blur_radius(FONT, 3));
    assert!(fm.glyph_cache().glyph(FONT, '#').is_none(), "forgotten");
    assert_eq!(glyph(&mut fm, '#').0.w, 9);
    // The same radius again keeps them.
    assert!(fm.set_blur_radius(FONT, 3));
    assert!(fm.glyph_cache().glyph(FONT, '#').is_some());
    // Off again.
    assert!(fm.set_blur_radius(FONT, 0));
    assert_eq!(glyph(&mut fm, '#'), (plain, vec![255; 9]));
    // Measuring is not affected by the effect.
    fm.set_blur_radius(FONT, 5);
    assert_eq!(fm.face(FONT).string_width("#.#"), 4 + 2 + 4);
}

#[test]
fn color_glyphs_are_not_blurred() {
    let mut fm = manager();
    fm.set_blur_radius(FONT, 2);
    let (m, a) = glyph(&mut fm, 'c');
    assert_eq!(m, GlyphMetrics { advance: 3, x: 0, y: -2, w: 2, h: 2 });
    assert_eq!(a, [255; 4]);
}

#[test]
fn faces_that_do_not_exist_and_large_radii() {
    let mut fm = manager();
    assert!(!fm.set_blur_radius(FontDesc::new(TbId::new("Blocks"), 11), 2), "no face at that size");
    assert!(!fm.set_blur_radius(BOX_FONT, 2), "the box font has no glyphs");
    assert!(fm.set_blur_radius(FONT, 255));
    assert_eq!(fm.face(FONT).effect().blur_radius(), MAX_BLUR_RADIUS);
    let (m, a) = glyph(&mut fm, '.');
    let side = 1 + 2 * i32::from(MAX_BLUR_RADIUS);
    assert_eq!((m.w, m.h, m.x), (side, side, -i32::from(MAX_BLUR_RADIUS)));
    assert_eq!(a.len(), (side * side) as usize);
}
