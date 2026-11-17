//! Regressions and stress tests from the review (2026-11): outlines clipped at the
//! bitmap's border, wild coordinates, damaged fonts at large sizes, and a real font
//! whose glyph header box is smaller than its outline (macOS only; skipped elsewhere).
#![allow(dead_code)]
use std::rc::Rc;

use turbo_ferret::font::{GlyphImage, GlyphSource};
use turbo_ferret_ttf::{Font, Raster, TtfFace};

/// A point: x, y, on-curve.
type Pt = (i16, i16, bool);

/// A component of a composite glyph: glyph, x and y offset, scale.
type Part = (u16, i16, i16, Option<f32>);

#[derive(Clone, Default)]
struct Glyph {
    advance: u16,
    contours: Vec<Vec<Pt>>,
    parts: Vec<Part>,
    /// The header's box: x_min, y_min, x_max, y_max. Worked out from the contours
    /// if `None`.
    bounds: Option<(i16, i16, i16, i16)>,
}

fn glyph(advance: u16, contours: &[&[Pt]]) -> Glyph {
    Glyph { advance, contours: contours.iter().map(|c| c.to_vec()).collect(), ..Default::default() }
}

#[derive(Clone, Copy, Default)]
struct Opts {
    cmap12: bool,
    loca_long: bool,
    /// Wrap the font in a `.ttc` collection.
    collection: bool,
}

fn push16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}
fn push32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// A TrueType file with 2048 units per em, ascent 1600, descent -400, line gap 100.
fn build(glyphs: &[Glyph], map: &[(u32, u16)], opts: Opts) -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&2048u16.to_be_bytes());
    head[50..52].copy_from_slice(&(opts.loca_long as u16).to_be_bytes());
    let mut maxp = vec![0u8; 6];
    maxp[4..6].copy_from_slice(&(glyphs.len() as u16).to_be_bytes());
    let mut hhea = vec![0u8; 36];
    hhea[4..6].copy_from_slice(&1600i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-400i16).to_be_bytes());
    hhea[8..10].copy_from_slice(&100i16.to_be_bytes());
    hhea[34..36].copy_from_slice(&(glyphs.len() as u16).to_be_bytes());
    let mut hmtx = Vec::new();
    for g in glyphs {
        push16(&mut hmtx, g.advance);
        push16(&mut hmtx, 0);
    }

    let (mut glyf, mut offsets) = (Vec::new(), Vec::new());
    for g in glyphs {
        offsets.push(glyf.len() as u32);
        if g.contours.is_empty() && g.parts.is_empty() {
            continue; // no outline: an empty entry
        }
        let pts: Vec<Pt> = g.contours.iter().flatten().copied().collect();
        let b = g.bounds.unwrap_or_else(|| {
            let xs = pts.iter().map(|p| p.0);
            let ys = pts.iter().map(|p| p.1);
            (xs.clone().min().unwrap(), ys.clone().min().unwrap(), xs.max().unwrap(), ys.max().unwrap())
        });
        let count = if g.parts.is_empty() { g.contours.len() as i16 } else { -1 };
        push16(&mut glyf, count as u16);
        for v in [b.0, b.1, b.2, b.3] {
            push16(&mut glyf, v as u16);
        }
        if g.parts.is_empty() {
            let mut end = 0u16;
            for c in &g.contours {
                end += c.len() as u16;
                push16(&mut glyf, end - 1);
            }
            push16(&mut glyf, 0); // no instructions
            glyf.extend(pts.iter().map(|p| p.2 as u8)); // flags: word coordinates
            let mut prev = 0i16;
            for p in &pts {
                push16(&mut glyf, (p.0 - prev) as u16);
                prev = p.0;
            }
            prev = 0;
            for p in &pts {
                push16(&mut glyf, (p.1 - prev) as u16);
                prev = p.1;
            }
        } else {
            for (i, &(part, dx, dy, scale)) in g.parts.iter().enumerate() {
                // Word arguments that are x and y values, maybe a scale, maybe more.
                let more = if i + 1 < g.parts.len() { 0x20 } else { 0 };
                push16(&mut glyf, 0x0001 | 0x0002 | more | if scale.is_some() { 0x0008 } else { 0 });
                push16(&mut glyf, part);
                push16(&mut glyf, dx as u16);
                push16(&mut glyf, dy as u16);
                if let Some(s) = scale {
                    push16(&mut glyf, (s * 16384.0) as i16 as u16);
                }
            }
        }
        if glyf.len() % 2 == 1 {
            glyf.push(0);
        }
    }
    offsets.push(glyf.len() as u32);
    let mut loca = Vec::new();
    for o in offsets {
        if opts.loca_long {
            push32(&mut loca, o);
        } else {
            push16(&mut loca, (o / 2) as u16);
        }
    }

    let mut map = map.to_vec();
    map.sort();
    let mut cmap = Vec::new();
    push16(&mut cmap, 0);
    push16(&mut cmap, 1);
    push16(&mut cmap, 3); // Windows
    push16(&mut cmap, if opts.cmap12 { 10 } else { 1 });
    push32(&mut cmap, 12);
    if opts.cmap12 {
        push16(&mut cmap, 12);
        push16(&mut cmap, 0);
        push32(&mut cmap, 16 + 12 * map.len() as u32);
        push32(&mut cmap, 0);
        push32(&mut cmap, map.len() as u32);
        for &(c, g) in &map {
            push32(&mut cmap, c);
            push32(&mut cmap, c);
            push32(&mut cmap, u32::from(g));
        }
    } else {
        // One segment per character, then the closing 0xFFFF segment.
        let segs = map.len() as u16 + 1;
        push16(&mut cmap, 4);
        push16(&mut cmap, 16 + 8 * segs);
        push16(&mut cmap, 0);
        push16(&mut cmap, segs * 2);
        for _ in 0..3 {
            push16(&mut cmap, 0); // search hints, unused
        }
        for &(c, _) in &map {
            push16(&mut cmap, c as u16); // end codes
        }
        push16(&mut cmap, 0xFFFF);
        push16(&mut cmap, 0);
        for &(c, _) in &map {
            push16(&mut cmap, c as u16); // start codes
        }
        push16(&mut cmap, 0xFFFF);
        for &(c, g) in &map {
            push16(&mut cmap, g.wrapping_sub(c as u16)); // deltas
        }
        push16(&mut cmap, 1);
        for _ in 0..segs {
            push16(&mut cmap, 0); // range offsets
        }
    }

    let tables: [(&[u8; 4], Vec<u8>); 7] = [
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
    ];
    let mut out = Vec::new();
    if opts.collection {
        out.extend_from_slice(b"ttcf");
        push32(&mut out, 0x0001_0000);
        push32(&mut out, 1);
        push32(&mut out, 16);
    }
    let base = out.len();
    push32(&mut out, 0x0001_0000);
    push16(&mut out, tables.len() as u16);
    out.extend_from_slice(&[0; 6]);
    let mut offset = base + 12 + 16 * tables.len();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        push32(&mut out, 0);
        push32(&mut out, offset as u32);
        push32(&mut out, data.len() as u32);
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

