//! The parser and rasterizer on small fonts built in the test, where every pixel
//! can be worked out by hand, and on damaged files.

use std::rc::Rc;

use turbo_ferret::font::{FontDesc, FontManager, GlyphImage, GlyphMetrics, GlyphSource, LoadCtx};
use turbo_ferret::services::NoImages;
use turbo_ferret::{MemLoader, TbId};
use turbo_ferret_ttf::{Font, OutlineSink, TtfBackend, TtfFace};

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
/// only, 7 'G': 'A' at half size moved right, plus 'B', 8 ' ': nothing, 9 'W': 'A'
/// moved up, with a wrong box in its header.
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
        // 'A' moved up by four pixels, with a header box that does not know it.
        Glyph { advance: 500, parts: vec![(1, 0, 512, None)], bounds: Some((0, 0, 1024, 1024)), ..Default::default() },
    ];
    let mut map: Vec<(u32, u16)> = "ABCDEFG".chars().zip(1u16..).map(|(c, g)| (c as u32, g)).collect();
    map.push((' ' as u32, 8));
    map.push(('W' as u32, 9));
    map.push((0x20AC, 2)); // the euro sign is 'B' too
    if opts.cmap12 {
        map.push((0x1F600, 3));
    }
    build(&glyphs, &map, opts)
}

fn face(opts: Opts, size: u16) -> TtfFace {
    TtfFace::new(Rc::new(Font::parse(test_font(opts)).expect("the test font parses")), size)
}

fn image(face: &mut TtfFace, c: char) -> (GlyphMetrics, Vec<u8>) {
    let m = face.glyph_metrics(c).unwrap();
    let mut img = GlyphImage::new();
    let drew = face.rasterize(c, &mut img);
    assert_eq!(drew, m.w > 0 && m.h > 0);
    assert_eq!(img.data.len(), (m.w * m.h) as usize);
    assert_eq!((img.size.w, img.size.h), (m.w, m.h));
    (m, img.data)
}

fn area(data: &[u8]) -> f32 {
    data.iter().map(|&a| f32::from(a)).sum::<f32>() / 255.0
}

#[test]
fn face_metrics_scale_with_the_size() {
    let f = face(Opts::default(), 16);
    let m = f.metrics();
    // 1600, -400 and 100 units at 16 / 2048 pixels per unit.
    assert_eq!((m.ascent, m.descent, m.height), (13, 3, 16));
    let m = face(Opts::default(), 64).metrics();
    assert_eq!((m.ascent, m.descent, m.height), (50, 13, 66));
}

#[test]
fn a_square_on_whole_pixels_is_solid() {
    let mut f = face(Opts::default(), 16);
    let (m, data) = image(&mut f, 'A');
    assert_eq!(m, GlyphMetrics { advance: 9, x: 0, y: -8, w: 8, h: 8 });
    assert!(data.iter().all(|&a| a == 255), "{data:?}");
    // Twice the size: twice the pixels.
    let (m, data) = image(&mut face(Opts::default(), 32), 'A');
    assert_eq!(m, GlyphMetrics { advance: 19, x: 0, y: -16, w: 16, h: 16 });
    assert!(data.iter().all(|&a| a == 255));
}

#[test]
fn edges_on_half_pixels_are_half_covered() {
    let mut f = face(Opts::default(), 16);
    let (m, data) = image(&mut f, 'B');
    // x from 1.5 to 6.5, y (down) from -7.5 to -0.5.
    assert_eq!(m, GlyphMetrics { advance: 9, x: 1, y: -8, w: 6, h: 8 });
    for row in 0..8 {
        for col in 0..6 {
            let edge_x = col == 0 || col == 5;
            let edge_y = row == 0 || row == 7;
            let want = match (edge_x, edge_y) {
                (false, false) => 255,
                (true, true) => 64,
                _ => 128,
            };
            assert_eq!(data[row * 6 + col], want, "pixel {col},{row}");
        }
    }
    assert!((area(&data) - 5.0 * 7.0).abs() < 0.2);
}

