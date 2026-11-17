//! A font made by the tool draws exactly what the TrueType backend draws.

use std::rc::Rc;

use turbo_ferret::font::{FontDesc, GlyphFormat, GlyphImage, GlyphSource, LoadCtx, TbbfFont};
use turbo_ferret::geom::{Color, Size};
use turbo_ferret::image::PngDecoder;
use turbo_ferret::*;
use turbo_ferret_fontgen::{Error, Options, generate, parse_chars};
use turbo_ferret_soft::SoftRenderer;
use turbo_ferret_ttf::{Font, TtfBackend, TtfFace};

fn vera() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/vera.ttf")).unwrap()
}

fn options(sizes: &[u16]) -> Options {
    Options { stem: "vera".into(), sizes: sizes.to_vec(), chars: Options::default_chars() }
}

/// The generated files in a loader, under `fonts/`.
fn loader_with(output: &turbo_ferret_fontgen::Output) -> MemLoader {
    let mut loader = MemLoader::new();
    for (name, data) in output.files("vera") {
        loader.add_owned(&format!("fonts/{name}"), data);
    }
    loader
}

#[test]
fn every_glyph_is_the_same_as_the_rasterizer_draws() {
    let ttf = vera();
    let output = generate(&ttf, &options(&[11, 14, 28])).unwrap();
    assert!(output.missing.is_empty());
    assert_eq!(output.bitmaps.len(), 3);
    let mut loader = loader_with(&output);
    let mut images = PngDecoder::new();
    let font = Rc::new(Font::parse(ttf).unwrap());

    for size in [11u16, 14, 28] {
        let data = output.description.clone().into_bytes();
        let mut cx = LoadCtx::new(&mut loader, &mut images);
        let mut baked = TbbfFont::load("fonts/vera.tb.txt", &data, size, &mut cx).expect("the bitmap font loads");
        assert!(baked.has_glyph_table());
        assert_eq!(baked.size(), size);
        let mut live = TtfFace::new(font.clone(), size);
        assert_eq!(baked.metrics(), live.metrics());
        assert_eq!(baked.glyph_count(), Options::default_chars().len());

        for c in Options::default_chars() {
            assert_eq!(baked.glyph_metrics(c), live.glyph_metrics(c), "{c:?} at {size}");
            let (mut a, mut b) = (GlyphImage::new(), GlyphImage::new());
            let (drew_a, drew_b) = (baked.rasterize(c, &mut a), live.rasterize(c, &mut b));
            assert_eq!(drew_a, drew_b, "{c:?} at {size}");
            if drew_a {
                assert_eq!(a.size, b.size);
                assert_eq!(a.format, GlyphFormat::Rgba);
                assert!(!a.color, "it takes the text color");
                // White with the coverage as alpha, premultiplied: four times the coverage.
                let want: Vec<u8> = b.data.iter().flat_map(|&v| [v, v, v, v]).collect();
                assert_eq!(a.data, want, "{c:?} at {size}");
            }
        }
        // A character that was not included draws as '?', as in any bitmap font.
        assert_eq!(baked.glyph_metrics('Ω'), baked.glyph_metrics('?'));
    }
}

#[test]
fn a_ui_looks_the_same_with_either_font() {
    const SRC: &str = "TBLayout: axis: y\n\
        \tTBTextField: text: \"The quick brown fox jumps over the lazy dog.\"\n\
        \tTBButton: text: \"Résumé & naïve café (100%)\"\n\
        \tTBTextField: text: \"Larger: Ágætis byrjun\"\n\
        \t\tfont: size: 28\n";
    let ttf = vera();
    let output = generate(&ttf, &options(&[14, 28])).unwrap();
    let size = Size::new(420, 120);
    let render = |baked: bool| -> Vec<u8> {
        let mut loader = loader_with(&output);
        loader.add_owned("fonts/vera.ttf", ttf.clone());
        let mut ui = Ui::new(Config::default(), Services::with_loader(loader));
        ui.services_mut().log = None;
        ui.set_viewport(size);
        if baked {
            ui.add_font("Vera", "fonts/vera.tb.txt");
        } else {
            ui.fonts_mut().add_backend(Box::new(TtfBackend::new()));
            ui.add_font("Vera", "fonts/vera.ttf");
        }
        assert!(ui.create_font_face(FontDesc::new(tbid!("Vera"), 28)));
        assert!(ui.set_default_font(FontDesc::new(tbid!("Vera"), 14)));
        let root = ui.root();
        let ids = ui.inflate(root, SRC).unwrap();
        ui.set_rect(ids[0], Rect::new(8, 8, 404, 104));
        ui.update();
        let mut r = SoftRenderer::new(size);
        r.clear(Color::rgb(40, 44, 52));
        ui.paint(&mut r);
        r.pixels().to_vec()
    };
    let (live, baked) = (render(false), render(true));
    let inked = live.chunks_exact(4).filter(|p| p[..3] != [40, 44, 52]).count();
    assert!(inked > 2000, "text was drawn: {inked} pixels");
    assert!(live == baked, "the frames differ");
}

