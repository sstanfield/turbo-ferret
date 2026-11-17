//! Regressions found in review (2026-11): fonts without a space, file names the
//! description cannot hold, hostile numbers in a description, blur on bitmap fonts,
//! and sizes too large to draw.

use turbo_ferret::font::{FontDesc, LoadCtx, TbbfFont};
use turbo_ferret::geom::{Color, Size};
use turbo_ferret::image::PngDecoder;
use turbo_ferret::*;
use turbo_ferret_fontgen::{Error, Options, generate, parse_chars};
use turbo_ferret_soft::SoftRenderer;
use turbo_ferret_ttf::TtfBackend;

fn vera() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/vera.ttf")).unwrap()
}

fn ui_with(baked: Option<&turbo_ferret_fontgen::Output>, stem: &str, size: u16) -> Ui {
    let mut loader = MemLoader::new();
    if let Some(output) = baked {
        for (name, data) in output.files(stem) {
            loader.add_owned(&format!("fonts/{name}"), data);
        }
    }
    loader.add_owned("fonts/vera.ttf", vera());
    let mut ui = Ui::new(Config::default(), Services::with_loader(loader));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(300, 60));
    if baked.is_some() {
        ui.add_font("Vera", &format!("fonts/{stem}.tb.txt"));
    } else {
        ui.fonts_mut().add_backend(Box::new(TtfBackend::new()));
        ui.add_font("Vera", "fonts/vera.ttf");
    }
    assert!(ui.create_font_face(FontDesc::new(tbid!("Vera"), size)));
    ui.set_default_font(FontDesc::new(tbid!("Vera"), size));
    ui
}

/// A font generated for digits only (a clock, a counter) still has a space.
#[test]
fn chars_without_a_space() {
    for spec in ["48-57", "48-57,63"] {
        let chars = parse_chars(spec).unwrap();
        let output = generate(&vera(), &Options { stem: "vera".into(), sizes: vec![14], chars }).unwrap();
        let live = ui_with(None, "vera", 14);
        let baked = ui_with(Some(&output), "vera", 14);
        let fd = FontDesc::new(tbid!("Vera"), 14);
        let width = |ui: &Ui, text: &str| ui.fonts().face(fd).string_width(text);
        assert_eq!(width(&live, "12 34"), width(&baked, "12 34"), "chars {spec}");
        let m = baked.fonts().face(fd).glyph_metrics(' ').unwrap();
        assert!(m.advance > 0 && m.w == 0 && m.h == 0, "{m:?}");
    }
}

/// A glyph table that leaves the space out (written by hand) uses `space_advance`,
/// not the question mark.
#[test]
fn a_table_without_a_space_uses_space_advance() {
    let output = generate(&vera(), &Options { stem: "vera".into(), sizes: vec![14], chars: vec!['A', '?'] }).unwrap();
    let mut out = output.clone();
    out.description = "size 14\n\tbitmap vera_14.png\n\tascent 13\n\tdescent 3\n\tspace_advance 3\n\tglyphs\n\
                       \t\tU+0041 0 0 2 2 0 -2 6\n\t\tU+003F 0 0 2 2 0 -2 6\n"
        .into();
    let ui = ui_with(Some(&out), "vera", 14);
    let face = ui.fonts().face(FontDesc::new(tbid!("Vera"), 14));
    let m = face.glyph_metrics(' ').unwrap();
    assert_eq!((m.advance, m.w, m.h), (3, 0, 0));
    assert_eq!(face.string_width("A A"), 6 + 3 + 6);
}

/// The stem ends up in the description as the bitmap's file name, which has to read
/// back as a string: a stem that would read as a list of numbers is refused.
#[test]
fn stems_the_description_reads_back() {
    for stem in ["vera", "7seg", "my font", "7 seg", "2024", "1.5", "a,b", "-1 x", "", " x"] {
        let r = generate(&vera(), &Options { stem: stem.into(), sizes: vec![14], chars: vec!['A', ' '] });
        let output = match r {
            Ok(output) => output,
            Err(e) => {
                assert_eq!(e, Error::BadStem(stem.into()));
                assert!(["7 seg", "-1 x", "", " x"].contains(&stem), "{stem:?} was refused");
                continue;
            }
        };
        let mut loader = MemLoader::new();
        for (name, data) in output.files(stem) {
            loader.add_owned(&format!("fonts/{name}"), data);
        }
        let mut images = PngDecoder::new();
        let mut cx = LoadCtx::new(&mut loader, &mut images);
        let path = format!("fonts/{stem}.tb.txt");
        assert!(TbbfFont::load(&path, output.description.as_bytes(), 14, &mut cx).is_some(), "stem {stem:?} loads");
    }
}

