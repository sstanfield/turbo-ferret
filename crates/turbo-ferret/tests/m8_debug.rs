//! M8: the debug overlays and the debug settings window (feature `debug`).
//! TB: tb_debug.cpp, and the TB_IF_DEBUG_SETTING lines in the paint code.
#![cfg(feature = "debug")]

use std::cell::RefCell;

use turbo_ferret::debug::{DebugSettings, FrameStats};
use turbo_ferret::geom::{Color, Point, Rect, Size};
use turbo_ferret::services::Level;
use turbo_ferret::widgets::*;
use turbo_ferret::*;

/// Records what is drawn: solid quads by color, textured quads by bitmap.
#[derive(Default)]
struct Rec {
    next: u32,
    solid: Vec<(Color, Rect)>,
    textured: Vec<(BitmapId, Rect)>,
    calls: u32,
}

impl Renderer for Rec {
    fn begin_frame(&mut self, _viewport: Size) {
        self.solid.clear();
        self.textured.clear();
        self.calls = 0;
    }
    fn end_frame(&mut self) {}
    fn create_bitmap(&mut self, _size: Size, _rgba: &[u8]) -> Option<BitmapId> {
        self.next += 1;
        Some(BitmapId(self.next))
    }
    fn update_bitmap(&mut self, _bitmap: BitmapId, _rgba: &[u8]) {}
    fn destroy_bitmap(&mut self, _bitmap: BitmapId) {}
    fn draw(&mut self, bitmap: Option<BitmapId>, _clip: Rect, quads: &[Quad]) {
        self.calls += 1;
        for q in quads {
            match bitmap {
                Some(b) => self.textured.push((b, q.dst)),
                None => self.solid.push((q.color, q.dst)),
            }
        }
    }
}

impl Rec {
    /// The quads drawn in a (straight alpha) color.
    fn in_color(&self, c: Color) -> Vec<Rect> {
        let c = c.premultiplied();
        self.solid.iter().filter(|(col, _)| *col == c).map(|(_, r)| *r).collect()
    }

    /// The outline rects drawn in a color: four one pixel quads each.
    fn outlines(&self, c: Color) -> Vec<Rect> {
        self.in_color(c).chunks(4).map(|q| q.iter().fold(q[0], |a, b| a.union(b))).collect()
    }
}

const WHITE: Color = Color::rgba(255, 255, 255, 50);
const RED: Color = Color::rgba(255, 0, 0, 200);
const ARRANGED: Color = Color::rgba(255, 30, 30, 200);
const MEASURED: Color = Color::rgba(255, 255, 30, 200);

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn log(level: Level, args: std::fmt::Arguments<'_>) {
    if level >= Level::Info {
        LOG.with(|l| l.borrow_mut().push(format!("{level:?}: {args}")));
    }
}

/// A `Ui` with the default skin and font, logging into `LOG`.
fn ui() -> Ui {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = Some(log);
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    ui.add_font("DejaVuShadow", "default_font/dejavu_white_with_shadow.tb.txt");
    assert!(ui.set_default_font(FontDesc::new(tbid!("DejaVuShadow"), 14)));
    ui.set_viewport(Size::new(640, 480));
    ui.set_time(1000);
    LOG.with(|l| l.borrow_mut().clear());
    ui
}

const SRC: &str = "TBLayout: axis: y, id: col\n\
    \tTBButton: id: ok, text: OK\n\
    \tTBCheckBox: id: check\n\
    \tTBScrollContainer: id: scroll\n\
    \t\tlp: width: 100, height: 60\n\
    \t\tTBButton: id: inner, text: Inside\n\
    \t\t\tlp: width: 300, height: 200\n";

fn build(ui: &mut Ui) -> WidgetId {
    let root = ui.root();
    let ids = ui.inflate(root, SRC).unwrap();
    ui.set_rect(ids[0], Rect::new(20, 20, 200, 200));
    ui.update();
    ids[0]
}

fn click(ui: &mut Ui, w: WidgetId) {
    let p = ui.to_root(w, Point::new(ui.rect(w).w / 2, ui.rect(w).h / 2));
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
}

#[test]
fn nothing_is_drawn_by_default() {
    let mut ui = ui();
    build(&mut ui);
    assert_eq!(ui.debug_settings(), DebugSettings::default());
    let mut r = Rec::default();
    ui.paint(&mut r);
    for c in [WHITE, RED, ARRANGED, MEASURED] {
        assert!(r.in_color(c).is_empty());
    }
    assert_eq!(ui.debug_frame_stats(), FrameStats::default());
    assert!(!ui.needs_repaint());
}

