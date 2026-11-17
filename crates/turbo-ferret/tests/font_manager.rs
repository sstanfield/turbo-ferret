//! FontManager: faces, fallback to the default face and to the box font, string
//! widths, and drawing through the glyph cache (rasterize, upload once, emit quads).

use turbo_ferret::font::{BOX_FONT, BackgroundFont, FontDesc, FontManager, GlyphMetrics, LoadCtx};
use turbo_ferret::render::painter::Painter;
use turbo_ferret::{
    BitmapId, Color, DecodeError, FsLoader, Image, ImageDecoder, Point, Quad, Rect, Renderer, Size, TbId,
};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources");
const SHADOW: &str = "default_font/dejavu_white_with_shadow.tb.txt";
const SHADOW_14: FontDesc = FontDesc::new(TbId::new("DejaVuShadow"), 14);
const SHADOW_28: FontDesc = FontDesc::new(TbId::new("DejaVuShadow"), 28);

/// An [`ImageDecoder`] on the `png` crate (dev-dependency), until the built-in
/// decoder lands.
struct PngImages;

impl ImageDecoder for PngImages {
    fn decode(&mut self, bytes: &[u8]) -> Result<Image, DecodeError> {
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
}

/// Create faces through an `FsLoader` rooted at `resources/`.
fn create(fm: &mut FontManager, desc: FontDesc) -> bool {
    let mut loader = FsLoader::new(ROOT);
    let mut images = PngImages;
    fm.create_face(desc, &mut LoadCtx::new(&mut loader, &mut images))
}

/// A manager with the default font (DejaVu Sans, white with a shadow) registered and
/// created at 14 and 28.
fn shadow_font() -> FontManager {
    shadow_font_with(FontManager::new())
}

fn shadow_font_with(mut fm: FontManager) -> FontManager {
    assert_eq!(fm.add_font("DejaVuShadow", SHADOW), TbId::new("DejaVuShadow"));
    assert!(create(&mut fm, SHADOW_14));
    assert!(create(&mut fm, SHADOW_28));
    fm
}

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Begin,
    End,
    Create(BitmapId, Size),
    Update(BitmapId),
    Destroy(BitmapId),
    Draw(Option<BitmapId>, Vec<Quad>),
}

#[derive(Default)]
struct Rec {
    calls: Vec<Call>,
    next: u32,
}

impl Rec {
    fn draws(&self) -> Vec<(Option<BitmapId>, &[Quad])> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Draw(b, q) => Some((*b, q.as_slice())),
                _ => None,
            })
            .collect()
    }

    fn quads(&self) -> Vec<Quad> {
        self.draws().iter().flat_map(|d| d.1.iter().copied()).collect()
    }

    fn count(&self, f: impl Fn(&Call) -> bool) -> usize {
        self.calls.iter().filter(|c| f(c)).count()
    }

    fn creates(&self) -> usize {
        self.count(|c| matches!(c, Call::Create(..)))
    }

    fn updates(&self) -> usize {
        self.count(|c| matches!(c, Call::Update(..)))
    }

    fn destroys(&self) -> usize {
        self.count(|c| matches!(c, Call::Destroy(..)))
    }
}

impl Renderer for Rec {
    fn begin_frame(&mut self, _: Size) {
        self.calls.push(Call::Begin);
    }
    fn end_frame(&mut self) {
        self.calls.push(Call::End);
    }
    fn create_bitmap(&mut self, size: Size, rgba: &[u8]) -> Option<BitmapId> {
        assert_eq!(rgba.len(), size.w as usize * size.h as usize * 4);
        self.next += 1;
        self.calls.push(Call::Create(BitmapId(self.next), size));
        Some(BitmapId(self.next))
    }
    fn update_bitmap(&mut self, bitmap: BitmapId, _: &[u8]) {
        self.calls.push(Call::Update(bitmap));
    }
    fn destroy_bitmap(&mut self, bitmap: BitmapId) {
        self.calls.push(Call::Destroy(bitmap));
    }
    fn draw(&mut self, bitmap: Option<BitmapId>, _: Rect, quads: &[Quad]) {
        self.calls.push(Call::Draw(bitmap, quads.to_vec()));
    }
}