/// Huge numbers in a description are clamped: measuring and drawing do not overflow.
#[test]
fn huge_numbers_in_a_description() {
    let output =
        generate(&vera(), &Options { stem: "vera".into(), sizes: vec![14], chars: vec!['A', ' ', 'B'] }).unwrap();
    let draw = |description: String| {
        let mut out = output.clone();
        out.description = description;
        let mut ui = ui_with(Some(&out), "vera", 14);
        let root = ui.root();
        let ids = ui.inflate(root, "TBTextField: text: \"AAB\"\n").unwrap();
        ui.set_rect(ids[0], Rect::new(8, 8, 200, 40));
        ui.update();
        let mut r = SoftRenderer::new(Size::new(300, 60));
        r.clear(Color::rgb(0, 0, 0));
        ui.paint(&mut r);
    };
    for line_a in [
        "U+0041 0 0 1 1 1 0 2147483647",
        "U+0041 0 0 1 1 2147483647 0 5",
        "U+0041 0 0 1 1 0 2147483647 5",
        "U+0041 0 0 1 1 0 -2147483648 5",
        "U+0041 0 0 1 1 -2147483648 -2147483648 -2147483648",
    ] {
        draw(format!(
            "size 14\n\tbitmap vera_14.png\n\tascent 13\n\tdescent 3\n\tglyphs\n\t\t{line_a}\n\t\tU+0042 0 0 1 1 1 0 5\n"
        ));
    }
    for lines in [
        "\tascent -2147483648\n\tdescent 3\n",
        "\tascent 2147483647\n\tdescent 3\n",
        "\tascent 2147483647\n\tdescent 2147483647\n",
        "\tascent 13\n\tdescent 3\n\theight 2147483647\n",
        "\tascent 13\n\tdescent 3\n\tx_ofs 2147483647\n\tadvance_delta -2147483648\n\tspace_advance 2147483647\n",
    ] {
        for glyphs in ["\tglyphs\n\t\tU+0041 0 0 1 1 0 0 5\n", ""] {
            draw(format!("info\n\tglyph_str A\nsize 14\n\tbitmap vera_14.png\n{lines}{glyphs}"));
        }
    }
}

/// A radius above the limit, set again and again (every frame, say), keeps the
/// glyphs that are rendered.
#[test]
fn blur_radius_above_the_limit_set_twice() {
    let mut ui = ui_with(None, "vera", 14);
    let fd = FontDesc::new(tbid!("Vera"), 14);
    for radius in [10u8, 100] {
        assert!(ui.fonts_mut().set_blur_radius(fd, radius));
        assert!(ui.fonts_mut().render_glyphs(fd, "Hello"));
        let before = ui.fonts().glyph_cache().len();
        assert!(before >= 4);
        assert!(ui.fonts_mut().set_blur_radius(fd, radius));
        assert_eq!(ui.fonts().glyph_cache().len(), before, "radius {radius}: the glyphs stay");
    }
    let pages = ui.fonts().glyph_cache().page_count();
    for _ in 0..200 {
        ui.fonts_mut().set_blur_radius(fd, 100);
        ui.fonts_mut().render_glyphs(fd, "Hello");
    }
    assert_eq!(ui.fonts().glyph_cache().page_count(), pages);
}

/// A font made by the tool blurs like the TrueType font it was made from.
#[test]
fn blur_on_a_baked_font() {
    let output =
        generate(&vera(), &Options { stem: "vera".into(), sizes: vec![14], chars: "Glow ".chars().collect() }).unwrap();
    let render = |baked: bool| {
        let mut ui = ui_with(baked.then_some(&output), "vera", 14);
        let fd = FontDesc::new(tbid!("Vera"), 14);
        assert!(ui.fonts_mut().set_blur_radius(fd, 3));
        ui.fonts_mut().render_glyphs(fd, "G");
        let metrics = ui.fonts().glyph_cache().glyph(fd, 'G').unwrap().metrics;
        let root = ui.root();
        let ids = ui.inflate(root, "TBTextField: text: \"Glow\"\n").unwrap();
        ui.set_rect(ids[0], Rect::new(20, 10, 200, 40));
        ui.update();
        let mut r = SoftRenderer::new(Size::new(300, 60));
        r.clear(Color::rgb(40, 44, 52));
        ui.paint(&mut r);
        (metrics, r.pixels().to_vec())
    };
    let (live_metrics, live) = render(false);
    let (baked_metrics, baked) = render(true);
    assert!(live_metrics.x < 0 && live_metrics.w > 12, "the glyph grew by the radius: {live_metrics:?}");
    assert_eq!(live_metrics, baked_metrics);
    let inked = live.chunks_exact(4).filter(|p| p[..3] != [40, 44, 52]).count();
    assert!(inked > 300, "the blurred text was drawn: {inked} pixels");
    assert!(live == baked, "the blurred frames differ");
}

/// A size whose glyphs the core cannot draw is refused, not written.
#[test]
fn sizes_too_large_to_draw_are_refused() {
    let gen_size =
        |size: u16| generate(&vera(), &Options { stem: "vera".into(), sizes: vec![size], chars: vec!['W', 'i', ' '] });
    assert!(gen_size(300).is_ok());
    // 'W' is wider than a glyph cache page (512 pixels) here.
    assert_eq!(gen_size(600), Err(Error::GlyphTooLarge(600, 'W')));
    // And too large to rasterize at all here.
    assert_eq!(gen_size(3000), Err(Error::GlyphTooLarge(3000, 'W')));
}