/// A rectangle, clockwise in font coordinates (y up).
fn rect(x0: i16, y0: i16, x1: i16, y1: i16) -> Vec<Pt> {
    vec![(x0, y0, true), (x0, y1, true), (x1, y1, true), (x1, y0, true)]
}

/// The test font. At size 16 a pixel is 128 units.
/// 0: missing glyph (a small square), 1 'A': an 8x8 pixel square, 2 'B': a square on
/// half pixels, 3 'C': a triangle, 4 'D': a square with a hole, 5 'E': a square with
/// a square inside wound the same way, 6 'F': a round shape of off-curve points
/// only, 7 'G': 'A' at half size moved right, plus 'B', 8 ' ': nothing.
fn test_font(opts: Opts) -> Vec<u8> {
    let mut hole = rect(256, 256, 768, 768);
    hole.reverse();
    let r = 512i16;
    let glyphs = [
        glyph(600, &[&rect(128, 0, 384, 256)]),
        glyph(1200, &[&rect(0, 0, 1024, 1024)]),
        glyph(1100, &[&rect(192, 64, 832, 960)]),
        glyph(1000, &[&[(0, 0, true), (0, 1024, true), (1024, 0, true)]]),
        glyph(1200, &[&rect(0, 0, 1024, 1024), &hole]),
        glyph(1200, &[&rect(0, 0, 1024, 1024), &rect(256, 256, 768, 768)]),
        glyph(1300, &[&[(r + r, r + r, false), (r + r, 0, false), (0, 0, false), (0, r + r, false)]]),
        Glyph {
            advance: 1500,
            parts: vec![(1, 1024, 0, Some(0.5)), (2, 0, 0, None)],
            bounds: Some((192, 0, 1536, 960)),
            ..Default::default()
        },
        glyph(500, &[]),
    ];
    let mut map: Vec<(u32, u16)> = "ABCDEFG".chars().zip(1u16..).map(|(c, g)| (c as u32, g)).collect();
    map.push((' ' as u32, 8));
    map.push((0x20AC, 2)); // the euro sign is 'B' too
    if opts.cmap12 {
        map.push((0x1F600, 3));
    }
    build(&glyphs, &map, opts)
}

fn rng(seed: u32) -> impl FnMut() -> u32 {
    let mut state = seed;
    move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    }
}

