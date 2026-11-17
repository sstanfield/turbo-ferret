//! TBBF loading against the real font files in `resources/`: metrics from the
//! descriptions, glyph boundaries for every character of `glyph_str`, nearest-size
//! selection and missing glyphs.

use turbo_ferret::font::{FontMetrics, GlyphFormat, GlyphImage, GlyphMetrics, GlyphSource, LoadCtx, TbbfFont};
use turbo_ferret::res::ResNode;
use turbo_ferret::{DecodeError, FsLoader, Image, ImageDecoder, ResourceLoader, Size};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources");
const SEGOE: &str = "default_font/segoe_white_with_shadow.tb.txt";

/// An [`ImageDecoder`] on the `png` crate (dev-dependency), until the built-in
/// decoder lands.
struct PngImages;

impl ImageDecoder for PngImages {
    fn decode(&mut self, bytes: &[u8]) -> Result<Image, DecodeError> {
        decode_png(bytes)
    }
}

fn decode_png(bytes: &[u8]) -> Result<Image, DecodeError> {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|_| DecodeError::UnknownFormat)?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|_| DecodeError::Corrupt("png"))?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(DecodeError::Unsupported("indexed")),
    };
    Ok(Image { size: Size::new(info.width as i32, info.height as i32), rgba })
}

fn open(path: &str, size: u16) -> TbbfFont {
    let mut loader = FsLoader::new(ROOT);
    let data = loader.load(path).unwrap();
    let mut images = PngImages;
    let mut cx = LoadCtx { loader: &mut loader, images: &mut images, path };
    TbbfFont::load(path, &data, size, &mut cx).unwrap_or_else(|| panic!("{path} at {size} failed to load"))
}

fn glyph_str(path: &str) -> String {
    let node = ResNode::load(&mut FsLoader::new(ROOT), path, None).unwrap();
    node.str("info>glyph_str", "").into_owned()
}

/// Glyph columns found independently of the port: runs of columns with any alpha,
/// in order, one per character.
fn scan_strip(png_path: &str) -> Vec<(i32, i32)> {
    let img = decode_png(&std::fs::read(format!("{ROOT}/{png_path}")).unwrap()).unwrap();
    let (w, h) = (img.size.w as usize, img.size.h as usize);
    let inked: Vec<bool> = (0..w).map(|x| (0..h).any(|y| img.rgba[(y * w + x) * 4 + 3] != 0)).collect();
    let mut runs = Vec::new();
    let mut x = 0;
    while x < w {
        if inked[x] {
            let start = x;
            while x < w && inked[x] {
                x += 1;
            }
            runs.push((start as i32, (x - start) as i32));
        }
        x += 1;
    }
    runs
}

#[test]
fn segoe_metrics_match_the_description() {
    let f14 = open(SEGOE, 14);
    assert_eq!(f14.size(), 14);
    assert_eq!(f14.metrics(), FontMetrics { ascent: 15, descent: 5, height: 20 });
    assert_eq!((f14.x_ofs(), f14.advance_delta(), f14.space_advance()), (0, -3, 5));
    assert_eq!(f14.strip_size(), Size::new(3087, 21));
    assert!(!f14.is_color());

    let f28 = open(SEGOE, 28);
    assert_eq!(f28.size(), 28);
    assert_eq!(f28.metrics(), FontMetrics { ascent: 31, descent: 10, height: 41 });
    assert_eq!((f28.x_ofs(), f28.advance_delta(), f28.space_advance()), (0, -7, 10));
    assert_eq!(f28.strip_size(), Size::new(6174, 42));
}

#[test]
fn segoe_every_glyph_is_found() {
    let chars: Vec<char> = glyph_str(SEGOE).chars().collect();
    assert_eq!(chars.len(), 215);
    assert!(chars.contains(&'\\') && chars.contains(&'"') && chars.contains(&'ÿ'));

    for (size, png) in
        [(14, "default_font/segoe_white_with_shadow_14.png"), (28, "default_font/segoe_white_with_shadow_28.png")]
    {
        let font = open(SEGOE, size);
        let runs = scan_strip(png);
        assert_eq!(font.glyph_count(), chars.len(), "size {size}");
        assert_eq!(runs.len(), chars.len(), "size {size}: one run of inked columns per glyph");
        for (c, run) in chars.iter().zip(&runs) {
            assert_eq!(font.glyph_columns(*c), Some(*run), "size {size}, {c:?}");
        }
    }

    // A few by hand.
    let f14 = open(SEGOE, 14);
    assert_eq!(f14.glyph_columns('!'), Some((5, 7)));
    assert_eq!(f14.glyph_columns('H'), Some((547, 12)));
    assert_eq!(f14.glyph_columns('?'), Some((410, 10)));
    assert_eq!(f14.glyph_columns('ÿ'), Some((3021, 11)));
    assert_eq!(open(SEGOE, 28).glyph_columns('H'), Some((1094, 25)));
}