const VIEW: Size = Size::new(400, 200);

/// Draw each `(desc, pos, color, text)` in one frame.
fn frame(fm: &mut FontManager, r: &mut Rec, texts: &[(FontDesc, Point, Color, &str)]) {
    fm.begin_frame();
    let mut p = Painter::new(r, VIEW);
    for &(desc, pos, color, text) in texts {
        fm.draw_text(&mut p, desc, pos, color, text);
    }
    p.finish();
}

fn sum_advances(fm: &FontManager, desc: FontDesc, text: &str) -> i32 {
    text.chars().map(|c| fm.face(desc).glyph_metrics(c).map_or(0, |m| m.advance)).sum()
}

#[test]
fn box_font_is_the_default() {
    let fm = FontManager::new();
    assert_eq!(fm.default_desc(), BOX_FONT);
    assert_eq!(BOX_FONT, FontDesc::new(TbId::NONE, 16));
    assert!(fm.has_face(BOX_FONT));
    assert!(!fm.has_face(SHADOW_14));
    for desc in [BOX_FONT, SHADOW_14, FontDesc::default(), FontDesc::new(TbId::new("x"), 3)] {
        let face = fm.face(desc);
        assert!(face.is_box_font());
        assert_eq!(face.desc(), BOX_FONT);
        // TB: invented metrics for the test font.
        assert_eq!((face.height(), face.ascent(), face.descent()), (16, 12, 4));
    }
    let face = fm.face(BOX_FONT);
    // Every character is a box a third of the height wide, plus one pixel.
    assert_eq!(face.string_width("abc"), 18);
    assert_eq!(face.string_width("a c\u{FFFF}"), 18);
    assert_eq!(face.string_width(""), 0);
    assert_eq!(face.glyph_metrics('x'), Some(GlyphMetrics { advance: 6, x: 0, y: -12, w: 5, h: 16 }));
}

#[test]
fn box_font_at_other_sizes() {
    let mut fm = FontManager::new();
    let desc = FontDesc::new(TbId::NONE, 24);
    assert!(!fm.has_face(desc));
    assert!(create(&mut fm, desc));
    assert!(fm.has_face(desc));
    let face = fm.face(desc);
    assert!(face.is_box_font());
    assert_eq!(face.desc(), desc);
    assert_eq!((face.height(), face.ascent(), face.descent()), (24, 18, 6));
    assert_eq!(face.string_width("ab"), 18);
}

#[test]
fn create_default_font_faces() {
    let mut fm = shadow_font();
    let info = fm.font_info(TbId::new("DejaVuShadow")).unwrap();
    assert_eq!((info.name(), info.path(), info.id()), ("DejaVuShadow", SHADOW, TbId::new("DejaVuShadow")));
    assert!(fm.has_face(SHADOW_14) && fm.has_face(SHADOW_28));

    let f14 = fm.face(SHADOW_14);
    assert!(!f14.is_box_font());
    assert_eq!(f14.desc(), SHADOW_14);
    assert_eq!((f14.height(), f14.ascent(), f14.descent()), (20, 15, 5));
    let f28 = fm.face(SHADOW_28);
    assert_eq!((f28.height(), f28.ascent(), f28.descent()), (41, 30, 11));

    // Creating again keeps the face.
    assert!(create(&mut fm, SHADOW_14));

    // A size without its own strip uses the nearest one, under its own description.
    let s20 = FontDesc::new(TbId::new("DejaVuShadow"), 20);
    assert!(create(&mut fm, s20));
    assert_eq!(fm.face(s20).desc(), s20);
    assert_eq!(fm.face(s20).height(), 20);
    assert_eq!(fm.face(s20).string_width("Hello"), fm.face(SHADOW_14).string_width("Hello"));
}