/// The face at absurd sizes on a font whose units per em is tiny.
#[test]
fn tiny_units_per_em_and_huge_sizes() {
    for upem in [1u16, 2, 16] {
        for size in [1000u16, 32767, 40000, 65535] {
            let mut data = test_font(Opts::default());
            // Find the head table and patch unitsPerEm.
            let n = u16::from_be_bytes([data[4], data[5]]) as usize;
            for i in 0..n {
                let rec = 12 + i * 16;
                if &data[rec..rec + 4] == b"head" {
                    let off =
                        u32::from_be_bytes([data[rec + 8], data[rec + 9], data[rec + 10], data[rec + 11]]) as usize;
                    data[off + 18..off + 20].copy_from_slice(&upem.to_be_bytes());
                }
            }
            let r = std::panic::catch_unwind(|| {
                let font = Rc::new(Font::parse(data).unwrap());
                let mut face = TtfFace::new(font, size);
                let _ = face.metrics();
                for c in "ABCDEFG x".chars() {
                    let m = face.glyph_metrics(c).unwrap();
                    let mut img = GlyphImage::new();
                    if face.rasterize(c, &mut img) {
                        assert_eq!(img.data.len(), (m.w * m.h) as usize);
                    }
                }
            });
            println!("units per em {upem}, size {size}: {}", if r.is_ok() { "ok" } else { "PANIC" });
        }
    }
}

/// Damaged files at large sizes (the suite's own test uses 7 and 20).
#[test]
fn damaged_files_at_large_sizes() {
    let mut next = rng(0x1234_5678);
    let mut panics = 0;
    for opts in [Opts::default(), Opts { cmap12: true, loca_long: true, collection: true }] {
        let full = test_font(opts);
        for _ in 0..100 {
            let mut data = full.clone();
            for _ in 0..1 + next() % 4 {
                let at = next() as usize % data.len();
                data[at] = match next() % 4 {
                    0 => 0,
                    1 => 0xFF,
                    2 => 0x80,
                    _ => next() as u8,
                };
            }
            let r = std::panic::catch_unwind(|| {
                let Some(font) = Font::parse(data) else { return };
                let font = Rc::new(font);
                for size in [150u16, 700, 65535] {
                    let mut face = TtfFace::new(font.clone(), size);
                    let _ = face.metrics();
                    for c in "ABCDEFG €x😀".chars() {
                        let m = face.glyph_metrics(c).unwrap();
                        let mut img = GlyphImage::new();
                        if face.rasterize(c, &mut img) {
                            assert_eq!(img.data.len(), (m.w * m.h) as usize);
                        }
                    }
                }
            });
            if r.is_err() {
                panics += 1;
            }
        }
    }
    println!("panics: {panics}");
    assert_eq!(panics, 0);
}

/// The rasterizer with wild coordinates.
#[test]
fn raster_with_wild_coordinates() {
    let mut next = rng(0xdead_beef);
    let specials = [
        0.0f32,
        -0.0,
        1.0,
        -1.0,
        0.5,
        7.999_999,
        8.0,
        8.000_001,
        16.0,
        1e-30,
        -1e-30,
        1e9,
        -1e9,
        3.4e38,
        -3.4e38,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::MIN_POSITIVE,
        16_777_216.0,
        4_294_967_296.0,
    ];
    for i in 0..20_000 {
        let (w, h) = ((next() % 9) as usize, (next() % 9) as usize);
        let mut r = Raster::new(w, h);
        let mut v = || {
            if next() % 3 == 0 {
                specials[next() as usize % specials.len()]
            } else {
                (next() % 2400) as f32 / 100.0 - 8.0
            }
        };
        r.move_to(v(), v());
        for _ in 0..3 {
            if i % 2 == 0 {
                r.line_to(v(), v());
            } else {
                r.quad_to(v(), v(), v(), v());
            }
        }
        r.close();
        let out = r.to_coverage();
        assert_eq!(out.len(), w * h);
    }
}

/// A closed polygon entirely inside the bitmap: coverage sums to its area, whatever
/// the winding.
#[test]
fn raster_area_of_random_triangles() {
    let mut next = rng(0x0bad_cafe);
    let mut worst = 0.0f32;
    for _ in 0..20_000 {
        let (w, h) = (12usize, 12usize);
        let mut r = Raster::new(w, h);
        let mut v = || (next() % 1201) as f32 / 100.0; // 0..=12, often on integers
        let p = [(v(), v()), (v(), v()), (v(), v())];
        r.move_to(p[0].0, p[0].1);
        r.line_to(p[1].0, p[1].1);
        r.line_to(p[2].0, p[2].1);
        r.close();
        let out = r.to_coverage();
        let got: f32 = out.iter().map(|&a| f32::from(a)).sum::<f32>() / 255.0;
        let want = ((p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1)).abs() / 2.0;
        let err = (got - want).abs();
        if err > worst {
            worst = err;
            if err > 0.6 {
                println!("triangle {p:?}: area {want}, coverage {got}");
            }
        }
    }
    println!("worst area error: {worst}");
    assert!(worst < 0.6);
}