#[test]
fn bitmaps_are_packed_without_overlap() {
    let output = generate(&vera(), &options(&[20])).unwrap();
    let b = &output.bitmaps[0];
    assert_eq!(b.file_name, "vera_20.png");
    assert!(b.width.is_power_of_two() && b.width <= 512 && b.height <= 512, "{}x{}", b.width, b.height);
    // Read the table back and check that no two glyph rects touch.
    let mut rects = Vec::new();
    let table = output.description.split("\tglyphs\n").nth(1).unwrap();
    for line in table.lines() {
        let n: Vec<i32> = line.split_whitespace().skip(1).map(|v| v.parse().unwrap()).collect();
        assert_eq!(n.len(), 7, "{line}");
        if n[2] > 0 && n[3] > 0 {
            assert!(n[0] + n[2] <= b.width as i32 && n[1] + n[3] <= b.height as i32);
            rects.push(Rect::new(n[0], n[1], n[2] + 1, n[3] + 1));
        }
    }
    assert_eq!(rects.len(), Options::default_chars().len() - 2, "all but the two spaces have an image");
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            assert!(!a.intersects(b), "{a:?} and {b:?} overlap or touch");
        }
    }
    // The PNG decodes to the same coverage.
    let image = PngDecoder::new().decode(&b.to_png()).unwrap();
    assert_eq!(image.size, Size::new(b.width as i32, b.height as i32));
    assert!(image.rgba.chunks_exact(4).zip(&b.alpha).all(|(p, &a)| p == [255, 255, 255, a]));
}

#[test]
fn characters_the_font_lacks_are_reported_and_left_out() {
    let mut opts = options(&[14]);
    opts.chars = "Ab\u{e000}A\u{10ffff}".chars().collect();
    let output = generate(&vera(), &opts).unwrap();
    assert_eq!(output.missing, ['\u{e000}', '\u{10ffff}']);
    let glyphs: Vec<&str> = output.description.lines().filter(|l| l.starts_with("\t\tU+")).collect();
    assert_eq!(glyphs.len(), 2, "A and b, once each: {glyphs:?}");
    assert!(glyphs[0].starts_with("\t\tU+0041 ") && glyphs[1].starts_with("\t\tU+0062 "));
}

#[test]
fn errors() {
    assert_eq!(generate(b"not a font", &options(&[14])), Err(Error::NotAFont));
    assert_eq!(generate(&vera(), &options(&[])), Err(Error::NothingToDo));
    let mut opts = options(&[14]);
    opts.chars = vec!['\u{e000}'];
    assert_eq!(generate(&vera(), &opts), Err(Error::NothingToDo));
    // Thousands of large glyphs do not fit.
    let mut opts = options(&[400]);
    opts.chars = ('\u{20}'..='\u{2ff}').collect();
    assert_eq!(generate(&vera(), &opts), Err(Error::TooLarge(400)));
}

#[test]
fn character_lists() {
    assert_eq!(parse_chars("65").unwrap(), ['A']);
    assert_eq!(parse_chars("U+41-U+43, 0x61 ,48-50").unwrap(), ['A', 'B', 'C', 'a', '0', '1', '2']);
    assert_eq!(parse_chars("").unwrap(), []);
    assert_eq!(parse_chars("32-126,U+A0-U+FF").unwrap(), Options::default_chars());
    assert!(parse_chars("x").is_err());
    assert!(parse_chars("70-60").is_err());
    // Surrogates are not characters.
    assert_eq!(parse_chars("U+D7FF-U+E000").unwrap(), ['\u{d7ff}', '\u{e000}']);
}

/// `resources/vera_font` is this tool's output for `resources/vera.ttf`. If the
/// rasterizer or the tool changes what it writes, generate the files again:
/// `cargo run -p turbo-ferret-fontgen -- resources/vera.ttf --sizes 14,21,28 --out resources/vera_font/vera`
#[test]
fn the_font_in_resources_is_up_to_date() {
    let output = generate(&vera(), &options(&[14, 21, 28])).unwrap();
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/vera_font");
    for (name, data) in output.files("vera") {
        let on_disk = std::fs::read(format!("{dir}/{name}")).unwrap_or_default();
        if name.ends_with(".png") {
            // Compare the pixels: the PNG encoder may compress differently one day.
            let (a, b) = (PngDecoder::new().decode(&on_disk).unwrap(), PngDecoder::new().decode(&data).unwrap());
            assert!(a.size == b.size && a.rgba == b.rgba, "{name} is out of date");
        } else {
            assert!(on_disk == data, "{name} is out of date");
        }
    }
}