#[test]
fn create_face_failures() {
    let mut fm = FontManager::new();
    // Not registered.
    assert!(!create(&mut fm, SHADOW_14));
    // Registered, but the file is missing.
    fm.add_font("Missing", "default_font/nope.tb.txt");
    assert!(!create(&mut fm, FontDesc::new(TbId::new("Missing"), 14)));
    // Not a font any backend opens.
    fm.add_font("Licence", "default_font/license.txt");
    assert!(!create(&mut fm, FontDesc::new(TbId::new("Licence"), 14)));
    fm.add_font("Skin", "default_skin/skin.tb.txt");
    assert!(!create(&mut fm, FontDesc::new(TbId::new("Skin"), 14)));
    // The empty name is the box font.
    assert_eq!(fm.add_font("", SHADOW), TbId::NONE);
    assert!(fm.font_info(TbId::NONE).is_none());
    // Nothing was added.
    assert!(fm.face(FontDesc::new(TbId::new("Skin"), 14)).is_box_font());
}

#[test]
fn create_face_resolves_against_the_context_path() {
    let mut fm = FontManager::new();
    fm.add_font("DejaVuShadow", "dejavu_white_with_shadow.tb.txt");
    let mut loader = FsLoader::new(ROOT);
    let mut images = PngImages;
    let mut cx = LoadCtx { loader: &mut loader, images: &mut images, path: "default_font/fonts.tb.txt" };
    assert!(fm.create_face(SHADOW_14, &mut cx));
    assert_eq!(fm.face(SHADOW_14).height(), 20);
}

#[test]
fn fallback_to_the_default_face() {
    let mut fm = shadow_font();
    let s99 = FontDesc::new(TbId::new("DejaVuShadow"), 99);
    assert!(fm.face(s99).is_box_font());

    fm.set_default(SHADOW_14);
    assert_eq!(fm.default_desc(), SHADOW_14);
    assert_eq!(fm.face(s99).desc(), SHADOW_14);
    assert_eq!(fm.face(FontDesc::new(TbId::new("Unknown"), 28)).desc(), SHADOW_14);
    assert_eq!(fm.face(SHADOW_28).desc(), SHADOW_28);
    assert_eq!(fm.face(BOX_FONT).desc(), BOX_FONT);

    // A default without a face falls back to the box font.
    fm.set_default(s99);
    assert!(fm.face(FontDesc::new(TbId::new("Unknown"), 28)).is_box_font());
}

#[test]
fn string_widths() {
    let fm = shadow_font();
    let f14 = fm.face(SHADOW_14);
    // DejaVu Sans's advances at 14 pixels (the strip keeps them: cell width +
    // advance_delta -6): H 11, e 9, l 4, l 4, o 9.
    assert_eq!(f14.string_width("Hello"), 37);
    assert_eq!(f14.string_width("Hello"), sum_advances(&fm, SHADOW_14, "Hello"));
    // space_advance 4
    assert_eq!(f14.string_width("Hello Hello"), 37 + 4 + 37);
    assert_eq!(f14.string_width(" "), 4);
    assert_eq!(f14.string_width(""), 0);
    // Missing glyphs measure as '?' (7); U+FFFF is skipped (TB).
    assert_eq!(f14.string_width("中"), 7);
    assert_eq!(f14.string_width("a\u{FFFF}b"), f14.string_width("ab"));

    // At 28: H 21, e 17, l 8, l 8, o 17.
    let f28 = fm.face(SHADOW_28);
    assert_eq!(f28.string_width("Hello"), 21 + 17 + 8 + 8 + 17);
    assert_eq!(f28.string_width(" "), 9);

    let text = "The quick brown fox jumps over the lazy dog. ÅÄÖ åäö €½";
    for desc in [SHADOW_14, SHADOW_28] {
        assert_eq!(fm.face(desc).string_width(text), sum_advances(&fm, desc, text));
    }
}

