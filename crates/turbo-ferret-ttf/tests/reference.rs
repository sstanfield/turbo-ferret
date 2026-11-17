//! The rasterizer against an established one (fontdue, a dev-dependency only), on
//! DejaVu Sans (the font TB ships as `vera.ttf`).

use std::rc::Rc;

use turbo_ferret::font::{GlyphImage, GlyphSource};
use turbo_ferret_ttf::{Font, TtfFace};

fn dejavu() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/DejaVuSans.ttf");
    std::fs::read(path).expect("resources/DejaVuSans.ttf")
}

/// Left margin of the canvas the glyph images are compared on.
const PAD: i32 = 8;

/// Draw a glyph image on a canvas: `x` from the pen, `y` from the baseline (which
/// is at half the canvas height).
#[allow(clippy::too_many_arguments)]
fn place(canvas: &mut [u8], cw: i32, ch: i32, x: i32, y: i32, w: i32, h: i32, data: &[u8]) {
    for row in 0..h {
        for col in 0..w {
            let (cx, cy) = (x + col + PAD, y + row + ch / 2);
            if cx >= 0 && cy >= 0 && cx < cw && cy < ch {
                canvas[(cy * cw + cx) as usize] = data[(row * w + col) as usize];
            }
        }
    }
}

#[test]
fn glyphs_match_the_reference_rasterizer() {
    let data = dejavu();
    let reference = fontdue::Font::from_bytes(&data[..], fontdue::FontSettings::default()).unwrap();
    let font = Rc::new(Font::parse(data).expect("parse"));

    let chars: Vec<char> = (' '..='~').chain('\u{a1}'..='\u{ff}').chain("ŁłŠšŽžΩπ€…–“”•™√∞≠≤≥".chars()).collect();
    let mut worst_max = 0u8;
    let mut worst_mean = 0.0f64;
    let mut compared = 0;
    for size in [9u16, 12, 14, 18, 24, 37, 64] {
        let mut face = TtfFace::new(font.clone(), size);
        for &c in &chars {
            assert!(face.has_glyph(c), "{c:?} is in the font");
            let m = face.glyph_metrics(c).unwrap();
            let (rm, rdata) = reference.rasterize(c, f32::from(size));

            // The advance: both round the same scaled value.
            assert_eq!(m.advance, rm.advance_width.round() as i32, "advance of {c:?} at {size}");

            let mut img = GlyphImage::new();
            let drew = face.rasterize(c, &mut img);
            assert_eq!(drew, rm.width > 0 && rm.height > 0, "{c:?} at {size}: has an image");
            if !drew {
                continue;
            }
            assert_eq!(img.data.len(), (m.w * m.h) as usize);

            // Both into one frame: x from the pen, y from the baseline (down).
            let (cw, ch) = (i32::from(size) * 3 + 2 * PAD, i32::from(size) * 4);
            let mut ours = vec![0u8; (cw * ch) as usize];
            let mut theirs = vec![0u8; (cw * ch) as usize];
            place(&mut ours, cw, ch, m.x, m.y, m.w, m.h, &img.data);
            let (rw, rh) = (rm.width as i32, rm.height as i32);
            place(&mut theirs, cw, ch, rm.xmin, -(rm.ymin + rh), rw, rh, &rdata);

            let mut max = 0u8;
            let mut sum = 0u64;
            let mut inked = 0u64;
            for (a, b) in ours.iter().zip(&theirs) {
                let d = a.abs_diff(*b);
                max = max.max(d);
                sum += u64::from(d);
                if *a > 0 || *b > 0 {
                    inked += 1;
                }
            }
            let mean = sum as f64 / inked.max(1) as f64;
            // The two flatten curves differently (the reference more coarsely, see
            // `coverage_is_accurate`), so edge pixels differ a little.
            assert!(max <= 40, "{c:?} at {size}: a pixel differs by {max}");
            assert!(mean <= 6.0, "{c:?} at {size}: pixels differ by {mean:.2} on average");
            worst_max = worst_max.max(max);
            worst_mean = worst_mean.max(mean);
            compared += 1;
        }
    }
    println!("compared {compared} glyph images: worst pixel difference {worst_max}, worst mean {worst_mean:.3}");
    assert!(compared > 1200);
}

#[test]
fn face_metrics_match_the_reference() {
    let data = dejavu();
    let reference = fontdue::Font::from_bytes(&data[..], fontdue::FontSettings::default()).unwrap();
    let font = Rc::new(Font::parse(data).unwrap());
    assert_eq!(font.units_per_em(), 2048);
    assert_eq!(f32::from(font.units_per_em()), reference.units_per_em());
    for size in [10u16, 14, 28, 100] {
        let face = TtfFace::new(font.clone(), size);
        let lm = reference.horizontal_line_metrics(f32::from(size)).unwrap();
        let m = face.metrics();
        assert_eq!(m.ascent, lm.ascent.round() as i32);
        assert_eq!(m.descent, (-lm.descent).round() as i32);
        assert!((m.height - lm.new_line_size.round() as i32).abs() <= 1, "{} vs {}", m.height, lm.new_line_size);
    }
    // Characters and glyph indices.
    for c in "AZaz09 !~éßÿŁ€Ωπ√".chars() {
        assert_eq!(font.glyph_index(c), Some(reference.lookup_glyph_index(c)), "{c:?}");
    }
    assert_eq!(font.glyph_index('\u{e000}'), None, "a private use character the font lacks");
    assert_eq!(reference.lookup_glyph_index('\u{e000}'), 0);
}

