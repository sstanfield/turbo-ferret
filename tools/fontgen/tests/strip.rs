//! Fonts in Turbo Badger's strip format, made from a TrueType font: the core's
//! loader finds every glyph, the metrics are the TrueType font's, and the glyphs'
//! bodies are where the TrueType backend draws them.

use std::rc::Rc;

use turbo_ferret::font::{GlyphImage, GlyphSource, LoadCtx, TbbfFont};
use turbo_ferret::image::PngDecoder;
use turbo_ferret::*;
use turbo_ferret_fontgen::{Error, Options, StripStyle, generate_strip};
use turbo_ferret_ttf::{Font, TtfFace};

fn dejavu() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/DejaVuSans.ttf")).unwrap()
}

fn options(stem: &str, sizes: &[u16]) -> Options {
    Options { stem: stem.into(), sizes: sizes.to_vec(), chars: Options::default_chars() }
}

fn load(output: &turbo_ferret_fontgen::StripOutput, stem: &str, size: u16) -> TbbfFont {
    let mut loader = MemLoader::new();
    for (name, data) in output.files(stem) {
        loader.add_owned(&format!("fonts/{name}"), data);
    }
    let mut images = PngDecoder::new();
    let mut cx = LoadCtx::new(&mut loader, &mut images);
    TbbfFont::load(&format!("fonts/{stem}.tb.txt"), output.description.as_bytes(), size, &mut cx)
        .expect("the strip font loads")
}

#[test]
fn a_strip_keeps_the_fonts_metrics_and_places_its_glyphs() {
    let ttf = dejavu();
    let font = Rc::new(Font::parse(ttf.clone()).unwrap());
    for (style, sizes) in
        [(StripStyle::SHADOW, vec![14u16, 28]), (StripStyle::GLOW, vec![42]), (StripStyle::CANDY, vec![20])]
    {
        let output = generate_strip(&ttf, &options("test", &sizes), &style).unwrap();
        assert!(output.missing.is_empty());
        assert_eq!(output.strips.len(), sizes.len());
        for &size in &sizes {
            let mut strip = load(&output, "test", size);
            let mut live = TtfFace::new(font.clone(), size);
            assert!(!strip.has_glyph_table(), "the glyphs are found by their clear columns");
            assert_eq!(strip.is_color(), style.color);
            // Every character but the space is a glyph of the strip.
            assert_eq!(strip.glyph_count(), Options::default_chars().len() - 1);
            // The line is higher by the room the halo needs, above and below.
            let pad = -strip.x_ofs().max(-strip.metrics().ascent);
            assert!(pad >= 1);
            assert!(strip.metrics().ascent > live.metrics().ascent && strip.metrics().descent > live.metrics().descent);

            for c in Options::default_chars() {
                let (sm, lm) = (strip.glyph_metrics(c).unwrap(), live.glyph_metrics(c).unwrap());
                assert_eq!(sm.advance, lm.advance, "advance of {c:?} at {size}");
                if c == ' ' {
                    continue;
                }
                assert!(strip.has_glyph(c), "{c:?} at {size}");
                // Where the TrueType glyph is fully covered, the strip has the
                // fill at full alpha, at the same place from the pen.
                let (mut si, mut li) = (GlyphImage::new(), GlyphImage::new());
                assert!(strip.rasterize(c, &mut si));
                if !live.rasterize(c, &mut li) {
                    continue; // no ink (a no-break space)
                }
                let mut solid = 0;
                for y in 0..lm.h {
                    for x in 0..lm.w {
                        if li.data[(y * lm.w + x) as usize] != 255 {
                            continue;
                        }
                        // The pixel's place in the strip's cell.
                        let (cx, cy) = (lm.x + x - sm.x, lm.y + y - sm.y);
                        assert!(cx >= 0 && cy >= 0 && cx < sm.w && cy < sm.h, "{c:?} at {size}: ink outside its cell");
                        let p = &si.data[((cy * sm.w + cx) * 4) as usize..][..4];
                        assert_eq!(p[3], 255, "{c:?} at {size}: pixel {x},{y}");
                        solid += 1;
                    }
                }
                if c == 'H' {
                    assert!(solid > i32::from(size), "'H' has a solid body");
                }
            }
        }
    }
}

#[test]
fn the_shadow_style_is_white_over_a_dark_shadow() {
    let output = generate_strip(&dejavu(), &options("test", &[14]), &StripStyle::SHADOW).unwrap();
    let mut strip = load(&output, "test", 14);
    let mut img = GlyphImage::new();
    assert!(strip.rasterize('H', &mut img));
    let px: Vec<&[u8]> = img.data.chunks_exact(4).collect();
    assert!(px.iter().any(|p| *p == [255, 255, 255, 255]), "a white body");
    assert!(px.iter().any(|p| p[3] > 60 && p[0] < 30), "a dark shadow");
    // Colors only in a color font.
    assert!(px.iter().all(|p| p[0] == p[1] && p[1] == p[2]));
    let output = generate_strip(&dejavu(), &options("test", &[30]), &StripStyle::SUNSET).unwrap();
    let mut strip = load(&output, "test", 30);
    assert!(strip.rasterize('H', &mut img));
    assert!(img.color);
    assert!(img.data.chunks_exact(4).any(|p| p[3] == 255 && p[0] > 200 && p[2] < 150), "a warm fill");
}

#[test]
fn errors() {
    assert_eq!(generate_strip(b"nope", &options("x", &[14]), &StripStyle::SHADOW), Err(Error::NotAFont));
    assert_eq!(generate_strip(&dejavu(), &options("x", &[]), &StripStyle::SHADOW), Err(Error::NothingToDo));
    assert_eq!(
        generate_strip(&dejavu(), &options("7 x", &[14]), &StripStyle::SHADOW),
        Err(Error::BadStem("7 x".into()))
    );
    // A strip of every character at this size would be too wide to decode.
    assert_eq!(generate_strip(&dejavu(), &options("x", &[200]), &StripStyle::SHADOW), Err(Error::TooLarge(200)));
    assert!(StripStyle::by_name("shadow").is_some() && StripStyle::by_name("neon").is_none());
}

/// The fonts in `resources/default_font` and `resources/demo_fonts` are this tool's
/// output for `resources/DejaVuSans.ttf` (the commands are in their license.txt).
#[test]
fn the_fonts_in_resources_are_up_to_date() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources");
    for (dir, stem, style, sizes) in [
        ("default_font", "dejavu_white_with_shadow", StripStyle::SHADOW, vec![14u16, 28]),
        ("demo_fonts", "glow", StripStyle::GLOW, vec![42]),
        ("demo_fonts", "sunset", StripStyle::SUNSET, vec![30]),
        ("demo_fonts", "candy", StripStyle::CANDY, vec![48]),
    ] {
        let output = generate_strip(&dejavu(), &options(stem, &sizes), &style).unwrap();
        for (name, data) in output.files(stem) {
            let on_disk = std::fs::read(format!("{root}/{dir}/{name}")).unwrap_or_default();
            if name.ends_with(".png") {
                let (a, b) = (PngDecoder::new().decode(&on_disk).unwrap(), PngDecoder::new().decode(&data).unwrap());
                assert!(a.size == b.size && a.rgba == b.rgba, "{dir}/{name} is out of date");
            } else {
                assert!(on_disk == data, "{dir}/{name} is out of date");
            }
        }
    }
}