#[test]
fn a_triangle_covers_half_its_box() {
    let mut f = face(Opts::default(), 16);
    let (m, data) = image(&mut f, 'C');
    assert_eq!((m.x, m.y, m.w, m.h), (0, -8, 8, 8));
    assert!((area(&data) - 32.0).abs() < 0.1, "{}", area(&data));
    // The corner at the origin is inside, the opposite one outside, and the pixels
    // the long edge cuts through the middle are half covered.
    assert_eq!(data[7 * 8], 255);
    assert_eq!(data[7], 0);
    for i in 0..8 {
        assert_eq!(data[i * 8 + i], 128, "pixel {i},{i}");
    }
}

#[test]
fn winding_decides_what_is_inside() {
    let mut f = face(Opts::default(), 16);
    // A contour wound the other way cuts a hole.
    let (_, hole) = image(&mut f, 'D');
    assert_eq!(hole[4 * 8 + 4], 0);
    assert_eq!(hole[0], 255);
    assert!((area(&hole) - (64.0 - 16.0)).abs() < 0.1);
    // Wound the same way it stays filled (non-zero winding).
    let (_, solid) = image(&mut f, 'E');
    assert!(solid.iter().all(|&a| a == 255));
}

#[test]
fn a_contour_of_off_curve_points_is_round() {
    let mut f = face(Opts::default(), 16);
    let (m, data) = image(&mut f, 'F');
    assert_eq!((m.x, m.y, m.w, m.h), (0, -8, 8, 8));
    // Four quadratic arcs with their control points on the corners of an 8 pixel
    // square: each quadrant is a triangle of r²/2 plus a segment of r²/3, r = 4.
    let want = 4.0 * (16.0 / 2.0 + 16.0 / 3.0);
    assert!((area(&data) - want).abs() < 0.25, "{} vs {want}", area(&data));
    // The corners are outside and the middle is inside.
    assert_eq!(data[0], 0);
    assert_eq!(data[63], 0);
    assert_eq!(data[3 * 8 + 3], 255);
    // It is symmetric.
    for row in 0..8 {
        for col in 0..8 {
            let a = data[row * 8 + col];
            assert!(a.abs_diff(data[row * 8 + 7 - col]) <= 1 && a.abs_diff(data[(7 - row) * 8 + col]) <= 1);
        }
    }
}

#[test]
fn a_composite_glyph_places_its_parts() {
    let mut f = face(Opts::default(), 16);
    let (m, data) = image(&mut f, 'G');
    // 'B' (1.5..6.5 by 0.5..7.5) and 'A' at half size, 8 pixels to the right (8..12 by 0..4).
    assert_eq!(m, GlyphMetrics { advance: 12, x: 1, y: -8, w: 11, h: 8 });
    let at = |x: i32, y: i32| data[((y + 8) * 11 + (x - 1)) as usize];
    assert_eq!(at(3, -4), 255, "inside 'B'");
    assert_eq!(at(1, -4), 128, "the left edge of 'B'");
    assert_eq!(at(7, -4), 0, "between the parts");
    assert_eq!(at(8, -4), 255, "the small 'A'");
    assert_eq!(at(11, -1), 255);
    assert_eq!(at(9, -5), 0, "above the small 'A'");
    assert!((area(&data) - (35.0 + 16.0)).abs() < 0.2);
}

/// The box of a glyph comes from its outline: some real fonts have composites whose
/// header box is too small, and such glyphs were cut.
#[test]
fn a_wrong_header_box_does_not_cut_the_glyph() {
    let font = Font::parse(test_font(Opts::default())).unwrap();
    let header = font.glyph_bounds(9).unwrap();
    assert_eq!((header.y_min, header.y_max), (0, 1024), "what the file claims");
    let mut f = face(Opts::default(), 16);
    let (m, data) = image(&mut f, 'W');
    // The square is four pixels above the baseline: 4 to 12 up.
    assert_eq!(m, GlyphMetrics { advance: 4, x: 0, y: -12, w: 8, h: 8 });
    assert!(data.iter().all(|&a| a == 255), "all of it is drawn");
}