/// A glyph whose header box spans the whole i16 range, in a font with 1 unit per em.
#[test]
fn widest_box_at_huge_scale() {
    let mut g = glyph(600, &[&rect(0, 0, 100, 100)]);
    g.bounds = Some((-32768, -32768, 32767, 32767));
    let glyphs = [glyph(600, &[&rect(0, 0, 10, 10)]), g];
    let mut data = build(&glyphs, &[('A' as u32, 1)], Opts::default());
    let n = u16::from_be_bytes([data[4], data[5]]) as usize;
    for i in 0..n {
        let rec = 12 + i * 16;
        let off = u32::from_be_bytes([data[rec + 8], data[rec + 9], data[rec + 10], data[rec + 11]]) as usize;
        if &data[rec..rec + 4] == b"head" {
            data[off + 18..off + 20].copy_from_slice(&1u16.to_be_bytes());
        }
        if &data[rec..rec + 4] == b"hhea" {
            data[off + 4..off + 6].copy_from_slice(&32767i16.to_be_bytes());
            data[off + 6..off + 8].copy_from_slice(&(-32768i16).to_be_bytes());
        }
    }
    for size in [30000u16, 40000, 65535] {
        let d = data.clone();
        let r = std::panic::catch_unwind(move || {
            let font = Rc::new(Font::parse(d).unwrap());
            let mut face = TtfFace::new(font, size);
            let m = face.glyph_metrics('A').unwrap();
            let mut img = GlyphImage::new();
            let _ = face.rasterize('A', &mut img);
            m
        });
        println!("size {size}: {:?}", r.as_ref().map_err(|_| "PANIC"));
    }
}

/// A triangle drawn into a `w` by `h` raster, moved left by `dx`.
fn tri(w: usize, h: usize, dx: f32, pts: [(f32, f32); 3]) -> Vec<u8> {
    let mut r = Raster::new(w, h);
    r.move_to(pts[0].0 - dx, pts[0].1);
    r.line_to(pts[1].0 - dx, pts[1].1);
    r.line_to(pts[2].0 - dx, pts[2].1);
    r.close();
    r.to_coverage()
}

/// An outline that sticks out of the bitmap: the pixels inside the bitmap should be
/// what a larger bitmap shows there.
#[test]
fn clipped_outline_matches_crop() {
    // A wide flat triangle from x = -6 to x = 14 in an 8 wide bitmap.
    let pts = [(-6.0, 0.5), (14.0, 3.5), (-6.0, 7.5)];
    let (w, h) = (8usize, 8usize);
    let clipped = tri(w, h, 0.0, pts);
    // The same in a 24 wide bitmap whose column 8 is our column 0.
    let big = tri(24, h, -8.0, pts);
    let mut worst = 0u8;
    for y in 0..h {
        let a: Vec<u8> = clipped[y * w..(y + 1) * w].to_vec();
        let b: Vec<u8> = big[y * 24 + 8..y * 24 + 16].to_vec();
        println!("row {y}: clipped {a:?}\n        crop    {b:?}");
        for (p, q) in a.iter().zip(&b) {
            worst = worst.max(p.abs_diff(*q));
        }
    }
    assert!(worst <= 2, "pixels differ by up to {worst}");
}

/// Hoefler Text's 'ģ' is a 'g' with a turned comma whose header box is too low: the
/// box has to come from the outline, or the accent is cut.
#[test]
fn hoefler_g_with_turned_comma_is_not_cut() {
    let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Hoefler Text.ttc") else {
        return; // the font is not installed
    };
    let reference = fontdue::Font::from_bytes(&data[..], fontdue::FontSettings::default()).unwrap();
    let font = Rc::new(Font::parse(data).unwrap());
    let c = '\u{0123}'; // ģ
    for size in [24u16, 48] {
        let mut face = TtfFace::new(font.clone(), size);
        let m = face.glyph_metrics(c).unwrap();
        let mut img = GlyphImage::new();
        assert!(face.rasterize(c, &mut img));
        let (rm, _) = reference.rasterize(c, f32::from(size));
        let ref_top = -(rm.ymin + rm.height as i32);
        assert_eq!(m.y, ref_top, "the top of the glyph at size {size}");
        assert_eq!(m.h, rm.height as i32, "its height at size {size}");
        // There is ink in the top row: the accent.
        assert!(img.data[..m.w as usize].iter().any(|&a| a > 0));
    }
}
