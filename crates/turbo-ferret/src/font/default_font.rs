//! The built-in font (feature `default-font`): DejaVu Sans as bitmaps, so a `Ui`
//! shows readable text before any font is loaded.
//!
//! The files in `default/` are made from `resources/DejaVuSans.ttf` by
//! `turbo-ferret-fontgen` (printable ASCII and Latin-1 at 14, 21 and 28 pixels: the
//! default size at 96, 144 and 192 DPI). A face of another size uses the nearest
//! of the three, as every bitmap font does.
//!
//! DejaVu Sans has a licence of its own (`default/LICENSE-DejaVu.txt`, Bitstream
//! Vera's with additions): its notices have to go with every copy of the font,
//! these bitmaps included. An application shipped with this feature on should
//! carry them. Turn the feature off to leave the font out.

use crate::id::TbId;
use crate::image::PngDecoder;
use crate::services::MemLoader;

use super::backend::LoadCtx;
use super::tbbf::TbbfFont;

/// The name the built-in font is known by (`font: name: "DejaVu Sans"` in a
/// resource). A font an application registers under this name takes its place.
pub const DEFAULT_FONT_NAME: &str = "DejaVu Sans";

/// The id of the built-in font, for a `FontDesc`.
pub const DEFAULT_FONT: TbId = TbId::new(DEFAULT_FONT_NAME);

/// The sizes the built-in font has bitmaps for.
pub const DEFAULT_FONT_SIZES: [u16; 3] = [14, 21, 28];

const DESCRIPTION: &[u8] = include_bytes!("default/dejavu_sans.tb.txt");

/// The licence of the built-in font, to show or ship with an application.
pub const DEFAULT_FONT_LICENSE: &str = include_str!("default/LICENSE-DejaVu.txt");

/// The built-in font at the size nearest to `size`.
pub fn default_font(size: u16) -> Option<TbbfFont> {
    let mut loader = MemLoader::new()
        .with("dejavu_sans_14.png", include_bytes!("default/dejavu_sans_14.png"))
        .with("dejavu_sans_21.png", include_bytes!("default/dejavu_sans_21.png"))
        .with("dejavu_sans_28.png", include_bytes!("default/dejavu_sans_28.png"));
    let mut images = PngDecoder::new();
    let mut cx = LoadCtx::new(&mut loader, &mut images);
    TbbfFont::load("dejavu_sans.tb.txt", DESCRIPTION, size, &mut cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::GlyphSource;

    #[test]
    fn every_size_loads_with_its_glyphs() {
        for (asked, got) in [(14u16, 14u16), (21, 21), (28, 28), (10, 14), (18, 21), (17, 14), (60, 28)] {
            let mut font = default_font(asked).expect("the built-in font loads");
            assert_eq!(font.size(), got, "asked for {asked}");
            assert!(font.has_glyph_table());
            for c in (' '..='~').chain('\u{a0}'..='\u{ff}') {
                assert!(font.has_glyph(c), "{c:?}");
            }
            assert!(font.glyph_metrics('A').is_some_and(|m| m.advance > 0 && m.w > 0));
            assert!(font.metrics().height >= i32::from(got));
        }
    }

    #[test]
    fn the_licence_comes_along() {
        assert!(DEFAULT_FONT_LICENSE.contains("DejaVu Sans"));
        assert!(DEFAULT_FONT_LICENSE.contains("Bitstream Vera Fonts Copyright"));
        assert!(DEFAULT_FONT_LICENSE.contains("Arev Fonts Copyright"));
    }
}