#[test]
fn layout_bounds_outline_every_widget() {
    let mut ui = ui();
    let col = build(&mut ui);
    ui.set_debug_settings(DebugSettings { layout_bounds: true, ..Default::default() });
    assert!(ui.needs_repaint());
    let mut r = Rec::default();
    ui.paint(&mut r);
    let outlines = r.outlines(WHITE);
    let rect_of = |ui: &Ui, w: WidgetId| Rect::from_origin_size(ui.to_root(w, Point::ZERO), ui.rect(w).size());
    for name in ["col", "ok", "check", "scroll"] {
        let w = ui.find(ui.root(), TbId::new(name)).unwrap();
        assert!(outlines.contains(&rect_of(&ui, w)), "{name} is outlined");
    }
    assert!(outlines.contains(&Rect::new(0, 0, 640, 480)), "the root too");
    assert!(outlines.len() >= 6);
    let _ = col;
}

#[test]
fn layout_clipping_outlines_clip_rects() {
    let mut ui = ui();
    build(&mut ui);
    ui.set_debug_settings(DebugSettings { layout_clipping: true, ..Default::default() });
    let mut r = Rec::default();
    ui.paint(&mut r);
    let outlines = r.outlines(RED);
    // The scroll container's root clips its content to its own area, but only
    // towards hidden content: on the other sides it leaves 100 pixels (as TB does).
    let scroll = ui.find(ui.root(), tbid!("scroll")).unwrap();
    let area = ui.content_root(scroll);
    let clip = Rect::from_origin_size(ui.to_root(area, Point::ZERO), ui.rect(area).size());
    assert!(clip.w < 300 && clip.h < 200, "smaller than the content: {clip:?}");
    assert_eq!(outlines, [clip.expand(100, 100, 0, 0)], "content is hidden to the right and below");
    // Scrolled to the middle there is hidden content on every side.
    ui.scroll_to(scroll, Point::new(50, 50));
    ui.update();
    ui.paint(&mut r);
    assert_eq!(r.outlines(RED), [clip]);
}

#[test]
fn size_calculation_flashes_measured_and_arranged_widgets() {
    let mut ui = ui();
    let col = build(&mut ui);
    let mut r = Rec::default();
    ui.paint(&mut r);
    ui.set_debug_settings(DebugSettings { layout_size_calculation: true, ..Default::default() });
    ui.paint(&mut r);
    assert!(r.in_color(ARRANGED).is_empty() && r.in_color(MEASURED).is_empty(), "nothing was laid out since");

    // A new text: the button measures again and its layout arranges again.
    let ok = ui.find(col, tbid!("ok")).unwrap();
    ui.set_text(ok, "A longer text");
    ui.update();
    ui.paint(&mut r);
    let rect_of = |ui: &Ui, w: WidgetId| Rect::from_origin_size(ui.to_root(w, Point::ZERO), ui.rect(w).size());
    assert!(r.outlines(ARRANGED).contains(&rect_of(&ui, col)), "the layout is red");
    assert!(r.outlines(MEASURED).contains(&rect_of(&ui, ok).shrink(1, 1, 1, 1)), "the button is yellow, one pixel in");
    assert!(ui.needs_repaint(), "it keeps painting while something is highlighted");

    // 299 ms later it still shows, at 300 ms it is gone and painting stops.
    ui.set_time(1299);
    ui.paint(&mut r);
    assert!(!r.in_color(ARRANGED).is_empty() && ui.needs_repaint());
    ui.set_time(1300);
    ui.paint(&mut r);
    assert!(r.in_color(ARRANGED).is_empty() && r.in_color(MEASURED).is_empty());
    assert!(!ui.needs_repaint());
}

#[test]
fn render_batches_tints_and_counts() {
    let mut ui = ui();
    build(&mut ui);
    let mut r = Rec::default();
    ui.paint(&mut r);
    let (plain_calls, plain_quads) = (r.calls, r.solid.len() + r.textured.len());
    assert!(plain_calls >= 1 && plain_quads >= 4);

    ui.set_debug_settings(DebugSettings { render_batches: true, ..Default::default() });
    ui.paint(&mut r);
    let stats = ui.debug_frame_stats();
    assert_eq!(stats.batches, plain_calls, "the frame's own batches");
    assert_eq!(stats.triangles as usize, plain_quads * 2);
    // Each batch was drawn again without a bitmap, half transparent.
    assert_eq!(r.calls, plain_calls * 2);
    let tinted = r.solid.iter().filter(|(c, _)| c.a == 0xAA).count();
    assert_eq!(tinted, plain_quads);
    // TB printed the counts every frame; here they are logged when they change.
    let line =
        format!("Info: Frame rendered using {} batches and a total of {} triangles.", stats.batches, stats.triangles);
    assert_eq!(LOG.with(|l| l.borrow().clone()), [line]);
    ui.paint(&mut r);
    assert_eq!(LOG.with(|l| l.borrow().len()), 1);

    ui.set_debug_settings(DebugSettings::default());
    ui.paint(&mut r);
    assert_eq!(r.calls, plain_calls);
    assert_eq!(ui.debug_frame_stats(), FrameStats::default());
}