#[test]
fn draw_text_emits_glyph_quads() {
    let mut fm = shadow_font();
    let mut r = Rec::default();
    let red = Color::rgb(255, 0, 0);
    frame(
        &mut fm,
        &mut r,
        &[(SHADOW_14, Point::new(10, 5), red, "Hi H"), (SHADOW_14, Point::new(10, 40), Color::WHITE, "iH")],
    );

    // One page, created once, all glyphs in one batch.
    assert_eq!(r.creates(), 1);
    assert_eq!(r.calls[1], Call::Create(BitmapId(1), Size::new(512, 512)));
    assert_eq!(r.updates(), 0);
    let draws = r.draws();
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].0, Some(BitmapId(1)));

    let cache = fm.glyph_cache();
    let frag = |c| cache.glyph(SHADOW_14, c).unwrap().frag.unwrap();
    let (h, i) = (frag('H'), frag('i'));
    assert_eq!(h.rect.size(), Size::new(17, 20));
    assert!(cache.glyph(SHADOW_14, ' ').unwrap().frag.is_none(), "a space has no image");
    let i_adv = fm.face(SHADOW_14).glyph_metrics('i').unwrap().advance;
    // The line's top-left is `pos`: TBBF glyphs sit at y + (-ascent) + ascent, and
    // x_ofs -3 puts each cell left of the pen (the room for the shadow).
    let q = |x, y, f: turbo_ferret::render::atlas::FragmentId, color: Color| Quad {
        dst: Rect::new(x, y, f.rect.w, f.rect.h),
        src: f.rect,
        color: color.premultiplied(),
    };
    assert_eq!(
        draws[0].1,
        [
            q(10 - 3, 5, h, red),
            q(10 + 11 - 3, 5, i, red),
            q(10 + 11 + i_adv + 4 - 3, 5, h, red),
            q(10 - 3, 40, i, Color::WHITE),
            q(10 + i_adv - 3, 40, h, Color::WHITE),
        ]
    );
    // The cached glyph texels are the premultiplied strip columns.
    let px = cache.atlas().page_pixels(h.page).unwrap();
    let stride = 512 * 4;
    assert!((0..20).any(|y| {
        let o = (h.rect.y as usize + y) * stride + h.rect.x as usize * 4;
        px[o..o + 17 * 4].chunks_exact(4).any(|p| p == [255, 255, 255, 255])
    }));
}

#[test]
fn glyphs_are_rasterized_before_one_upload() {
    let mut fm = shadow_font();
    let mut r = Rec::default();
    // Frame 1: everything is new; still a single create and no update.
    frame(&mut fm, &mut r, &[(SHADOW_28, Point::new(0, 0), Color::WHITE, "abcdefghijklmnop")]);
    assert_eq!((r.creates(), r.updates()), (1, 0));
    assert_eq!(fm.glyph_cache().len(), 16);

    // Frame 2: the same text needs no upload at all, and two strings share a batch.
    r.calls.clear();
    frame(
        &mut fm,
        &mut r,
        &[(SHADOW_28, Point::new(0, 0), Color::WHITE, "ponm"), (SHADOW_28, Point::new(0, 50), Color::WHITE, "abc")],
    );
    assert_eq!((r.creates(), r.updates()), (0, 0));
    assert_eq!(r.draws().len(), 1);

    // Frame 3: a new glyph updates the page once, after flushing the pending batch.
    r.calls.clear();
    frame(
        &mut fm,
        &mut r,
        &[(SHADOW_28, Point::new(0, 0), Color::WHITE, "abc"), (SHADOW_28, Point::new(0, 50), Color::WHITE, "xyzzy")],
    );
    let kinds: Vec<&str> = r
        .calls
        .iter()
        .map(|c| match c {
            Call::Begin => "begin",
            Call::End => "end",
            Call::Create(..) => "create",
            Call::Update(..) => "update",
            Call::Destroy(..) => "destroy",
            Call::Draw(..) => "draw",
        })
        .collect();
    assert_eq!(kinds, ["begin", "draw", "update", "draw", "end"]);
    assert_eq!(r.draws()[1].1.len(), 5);
}

#[test]
fn render_glyphs_needs_no_renderer() {
    let mut fm = shadow_font();
    assert!(fm.render_glyphs(SHADOW_14, "Hello, World"));
    assert_eq!(fm.glyph_cache().len(), 9); // distinct characters, including ' '
    assert!(fm.glyph_cache().atlas().is_dirty(0));
    // Missing glyphs render as '?', so they count as present.
    assert!(fm.render_glyphs(SHADOW_14, "中"));
    assert!(fm.render_glyphs(BOX_FONT, "boxes"));

    let mut r = Rec::default();
    frame(&mut fm, &mut r, &[(SHADOW_14, Point::new(0, 0), Color::WHITE, "World, Hello")]);
    assert_eq!((r.creates(), r.updates()), (1, 0));
    assert_eq!(fm.glyph_cache().len(), 10);
}