/// A curve's box is where the curve goes, not where its control points are.
#[test]
fn the_box_of_a_round_glyph_is_tight() {
    let mut f = face(Opts::default(), 16);
    // Control points on the corners of an 8 pixel square; the curve touches the
    // middle of each side.
    let (m, _) = image(&mut f, 'F');
    assert_eq!((m.x, m.y, m.w, m.h), (0, -8, 8, 8));
    let (m, _) = image(&mut face(Opts::default(), 160), 'F');
    assert_eq!((m.x, m.y, m.w, m.h), (0, -80, 80, 80));
}

#[test]
fn characters_map_to_glyphs() {
    for opts in [Opts::default(), Opts { cmap12: true, ..Default::default() }] {
        let font = Font::parse(test_font(opts)).unwrap();
        assert_eq!(font.num_glyphs(), 10);
        assert_eq!(font.glyph_index('A'), Some(1));
        assert_eq!(font.glyph_index('G'), Some(7));
        assert_eq!(font.glyph_index(' '), Some(8));
        assert_eq!(font.glyph_index('€'), Some(2));
        assert_eq!(font.glyph_index('H'), None);
        assert_eq!(font.glyph_index('@'), None);
        assert_eq!(font.glyph_index('\u{ffff}'), None);
        assert_eq!(font.glyph_index('😀'), opts.cmap12.then_some(3), "outside the BMP needs format 12");
        assert_eq!(font.advance_width(1), 1200);
        assert_eq!(font.advance_width(8), 500);
        assert_eq!(font.advance_width(200), 500, "past the end: the last advance");
    }
}

#[test]
fn a_space_has_an_advance_and_no_image() {
    let mut f = face(Opts::default(), 16);
    let (m, data) = image(&mut f, ' ');
    assert_eq!(m, GlyphMetrics { advance: 4, ..Default::default() });
    assert!(data.is_empty());
}

#[test]
fn a_missing_character_draws_the_missing_glyph() {
    let mut f = face(Opts::default(), 16);
    assert!(!f.has_glyph('x'));
    let (m, data) = image(&mut f, 'x');
    assert_eq!(m, GlyphMetrics { advance: 5, x: 1, y: -2, w: 2, h: 2 });
    assert!(data.iter().all(|&a| a == 255));
}

#[test]
fn long_offsets_and_collections_read_the_same() {
    let plain = image(&mut face(Opts::default(), 24), 'G');
    for opts in [
        Opts { loca_long: true, ..Default::default() },
        Opts { collection: true, ..Default::default() },
        Opts { cmap12: true, loca_long: true, collection: true },
    ] {
        assert_eq!(image(&mut face(opts, 24), 'G'), plain);
    }
}

#[test]
fn outlines_come_out_in_font_units() {
    #[derive(Default)]
    struct Log(Vec<String>);
    impl OutlineSink for Log {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.push(format!("M {x} {y}"));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.push(format!("L {x} {y}"));
        }
        fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
            self.0.push(format!("Q {cx} {cy} {x} {y}"));
        }
        fn close(&mut self) {
            self.0.push("Z".into());
        }
    }
    let font = Font::parse(test_font(Opts::default())).unwrap();
    let mut log = Log::default();
    assert!(font.outline(3, &mut log));
    assert_eq!(log.0, ["M 0 0", "L 0 1024", "L 1024 0", "Z"]);
    // Off-curve points only: it starts halfway between the last and the first.
    let mut log = Log::default();
    assert!(font.outline(6, &mut log));
    assert_eq!(
        log.0,
        ["M 512 1024", "Q 1024 1024 1024 512", "Q 1024 0 512 0", "Q 0 0 0 512", "Q 0 1024 512 1024", "Z"]
    );
    let mut log = Log::default();
    assert!(!font.outline(8, &mut log), "a space has no outline");
    assert!(!font.outline(99, &mut log), "no such glyph");
    assert!(log.0.is_empty());
}