#[test]
fn the_debug_window_switches_settings() {
    let mut ui = ui();
    build(&mut ui);
    ui.set_debug_settings(DebugSettings { layout_clipping: true, ..Default::default() });
    let win = ui.show_debug_window();
    ui.update();
    assert_eq!(ui.text(win), Some("Debug settings"));
    // Centered in the root.
    let r = ui.rect(win);
    assert!((r.x + r.w / 2 - 320).abs() <= 1 && (r.y + r.h / 2 - 240).abs() <= 1, "{r:?}");
    // The check boxes show the settings.
    let check = |ui: &Ui, name: &str| ui.find(win, TbId::new(name)).unwrap();
    assert_eq!(ui.value(check(&ui, "debug.layout_clipping")).as_int(), 1);
    assert_eq!(ui.value(check(&ui, "debug.layout_bounds")).as_int(), 0);

    let bounds = check(&ui, "debug.layout_bounds");
    click(&mut ui, bounds);
    assert_eq!(ui.debug_settings(), DebugSettings { layout_bounds: true, layout_clipping: true, ..Default::default() });
    assert!(ui.needs_repaint());
    let clipping = check(&ui, "debug.layout_clipping");
    click(&mut ui, clipping);
    click(&mut ui, bounds);
    assert_eq!(ui.debug_settings(), DebugSettings::default());
    for name in ["render_batches", "skin_bitmap_fragments", "font_bitmap_fragments", "layout_size_calculation"] {
        let c = check(&ui, &format!("debug.{name}"));
        click(&mut ui, c);
    }
    let s = ui.debug_settings();
    assert!(s.render_batches && s.skin_bitmap_fragments && s.font_bitmap_fragments && s.layout_size_calculation);
}

#[test]
fn the_debug_window_logs_events() {
    let mut ui = ui();
    let col = build(&mut ui);
    let win = ui.show_debug_window();
    ui.update();
    let output = ui.find(win, tbid!("output")).unwrap();
    assert_eq!(ui.text(output), Some(""));

    // A click on a button: the id is shown by the name it had in the resource.
    let ok = ui.find(col, tbid!("ok")).unwrap();
    click(&mut ui, ok);
    assert_eq!(ui.text(output), Some("CLICK (Button) id: \"ok\"\n"));
    // A check box is clicked, which changes its value.
    let check = ui.find(col, tbid!("check")).unwrap();
    click(&mut ui, check);
    assert_eq!(
        ui.text(output),
        Some(
            "CLICK (Button) id: \"ok\"\nCLICK (CheckBox) id: \"check\"\nCHANGED (CheckBox) id: \"check\", value: 1.00 (\"1\")\n"
        )
    );
    // A key goes to the focused widget; an id that was never named is a number.
    let nameless = ui.add(col, Button::default());
    ui.set_id(nameless, TbId(77));
    ui.set_focus(nameless);
    ui.key(Some('x'), SpecialKey::None, Modifiers::NONE, true);
    let text = ui.text(output).unwrap().to_string();
    assert!(text.ends_with("KEY_DOWN (Button) id: 77\n"), "{text}");

    // Activity in the debug window itself is not logged.
    let lines = text.lines().count();
    let bounds = ui.find(win, tbid!("debug.layout_bounds")).unwrap();
    click(&mut ui, bounds);
    assert_eq!(ui.text(output).unwrap().lines().count(), lines);

    // Closing the window stops the log (and nothing is left behind).
    ui.remove(win);
    ui.update();
    click(&mut ui, ok);
    assert!(!ui.exists(output));
}

#[test]
fn the_event_log_keeps_its_last_lines() {
    let mut ui = ui();
    let col = build(&mut ui);
    let win = ui.show_debug_window();
    ui.update();
    let output = ui.find(win, tbid!("output")).unwrap();
    let ok = ui.find(col, tbid!("ok")).unwrap();
    ui.set_focus(ok);
    for _ in 0..150 {
        ui.key(Some('a'), SpecialKey::None, Modifiers::NONE, true);
        ui.key(Some('a'), SpecialKey::None, Modifiers::NONE, false);
    }
    let text = ui.text(output).unwrap();
    assert_eq!(text.lines().count(), 200, "300 events, the last 200 kept");
    assert!(text.lines().next().unwrap().starts_with("KEY_DOWN"));
    assert!(text.lines().last().unwrap().starts_with("KEY_UP"));
    // The field is scrolled to its end.
    ui.update();
    let info = ui.scroll_info(output).unwrap();
    assert!(info.max.y > 0 && info.pos.y == info.max.y, "{info:?}");
}