#[test]
fn missing_glyphs_draw_question_marks() {
    let mut fm = shadow_font();
    let mut r = Rec::default();
    frame(&mut fm, &mut r, &[(SHADOW_14, Point::new(0, 0), Color::WHITE, "?中")]);
    let q = r.quads();
    assert_eq!(q.len(), 2);
    assert_eq!(q[0].dst.size(), q[1].dst.size());
    // The second starts one advance (7) further.
    assert_eq!((q[0].dst.x, q[1].dst.x), (-3, 7 - 3));
}

#[test]
fn fallback_faces_draw() {
    let mut fm = shadow_font();
    let mut r = Rec::default();
    // Unknown font, no default: boxes. Each box is a 1 pixel outline (4 fills).
    frame(&mut fm, &mut r, &[(FontDesc::new(TbId::new("Nope"), 14), Point::new(10, 20), Color::WHITE, "ab")]);
    let draws = r.draws();
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].0, None);
    let dsts: Vec<Rect> = draws[0].1.iter().map(|q| q.dst).collect();
    assert_eq!(
        dsts,
        [
            Rect::new(10, 20, 5, 1),
            Rect::new(10, 35, 5, 1),
            Rect::new(10, 21, 1, 14),
            Rect::new(14, 21, 1, 14),
            Rect::new(16, 20, 5, 1),
            Rect::new(16, 35, 5, 1),
            Rect::new(16, 21, 1, 14),
            Rect::new(20, 21, 1, 14),
        ]
    );
    assert_eq!(fm.glyph_cache().len(), 0);

    // With a default, the default face draws and caches under its own description.
    fm.set_default(SHADOW_14);
    r.calls.clear();
    frame(&mut fm, &mut r, &[(FontDesc::new(TbId::new("Nope"), 14), Point::new(10, 20), Color::WHITE, "ab")]);
    assert_eq!(r.quads().len(), 2);
    assert!(fm.glyph_cache().glyph(SHADOW_14, 'a').is_some());
}

#[test]
fn background_fonts_draw_first() {
    let mut fm = shadow_font();
    let shadow = BackgroundFont { desc: BOX_FONT, color: Color::BLACK, offset: Point::new(1, 2) };
    assert!(fm.set_background_font(SHADOW_14, Some(shadow)));
    assert_eq!(fm.face(SHADOW_14).background(), Some(shadow));
    assert!(!fm.set_background_font(FontDesc::new(TbId::new("Nope"), 1), None));

    let mut r = Rec::default();
    frame(&mut fm, &mut r, &[(SHADOW_14, Point::new(10, 10), Color::WHITE, "a")]);
    let draws = r.draws();
    assert_eq!(draws.len(), 2);
    assert_eq!(draws[0].0, None); // the box, behind
    assert_eq!(draws[0].1[0].dst.origin(), Point::new(11, 12));
    assert_eq!(draws[0].1[0].color, Color::BLACK);
    assert!(draws[1].0.is_some()); // then the glyph

    // A face that is its own background stops after a few levels.
    let own = BackgroundFont { desc: SHADOW_14, color: Color::BLACK, offset: Point::new(1, 1) };
    assert!(fm.set_background_font(SHADOW_14, Some(own)));
    r.calls.clear();
    frame(&mut fm, &mut r, &[(SHADOW_14, Point::new(10, 10), Color::WHITE, "a")]);
    assert_eq!(r.quads().len(), 5);

    assert!(fm.set_background_font(SHADOW_14, None));
    r.calls.clear();
    frame(&mut fm, &mut r, &[(SHADOW_14, Point::new(10, 10), Color::WHITE, "a")]);
    assert_eq!(r.quads().len(), 1);
}

#[test]
fn color_fonts_ignore_the_text_color() {
    let mut fm = FontManager::new();
    let glow = FontDesc::new(fm.add_font("Glow", "demo_fonts/glow.tb.txt"), 42);
    assert!(create(&mut fm, glow));
    let mut r = Rec::default();
    frame(&mut fm, &mut r, &[(glow, Point::new(0, 0), Color::rgb(255, 0, 0), "Ab")]);
    let q = r.quads();
    assert_eq!(q.len(), 2);
    assert!(q.iter().all(|q| q.color == Color::WHITE));
    // x_ofs -8 shifts each glyph left of the pen.
    assert_eq!(q[0].dst.x, -8);
}