#[test]
fn other_files_are_refused() {
    assert!(Font::parse(Vec::new()).is_none());
    assert!(Font::parse(b"not a font at all, just some text".to_vec()).is_none());
    // PostScript outlines.
    let mut otf = test_font(Opts::default());
    otf[..4].copy_from_slice(b"OTTO");
    assert!(Font::parse(otf).is_none());
    // A PNG.
    assert!(Font::parse(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec()).is_none());
}

/// Read everything a face reads. Must not panic whatever the file holds.
fn exercise(data: Vec<u8>, chars: &str) {
    let Some(font) = Font::parse(data) else { return };
    let font = Rc::new(font);
    for size in [7u16, 20] {
        let mut face = TtfFace::new(font.clone(), size);
        let _ = face.metrics();
        for c in chars.chars() {
            let m = face.glyph_metrics(c).unwrap();
            let mut img = GlyphImage::new();
            if face.rasterize(c, &mut img) {
                assert_eq!(img.data.len(), (m.w * m.h) as usize);
            }
        }
    }
}

#[test]
fn truncated_files_do_not_panic() {
    for opts in [Opts::default(), Opts { cmap12: true, loca_long: true, collection: true }] {
        let full = test_font(opts);
        for len in 0..full.len() {
            exercise(full[..len].to_vec(), "ABCDEFG €x😀");
        }
    }
}

#[test]
fn damaged_files_do_not_panic() {
    // A small generator, so the test is the same every run.
    let mut state = 0x2545_F491u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for opts in [Opts::default(), Opts { cmap12: true, loca_long: true, collection: true }] {
        let full = test_font(opts);
        for _ in 0..4000 {
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
            exercise(data, "ABCDEFG €x😀");
        }
    }
    // The real font, damaged in its tables' first kilobytes and anywhere.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/DejaVuSans.ttf");
    let full = std::fs::read(path).unwrap();
    for i in 0..300 {
        let mut data = full.clone();
        for _ in 0..1 + next() % 8 {
            let at = if i % 2 == 0 { next() as usize % 4096 } else { next() as usize % data.len() };
            data[at] = next() as u8;
        }
        exercise(data, "Ag€Ç%é");
    }
}

#[test]
fn the_backend_opens_fonts_for_the_font_manager() {
    let mut loader = MemLoader::new();
    loader.add_owned("fonts/test.ttf", test_font(Opts::default()));
    loader.add_owned("fonts/readme.txt", b"hello".to_vec());
    let mut images = NoImages;
    let mut fonts = FontManager::new();
    fonts.add_backend(Box::new(TtfBackend::new()));
    let id = fonts.add_font("Test", "fonts/test.ttf");
    let other = fonts.add_font("Text", "fonts/readme.txt");
    let mut cx = LoadCtx::new(&mut loader, &mut images);
    assert!(fonts.create_face(FontDesc::new(id, 16), &mut cx));
    assert!(fonts.create_face(FontDesc::new(id, 32), &mut cx));
    assert!(!fonts.create_face(FontDesc::new(other, 16), &mut cx), "not a font");

    let small = fonts.face(FontDesc::new(id, 16));
    assert!(!small.is_box_font());
    assert_eq!((small.ascent(), small.descent(), small.height()), (13, 3, 16));
    assert_eq!(small.string_width("AB A"), 9 + 9 + 4 + 9);
    assert_eq!(fonts.face(FontDesc::new(id, 32)).string_width("AB A"), 19 + 17 + 8 + 19);

    // Glyphs go into the glyph cache without a renderer.
    assert!(fonts.render_glyphs(FontDesc::new(id, 16), "ABC G"));
    let g = *fonts.glyph_cache().glyph(FontDesc::new(id, 16), 'A').unwrap();
    assert_eq!(g.metrics, GlyphMetrics { advance: 9, x: 0, y: -8, w: 8, h: 8 });
    assert_eq!(g.frag.unwrap().rect.size(), turbo_ferret::Size::new(8, 8));
    assert!(fonts.glyph_cache().glyph(FontDesc::new(id, 16), ' ').unwrap().frag.is_none());
    assert_eq!(TbId::new("Test"), id);
}