#[test]
fn atlas_pages_are_shown_beside_the_debug_window() {
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    ui.add_font("DejaVuShadow", "default_font/dejavu_white_with_shadow.tb.txt");
    assert!(ui.set_default_font(FontDesc::new(tbid!("DejaVuShadow"), 14)));
    ui.set_viewport(Size::new(1600, 1200));
    let win = ui.show_debug_window();
    ui.update();
    // Let the window fade in.
    ui.set_time(2000);
    ui.update();
    let mut r = Rec::default();
    ui.paint(&mut r);
    let right = ui.rect(win).right();
    let top = ui.rect(win).y;
    let beside = |r: &Rec| -> Vec<Rect> {
        r.textured.iter().map(|(_, d)| *d).filter(|d| d.x >= right && d.y == top && d.w >= 256).collect()
    };
    assert!(beside(&r).is_empty());

    ui.set_debug_settings(DebugSettings { skin_bitmap_fragments: true, ..Default::default() });
    ui.paint(&mut r);
    let skin_page = ui.skin_data().atlas().page_size(0);
    assert_eq!(beside(&r), [Rect::new(right, top, skin_page.w, skin_page.h)]);

    // Both: the glyph cache's page starts at the same place (as in TB).
    ui.set_debug_settings(DebugSettings {
        skin_bitmap_fragments: true,
        font_bitmap_fragments: true,
        ..Default::default()
    });
    ui.paint(&mut r);
    let glyph_page = ui.fonts().glyph_cache().atlas().page_size(0);
    assert_eq!(
        beside(&r),
        [Rect::new(right, top, skin_page.w, skin_page.h), Rect::new(right, top, glyph_page.w, glyph_page.h)]
    );
}

#[test]
fn id_names_are_remembered_and_collisions_reported() {
    let mut ui = ui();
    assert_eq!(ui.id_name(tbid!("ok")), None);
    build(&mut ui);
    // Names from the resource.
    assert_eq!(ui.id_name(tbid!("ok")), Some("ok"));
    assert_eq!(ui.id_name(tbid!("scroll")), Some("scroll"));
    // Names given by the app.
    assert_eq!(ui.name_id("my-widget"), tbid!("my-widget"));
    assert_eq!(ui.id_name(tbid!("my-widget")), Some("my-widget"));
    assert_eq!(ui.name_id(""), TbId::NONE);
    assert_eq!(ui.id_name(TbId::NONE), None);
    assert!(LOG.with(|l| l.borrow().is_empty()));

    // Two words with the same FNV-1a hash.
    assert_eq!(TbId::new("costarring"), TbId::new("liquid"));
    ui.name_id("costarring");
    ui.name_id("costarring");
    assert!(LOG.with(|l| l.borrow().is_empty()));
    ui.name_id("liquid");
    let log = LOG.with(|l| l.borrow().clone());
    assert_eq!(log.len(), 1);
    assert!(log[0].starts_with("Warn: id collision: \"costarring\" and \"liquid\""), "{}", log[0]);
    assert_eq!(ui.id_name(TbId::new("liquid")), Some("costarring"), "the first name stays");
}

/// With two debug windows open, neither logs the other (nor what the other's own
/// logging changes). TB ignored activity in any debug window.
#[test]
fn two_debug_windows_do_not_log_each_other() {
    let mut ui = ui();
    let col = build(&mut ui);
    let a = ui.show_debug_window();
    let b = ui.show_debug_window();
    ui.set_rect(a, Rect::new(220, 0, 200, 400));
    ui.set_rect(b, Rect::new(430, 0, 200, 400));
    ui.update();
    let (out_a, out_b) = (ui.find(a, tbid!("output")).unwrap(), ui.find(b, tbid!("output")).unwrap());
    let ok = ui.find(col, tbid!("ok")).unwrap();
    click(&mut ui, ok);
    let line = "CLICK (Button) id: \"ok\"\n";
    assert_eq!((ui.text(out_a), ui.text(out_b)), (Some(line), Some(line)));
    // Activity in one debug window is in neither log.
    let check = ui.find(b, tbid!("debug.layout_clipping")).unwrap();
    click(&mut ui, check);
    assert!(ui.debug_settings().layout_clipping);
    assert_eq!((ui.text(out_a), ui.text(out_b)), (Some(line), Some(line)));
}

/// The names of ids are remembered up to a limit.
#[test]
fn id_names_are_capped() {
    let mut ui = ui();
    for i in 0..10_050 {
        ui.name_id(&format!("generated-{i}"));
    }
    assert_eq!(ui.id_name(TbId::new("generated-0")), Some("generated-0"));
    assert_eq!(ui.id_name(TbId::new("generated-9999")), Some("generated-9999"));
    assert_eq!(ui.id_name(TbId::new("generated-10049")), None, "past the limit");
}