/// Coverage of `c` at `size` computed the slow way: rasterized 16 times larger and
/// averaged down. Returned on the same canvas as `place` draws on.
fn supersampled(font: &Rc<Font>, c: char, size: u16, cw: i32, ch: i32) -> Vec<u8> {
    const K: i32 = 16;
    let mut big = TtfFace::new(font.clone(), size * K as u16);
    let m = big.glyph_metrics(c).unwrap();
    let mut img = GlyphImage::new();
    let mut sums = vec![0u32; (cw * ch) as usize];
    if big.rasterize(c, &mut img) {
        for row in 0..m.h {
            for col in 0..m.w {
                // The big pixel's position from the pen, then the small pixel it is in.
                let (bx, by) = (m.x + col, m.y + row);
                let (cx, cy) = (bx.div_euclid(K) + PAD, by.div_euclid(K) + ch / 2);
                if cx >= 0 && cy >= 0 && cx < cw && cy < ch {
                    sums[(cy * cw + cx) as usize] += u32::from(img.data[(row * m.w + col) as usize]);
                }
            }
        }
    }
    sums.iter().map(|&s| ((s as f32 / (K * K) as f32) + 0.5) as u8).collect()
}

#[test]
fn coverage_is_accurate() {
    let data = dejavu();
    let reference = fontdue::Font::from_bytes(&data[..], fontdue::FontSettings::default()).unwrap();
    let font = Rc::new(Font::parse(data).unwrap());
    let (mut our_max, mut ref_max) = (0u8, 0u8);
    let (mut our_sum, mut ref_sum, mut n) = (0u64, 0u64, 0u64);
    for size in [9u16, 14, 24, 37, 64] {
        let mut face = TtfFace::new(font.clone(), size);
        for c in "cCO()$&@gé∞®Ω%8sS".chars() {
            let (cw, ch) = (i32::from(size) * 3 + 2 * PAD, i32::from(size) * 4);
            let truth = supersampled(&font, c, size, cw, ch);

            let m = face.glyph_metrics(c).unwrap();
            let mut img = GlyphImage::new();
            assert!(face.rasterize(c, &mut img));
            let mut ours = vec![0u8; (cw * ch) as usize];
            place(&mut ours, cw, ch, m.x, m.y, m.w, m.h, &img.data);

            let (rm, rdata) = reference.rasterize(c, f32::from(size));
            let mut theirs = vec![0u8; (cw * ch) as usize];
            let (rw, rh) = (rm.width as i32, rm.height as i32);
            place(&mut theirs, cw, ch, rm.xmin, -(rm.ymin + rh), rw, rh, &rdata);

            for i in 0..truth.len() {
                let (a, b) = (ours[i].abs_diff(truth[i]), theirs[i].abs_diff(truth[i]));
                our_max = our_max.max(a);
                ref_max = ref_max.max(b);
                if truth[i] > 0 || ours[i] > 0 || theirs[i] > 0 {
                    our_sum += u64::from(a);
                    ref_sum += u64::from(b);
                    n += 1;
                }
            }
        }
    }
    let (our_mean, ref_mean) = (our_sum as f64 / n as f64, ref_sum as f64 / n as f64);
    println!(
        "against 16x supersampling: ours max {our_max} mean {our_mean:.3}; reference max {ref_max} mean {ref_mean:.3}"
    );
    assert!(our_max <= 6, "a pixel is {our_max} off");
    assert!(our_mean <= 1.0, "pixels are {our_mean:.3} off on average");
    assert!(our_mean <= ref_mean, "less accurate than the reference: {our_mean:.3} vs {ref_mean:.3}");
}

/// Where two contours overlap (the cedilla and the C of 'Ç'), the anti-aliased
/// pixels of both add up instead of being merged. This is the method's known
/// limit; the test pins how large the effect is.
#[test]
fn overlapping_contours_are_slightly_bold_at_the_joint() {
    let font = Rc::new(Font::parse(dejavu()).unwrap());
    for size in [14u16, 37] {
        let (cw, ch) = (i32::from(size) * 3 + 2 * PAD, i32::from(size) * 4);
        let truth = supersampled(&font, 'Ç', size, cw, ch);
        let mut face = TtfFace::new(font.clone(), size);
        let m = face.glyph_metrics('Ç').unwrap();
        let mut img = GlyphImage::new();
        assert!(face.rasterize('Ç', &mut img));
        let mut ours = vec![0u8; (cw * ch) as usize];
        place(&mut ours, cw, ch, m.x, m.y, m.w, m.h, &img.data);
        let off: Vec<(u8, u8)> =
            ours.iter().zip(&truth).filter(|(a, b)| a.abs_diff(**b) > 6).map(|(a, b)| (*a, *b)).collect();
        assert!(!off.is_empty() && off.len() <= 6, "{} pixels are off at {size}", off.len());
        assert!(off.iter().all(|(a, b)| a > b && a - b <= 90), "only darker, and by a bounded amount: {off:?}");
    }
}
