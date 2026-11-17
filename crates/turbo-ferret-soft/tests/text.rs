//! Golden-image tests for text: the font manager, glyph cache and painter drawing
//! through `SoftRenderer`. Bless with `BLESS=1 cargo test -p turbo-ferret-soft`.
//!
//! The default font is derived from Segoe UI and is used for tests and the demo only
//! (see `resources/default_font/license.txt`).

mod common;

use common::{PngImages, assert_golden, resources_dir};
use turbo_ferret::font::{FontDesc, FontManager, LoadCtx};
use turbo_ferret::geom::{Color, Point, Size};
use turbo_ferret::render::painter::Painter;
use turbo_ferret::{FsLoader, TbId};
use turbo_ferret_soft::SoftRenderer;

const SEGOE: &str = "default_font/segoe_white_with_shadow.tb.txt";
const SEGOE_14: FontDesc = FontDesc::new(TbId::new("Segoe"), 14);
const SEGOE_28: FontDesc = FontDesc::new(TbId::new("Segoe"), 28);

/// A background light enough to show the baked shadow.
const BG: Color = Color::rgb(78, 96, 128);

#[test]
fn text_default_font() {
    let mut fonts = FontManager::new();
    fonts.add_font("Segoe", SEGOE);
    let mut loader = FsLoader::new(resources_dir());
    let mut images = PngImages;
    for desc in [SEGOE_14, SEGOE_28] {
        assert!(fonts.create_face(desc, &mut LoadCtx::new(&mut loader, &mut images)));
    }

    let size = Size::new(360, 132);
    let mut r = SoftRenderer::new(size);
    r.clear(BG);
    let gold = Color::rgb(255, 204, 64);
    let ice = Color::rgb(120, 220, 255);
    let lines: [(FontDesc, i32, Color, &str); 5] = [
        (SEGOE_14, 4, Color::WHITE, "The quick brown fox jumps over the lazy dog."),
        (SEGOE_14, 22, gold, "0123456789 !?@#%&*()[]{}<>/\\|\"' ÅÄÖ åäö €£ ©"),
        (SEGOE_14, 40, Color::rgba(255, 255, 255, 128), "Half transparent, and a missing glyph: 中"),
        (SEGOE_28, 58, Color::WHITE, "Hello, World!"),
        (SEGOE_28, 92, ice, "Turbo Ferret ÆØ ¿ß?"),
    ];
    fonts.begin_frame();
    let mut p = Painter::new(&mut r, size);
    for (desc, y, color, text) in lines {
        fonts.draw_text(&mut p, desc, Point::new(6, y), color, text);
    }
    // Painter-local coordinates: the translation moves the text.
    p.set_translation(Point::new(250, 58));
    fonts.draw_text(&mut p, SEGOE_14, Point::new(0, 4), gold, "translated");
    p.finish();

    // One glyph page: created by the first line, updated by each later line that
    // brings new glyphs (all but the last).
    let stats = r.stats();
    assert_eq!((stats.bitmaps_created, stats.bitmaps_updated), (1, 4));
    assert_golden("text_default_font", &r);
}