#[test]
fn cache_clears_between_frames_when_over_its_limit() {
    // 64x64 pages hold one row of 42 pixel high glyphs: a few glyphs per page.
    let mut fm = shadow_font_with(FontManager::with_glyph_cache(Size::new(64, 64), 1));
    let mut r = Rec::default();
    frame(&mut fm, &mut r, &[(SHADOW_28, Point::new(0, 0), Color::WHITE, "ab")]);
    assert_eq!(fm.glyph_cache().page_count(), 1);
    frame(&mut fm, &mut r, &[(SHADOW_28, Point::new(0, 0), Color::WHITE, "ab")]);
    assert_eq!(fm.glyph_cache().len(), 2, "within the limit: kept");

    // Over the limit within a frame: the atlas grows, nothing is evicted mid-frame.
    r.calls.clear();
    frame(&mut fm, &mut r, &[(SHADOW_28, Point::new(0, 0), Color::WHITE, "MWMWQ")]);
    let pages = fm.glyph_cache().page_count();
    assert!(pages > 1);
    assert_eq!(r.quads().len(), 5);
    assert_eq!(r.destroys(), 0);

    // The next frame starts empty; the old bitmaps are destroyed at the next upload.
    r.calls.clear();
    fm.begin_frame();
    assert!(fm.glyph_cache().is_empty());
    assert_eq!(fm.glyph_cache().page_count(), 0);
    frame(&mut fm, &mut r, &[(SHADOW_28, Point::new(0, 0), Color::WHITE, "ab")]);
    assert_eq!(r.destroys(), pages);
    assert_eq!(fm.glyph_cache().len(), 2);
    assert_eq!(fm.glyph_cache().max_pages(), 1);

    // A frame that alone needs more pages than the limit raises it instead of
    // re-rasterizing every frame.
    let mwq = [(SHADOW_28, Point::new(0, 0), Color::WHITE, "MWMWQ")];
    frame(&mut fm, &mut r, &mwq); // within the limit at the start; grows
    frame(&mut fm, &mut r, &mwq); // over: cleared first, then only M, W, Q
    let needed = fm.glyph_cache().page_count();
    assert!(needed > 1);
    assert_eq!(fm.glyph_cache().len(), 3);
    frame(&mut fm, &mut r, &mwq); // over again right after a clear: limit raised
    assert_eq!(fm.glyph_cache().max_pages(), needed);
    assert_eq!(fm.glyph_cache().len(), 3);
    r.calls.clear();
    frame(&mut fm, &mut r, &[(SHADOW_28, Point::new(0, 0), Color::WHITE, "MWMWQ")]);
    assert_eq!((r.creates(), r.updates()), (0, 0), "steady state: nothing re-rasterized");
}

#[test]
fn renderer_lost_and_destroy_bitmaps() {
    let mut fm = shadow_font();
    let mut r = Rec::default();
    frame(&mut fm, &mut r, &[(SHADOW_14, Point::new(0, 0), Color::WHITE, "Hi")]);
    assert_eq!(r.creates(), 1);

    // Context loss: the bitmap is forgotten (not destroyed) and re-created.
    fm.renderer_lost();
    frame(&mut fm, &mut r, &[(SHADOW_14, Point::new(0, 0), Color::WHITE, "Hi")]);
    assert_eq!((r.creates(), r.destroys()), (2, 0));
    let Some((Some(b), _)) = r.draws().last().copied() else { panic!() };
    assert_eq!(b, BitmapId(2));

    // Explicit destroy: destroyed now, re-created on the next draw.
    fm.destroy_bitmaps(&mut r);
    assert_eq!(r.calls.last(), Some(&Call::Destroy(BitmapId(2))));
    frame(&mut fm, &mut r, &[(SHADOW_14, Point::new(0, 0), Color::WHITE, "Hi")]);
    assert_eq!(r.creates(), 3);
}