#[test]
fn segoe_glyph_metrics() {
    let mut f = open(SEGOE, 14);
    // TBBF: advance = width + advance_delta, x = x_ofs, y = -ascent, bitmap = strip height.
    assert_eq!(f.glyph_metrics('H'), Some(GlyphMetrics { advance: 9, x: 0, y: -15, w: 12, h: 21 }));
    assert_eq!(f.glyph_metrics('l'), Some(GlyphMetrics { advance: 4, x: 0, y: -15, w: 7, h: 21 }));
    assert_eq!(f.glyph_metrics(' '), Some(GlyphMetrics { advance: 5, x: 0, y: -15, w: 0, h: 0 }));
    for c in glyph_str(SEGOE).chars() {
        let (_, w) = f.glyph_columns(c).unwrap();
        assert_eq!(f.glyph_metrics(c).unwrap().advance, w - 3, "{c:?}");
    }

    let mut f = open(SEGOE, 28);
    assert_eq!(f.glyph_metrics('H'), Some(GlyphMetrics { advance: 18, x: 0, y: -31, w: 25, h: 42 }));
    assert_eq!(f.glyph_metrics(' ').unwrap().advance, 10);
}

#[test]
fn segoe_missing_glyphs_draw_as_question_marks() {
    let mut f = open(SEGOE, 14);
    let q = f.glyph_metrics('?');
    for c in ['中', 'ā', '\u{1F980}', '\t'] {
        assert!(!f.has_glyph(c));
        assert_eq!(f.glyph_metrics(c), q, "{c:?}");
        let (mut a, mut b) = (GlyphImage::new(), GlyphImage::new());
        assert!(f.rasterize(c, &mut a));
        assert!(f.rasterize('?', &mut b));
        assert_eq!(a, b);
    }
    // A space has an advance but nothing to draw.
    assert!(!f.rasterize(' ', &mut GlyphImage::new()));
}

#[test]
fn segoe_glyph_images_are_premultiplied_white_with_shadow() {
    let mut f = open(SEGOE, 14);
    let mut img = GlyphImage::new();
    assert!(f.rasterize('H', &mut img));
    assert_eq!(img.size, Size::new(12, 21));
    assert_eq!(img.format, GlyphFormat::Rgba);
    assert_eq!(img.data.len(), 12 * 21 * 4);
    assert!(!img.color);
    let px: Vec<&[u8]> = img.data.chunks_exact(4).collect();
    assert!(px.iter().all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]), "premultiplied");
    assert!(px.iter().any(|p| *p == [255, 255, 255, 255]), "white body");
    assert!(px.iter().any(|p| p[3] > 64 && p[0] < p[3] / 2), "dark shadow");
}

#[test]
fn nearest_size_is_used_without_scaling() {
    // TB: TBBFRenderer::Load picks the size node closest to the requested size; on a
    // tie the first one in the file wins.
    for (want, got) in [(1, 14), (14, 14), (20, 14), (21, 14), (22, 28), (28, 28), (100, 28)] {
        let f = open(SEGOE, want);
        assert_eq!(f.size(), got, "requested {want}");
        assert_eq!(f.metrics().height, if got == 14 { 20 } else { 41 });
    }
}

#[test]
fn demo_fonts_load() {
    let cases = [
        ("demo_fonts/neon.tb.txt", 42, "demo_fonts/neon_42.png", FontMetrics { ascent: 50, descent: 16, height: 66 }),
        (
            "demo_fonts/orange.tb.txt",
            30,
            "demo_fonts/orange_30.png",
            FontMetrics { ascent: 62, descent: 25, height: 87 },
        ),
        (
            "demo_fonts/orangutang.tb.txt",
            48,
            "demo_fonts/orangutang_48.png",
            FontMetrics { ascent: 51, descent: 15, height: 66 },
        ),
    ];
    for (path, size, png, metrics) in cases {
        let mut f = open(path, size);
        assert_eq!(f.metrics(), metrics, "{path}");
        assert!(f.is_color(), "{path}: rgb 1");
        let chars: Vec<char> = glyph_str(path).chars().collect();
        assert_eq!(f.glyph_count(), chars.len(), "{path}");
        let runs = scan_strip(png);
        for (c, run) in chars.iter().zip(&runs) {
            assert_eq!(f.glyph_columns(*c), Some(*run), "{path}, {c:?}");
        }
        let mut img = GlyphImage::new();
        assert!(f.rasterize('A', &mut img));
        assert!(img.color);
        let m = f.glyph_metrics('A').unwrap();
        assert_eq!((m.x, m.w, m.h), (f.x_ofs(), img.size.w, img.size.h));
    }
}
