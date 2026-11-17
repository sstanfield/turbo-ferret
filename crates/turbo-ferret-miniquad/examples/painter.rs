//! Opens a window and draws a test scene with the core `Painter` on
//! `MiniquadRenderer`, and forwards input to a `Ui` through `InputAdapter`.
//!
//! ```text
//! cargo run -p turbo-ferret-miniquad --example painter [-- OPTIONS]
//!   --metal         use Metal instead of OpenGL (macOS)
//!   --frames N      quit after N frames
//!   --check [DIR]   render the scene offscreen on the GPU (nearest filtering), compare it
//!                   with the software renderer, write DIR/{gpu,soft}.tga, exit 0 if equal
//! ```
//!
//! What to look for, top to bottom:
//! 1. Red and green squares, a half-transparent white bar across both, a yellow
//!    1 pixel outline; a checkerboard from an atlas at 1:1 and 2:1; the
//!    checkerboard and a yellow bar clipped to the square inside the magenta outline.
//! 2. An "F" with a translucent green corner: plain, mirrored left-right, mirrored
//!    top-bottom, both; tinted blue; at half opacity with a half-opacity white bar.
//! 3. A 2x2 bitmap before (red, green / blue, white) and after (cyan, magenta /
//!    yellow, black) a mid-frame update; a magenta square from a bitmap destroyed
//!    right after drawing (--check only); a 64x40 gradient of 2560 quads.
//! 4. Input: four tiles (hover lightens, press darkens, click toggles, Tab or a click
//!    moves the white focus outline), yellow dots for the last click count, a bar
//!    moved by the wheel, a white square at the pointer. Events print to stdout.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use miniquad::conf::{AppleGfxApi, Conf};
use miniquad::{
    EventHandler, FilterMode, KeyCode, KeyMods, MouseButton, PassAction, TextureFormat, TextureParams, TouchPhase,
    window,
};
use turbo_ferret::render::atlas::{Atlas, FragmentId};
use turbo_ferret::render::painter::Painter;
use turbo_ferret::{
    BitmapId, Color, Config, Event, EventKind, Point, Rect, Services, Size, TbId, Ui, UiListener, Widget, WidgetId,
    WidgetState,
};
use turbo_ferret_miniquad::{InputAdapter, MiniquadRenderer, window_dpi};
use turbo_ferret_soft::SoftRenderer;

// ----------------------------------------------------------------------
// The test scene (shared by the window and --check)
// ----------------------------------------------------------------------

/// Size of the scene at scale 1 (the --check image).
const SCENE: Size = Size::new(456, 288);

const SWAP_A: [u8; 16] = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
const SWAP_B: [u8; 16] = [0, 255, 255, 255, 255, 0, 255, 255, 255, 255, 0, 255, 0, 0, 0, 255];

/// 32x32 checkerboard, 4 pixel cells, white and blue.
fn checkerboard() -> Vec<u8> {
    let mut v = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32 {
        for x in 0..32 {
            let c: [u8; 4] = if (x / 4 + y / 4) % 2 == 0 { [255, 255, 255, 255] } else { [40, 70, 200, 255] };
            v.extend_from_slice(&c);
        }
    }
    v
}

/// 16x16 "F" (opaque orange) with a translucent green bottom-right corner, on
/// transparent: asymmetric on both axes, so every mirroring is visible.
fn letter_f() -> Vec<u8> {
    let mut v = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let stem = (3..6).contains(&x) && (2..14).contains(&y);
            let top = (3..13).contains(&x) && (2..5).contains(&y);
            let mid = (3..10).contains(&x) && (7..10).contains(&y);
            let c: [u8; 4] = if stem || top || mid {
                [255, 140, 0, 255]
            } else if x >= 12 && y >= 12 {
                [0, 128, 0, 128] // premultiplied green at alpha 128
            } else {
                [0, 0, 0, 0]
            };
            v.extend_from_slice(&c);
        }
    }
    v
}

struct Scene {
    atlas: Atlas,
    checker: FragmentId,
    letter: FragmentId,
    swap: Option<BitmapId>,
    /// Integer scale (the window uses the DPI scale, --check uses 1).
    k: i32,
    /// Create, draw and destroy a bitmap each frame (--check only: miniquad keeps a
    /// small record per texture ever created).
    destroy_test: bool,
}

impl Scene {
    fn new(k: i32, destroy_test: bool) -> Scene {
        let mut atlas = Atlas::new(Size::new(128, 128));
        let checker = atlas.add(Size::new(32, 32), &checkerboard()).expect("atlas space");
        let letter = atlas.add(Size::new(16, 16), &letter_f()).expect("atlas space");
        Scene { atlas, checker, letter, swap: None, k: k.max(1), destroy_test }
    }

    fn paint(&mut self, p: &mut Painter) {
        let k = self.k;
        let r = |x: i32, y: i32, w: i32, h: i32| Rect::new(x * k, y * k, w * k, h * k);
        p.fill_rect(r(0, 0, SCENE.w, SCENE.h), Color::rgb(32, 34, 40));

        // 1. Solid fills, blending, an outline.
        p.fill_rect(r(16, 16, 48, 48), Color::rgb(220, 40, 40));
        p.fill_rect(r(40, 32, 48, 48), Color::rgb(40, 200, 60));
        p.fill_rect(r(8, 40, 88, 16), Color::rgba(255, 255, 255, 128));
        p.draw_rect(r(100, 16, 48, 48), Color::rgb(250, 220, 0));

        // Atlas bitmap at 1:1 and 2:1.
        let bmp = self.atlas.page_bitmap(self.checker.page, p.renderer()).expect("atlas upload");
        let cr = self.checker.rect;
        p.draw_bitmap(r(160, 16, 32, 32), cr, bmp, Color::WHITE);
        p.draw_bitmap(r(200, 16, 64, 64), cr, bmp, Color::WHITE);

        // Clipped: a 3:1 checkerboard and a bar, inside a 48x48 window.
        let clip = r(288, 24, 48, 48);
        let old = p.set_clip(clip);
        p.draw_bitmap(r(264, 0, 96, 96), cr, bmp, Color::WHITE);
        p.fill_rect(r(272, 44, 80, 8), Color::rgb(250, 220, 0));
        p.set_clip(old);
        p.draw_rect(clip.expand_xy(1, 1), Color::rgb(255, 0, 255));

        // 2. Mirrored blits (4:1): plain, left-right, top-bottom, both.
        let bmp = self.atlas.page_bitmap(self.letter.page, p.renderer()).expect("atlas upload");
        let a = self.letter.rect;
        let flips = [
            a,
            Rect::new(a.right(), a.y, -a.w, a.h),
            Rect::new(a.x, a.bottom(), a.w, -a.h),
            Rect::new(a.right(), a.bottom(), -a.w, -a.h),
        ];
        for (i, src) in flips.into_iter().enumerate() {
            p.draw_bitmap(r(16 + i as i32 * 72, 112, 64, 64), src, bmp, Color::WHITE);
        }
        // Tint (texel x color) and opacity.
        p.draw_bitmap(r(304, 112, 64, 64), a, bmp, Color::rgb(80, 160, 255));
        p.set_opacity(0.5);
        p.draw_bitmap(r(376, 112, 64, 64), a, bmp, Color::WHITE);
        p.fill_rect(r(376, 160, 64, 8), Color::WHITE);
        p.set_opacity(1.0);

        // 3. A bitmap updated mid-frame: the first draw must keep the old contents.
        let swap = match self.swap {
            Some(b) => b,
            None => *self.swap.insert(p.renderer().create_bitmap(Size::new(2, 2), &SWAP_A).expect("bitmap")),
        };
        p.renderer().update_bitmap(swap, &SWAP_A);
        p.draw_bitmap(r(16, 192, 32, 32), Rect::new(0, 0, 2, 2), swap, Color::WHITE);
        p.renderer().update_bitmap(swap, &SWAP_B);
        p.draw_bitmap(r(56, 192, 32, 32), Rect::new(0, 0, 2, 2), swap, Color::WHITE);
        // A bitmap destroyed right after it is drawn.
        if self.destroy_test {
            let t = p.renderer().create_bitmap(Size::new(1, 1), &[255, 0, 255, 255]).expect("bitmap");
            p.draw_bitmap(r(96, 192, 32, 32), Rect::new(0, 0, 1, 1), t, Color::WHITE);
            p.renderer().destroy_bitmap(t);
        }
        // More quads than one painter batch.
        for y in 0..40 {
            for x in 0..64 {
                p.fill_rect(r(144 + x * 2, 192 + y * 2, 2, 2), Color::rgb((x * 4) as u8, (y * 6) as u8, 160));
            }
        }
    }
}

// ----------------------------------------------------------------------
// Input demo: widgets plus a listener that records and prints events
// ----------------------------------------------------------------------

#[derive(Default)]
struct Shared {
    pointer: Point,
    clicks: u32,
    wheel: Point,
    toggled: Vec<WidgetId>,
}

struct Pad;
impl Widget for Pad {
    fn type_name(&self) -> &'static str {
        "Pad"
    }
}

struct Tile;
impl Widget for Tile {
    fn type_name(&self) -> &'static str {
        "Tile"
    }
}

fn command_name(id: TbId) -> String {
    let names =
        ["cut", "copy", "paste", "selectall", "undo", "redo", "new", "open", "save", "close", "prev_doc", "next_doc"];
    names.iter().find(|n| TbId::new(n) == id).map_or_else(|| format!("{id:?}"), |n| n.to_string())
}

struct Logger {
    shared: Rc<RefCell<Shared>>,
}

impl UiListener for Logger {
    fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool {
        let pos = ui.to_root(target, ev.pos);
        let mut s = self.shared.borrow_mut();
        match ev.kind {
            EventKind::PointerMove | EventKind::TouchMove => {
                s.pointer = pos;
                return false; // too chatty to print
            }
            EventKind::PointerDown => {
                s.pointer = pos;
                s.clicks = ev.count;
            }
            EventKind::Wheel => s.wheel += ev.delta,
            EventKind::Click if ui.node(target).is_some_and(|n| n.type_name() == "Tile") => {
                match s.toggled.iter().position(|&t| t == target) {
                    Some(i) => {
                        s.toggled.remove(i);
                    }
                    None => s.toggled.push(target),
                }
            }
            _ => {}
        }
        let name = ui.node(target).map_or("?", |n| n.type_name());
        let mut line = format!("{:<12} {name:<4} at {:>4},{:<4}", format!("{:?}", ev.kind), pos.x, pos.y);
        match ev.kind {
            EventKind::PointerDown | EventKind::TouchDown => line += &format!(" count={}", ev.count),
            EventKind::Wheel => line += &format!(" delta={},{}", ev.delta.x, ev.delta.y),
            EventKind::KeyDown | EventKind::KeyUp => line += &format!(" key={:?} special={:?}", ev.key, ev.special),
            EventKind::Shortcut => line += &format!(" {}", command_name(ev.ref_id)),
            EventKind::FileDrop => {
                if let Some(files) = ev.payload.as_ref().and_then(|p| p.downcast_ref::<Vec<String>>()) {
                    line += &format!(" {files:?}");
                }
            }
            _ => {}
        }
        if !ev.mods.is_empty() {
            line += &format!(" {:?}", ev.mods);
        }
        if ev.touch {
            line += " (touch)";
        }
        println!("{line}");
        false
    }
}

fn paint_input_overlay(p: &mut Painter, ui: &Ui, tiles: &[WidgetId], s: &Shared, k: i32) {
    for &t in tiles {
        let rect = Rect::from_origin_size(ui.to_root(t, Point::ZERO), ui.rect(t).size());
        let state = ui.auto_state(t);
        let mut c = if s.toggled.contains(&t) { [40, 150, 140] } else { [70, 80, 100] };
        let shift = |c: [i32; 3], d: i32| c.map(|v| (v + d).clamp(0, 255));
        if state.contains(WidgetState::PRESSED) {
            c = shift(c, -40);
        } else if state.contains(WidgetState::HOVERED) {
            c = shift(c, 50);
        }
        p.fill_rect(rect, Color::rgb(c[0] as u8, c[1] as u8, c[2] as u8));
        if ui.focused() == Some(t) {
            for i in 1..=k.max(1) {
                p.draw_rect(rect.expand_xy(2 * k + i, 2 * k + i), Color::WHITE);
            }
        }
    }
    let Some(&first) = tiles.first() else { return };
    let base = ui.to_root(first, Point::ZERO) + Point::new(0, ui.rect(first).h + 12 * k);
    // Last click count.
    for i in 0..s.clicks.min(12) as i32 {
        p.fill_rect(Rect::new(base.x + i * 12 * k, base.y, 8 * k, 8 * k), Color::rgb(250, 220, 0));
    }
    // Wheel: a bar on a track, one step = 4 units.
    let track = Rect::new(base.x + 200 * k, base.y, 200 * k, 8 * k);
    p.fill_rect(track, Color::rgb(60, 60, 70));
    let along = |v: i32| (v * 4 * k).rem_euclid(track.w - 8 * k);
    p.fill_rect(Rect::new(track.x + along(s.wheel.y), track.y, 8 * k, 8 * k), Color::rgb(120, 220, 255));
    let track = track.offset(0, 12 * k);
    p.fill_rect(track, Color::rgb(60, 60, 70));
    p.fill_rect(Rect::new(track.x + along(s.wheel.x), track.y, 8 * k, 8 * k), Color::rgb(255, 160, 120));
    // Pointer.
    let s3 = 3 * k;
    p.fill_rect(Rect::new(s.pointer.x - s3 - k, s.pointer.y - s3 - k, 2 * (s3 + k), 2 * (s3 + k)), Color::BLACK);
    p.fill_rect(Rect::new(s.pointer.x - s3, s.pointer.y - s3, 2 * s3, 2 * s3), Color::WHITE);
}

// ----------------------------------------------------------------------
// --check: GPU (offscreen, nearest) against the software renderer
// ----------------------------------------------------------------------

fn write_tga(path: &Path, size: Size, rgba: &[u8]) -> std::io::Result<()> {
    let (w, h) = (size.w as u16, size.h as u16);
    let mut out = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&[24, 0x20]); // 24 bit, top-left origin
    for px in rgba.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0]]); // premultiplied: composited over black
    }
    std::fs::write(path, out)
}

fn run_check(renderer: &mut MiniquadRenderer, out_dir: Option<&Path>) -> bool {
    let size = SCENE;
    let n = (size.w * size.h * 4) as usize;

    renderer.set_filter(FilterMode::Nearest); // the software renderer samples nearest
    let ctx = renderer.context_mut();
    let params = TextureParams {
        width: size.w as u32,
        height: size.h as u32,
        format: TextureFormat::RGBA8,
        min_filter: FilterMode::Nearest,
        mag_filter: FilterMode::Nearest,
        ..Default::default()
    };
    let target = ctx.new_render_texture(params);
    let pass = ctx.new_render_pass(target, None);
    ctx.begin_pass(Some(pass), PassAction::clear_color(0.0, 0.0, 0.0, 0.0));
    let mut scene = Scene::new(1, true);
    {
        let mut p = Painter::new(renderer, size);
        scene.paint(&mut p);
    }
    let stats = renderer.stats();
    let ctx = renderer.context_mut();
    ctx.end_render_pass();
    let mut gpu = vec![0u8; n];
    ctx.texture_read_pixels(target, &mut gpu);
    ctx.delete_render_pass(pass);
    ctx.delete_texture(target);
    ctx.commit_frame();
    // GL render targets are stored bottom row first.
    let row = size.w as usize * 4;
    let gpu: Vec<u8> = gpu.chunks_exact(row).rev().flatten().copied().collect();

    let mut soft = SoftRenderer::new(size);
    let mut scene = Scene::new(1, true);
    {
        let mut p = Painter::new(&mut soft, size);
        scene.paint(&mut p);
    }
    let soft = soft.pixels();

    let mut bad = 0;
    let mut worst = 0;
    for (i, (g, s)) in gpu.chunks_exact(4).zip(soft.chunks_exact(4)).enumerate() {
        let d = g.iter().zip(s).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        worst = worst.max(d);
        if d > 2 {
            if bad < 10 {
                let (x, y) = (i % size.w as usize, i / size.w as usize);
                println!("  mismatch at {x},{y}: gpu {g:?} soft {s:?}");
            }
            bad += 1;
        }
    }
    println!(
        "check: {}x{}, {} GPU draw calls, {} quads, {} uploads; {bad} pixels differ by more than 2 (max difference {worst})",
        size.w, size.h, stats.draw_calls, stats.quads, stats.uploads
    );
    if let Some(dir) = out_dir {
        for (name, px) in [("gpu.tga", &gpu[..]), ("soft.tga", soft)] {
            let path = dir.join(name);
            match write_tga(&path, size, px) {
                Ok(()) => println!("wrote {}", path.display()),
                Err(e) => println!("could not write {}: {e}", path.display()),
            }
        }
    }
    bad == 0
}

// ----------------------------------------------------------------------
// The app
// ----------------------------------------------------------------------

struct Args {
    metal: bool,
    frames: Option<u64>,
    check: Option<Option<PathBuf>>,
}

fn parse_args() -> Args {
    let mut args = Args { metal: false, frames: None, check: None };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--metal" => args.metal = true,
            "--frames" => args.frames = it.next().and_then(|v| v.parse().ok()),
            "--check" => {
                let dir = it.next_if(|v| !v.starts_with("--")).map(PathBuf::from);
                args.check = Some(dir);
            }
            other => {
                eprintln!("unknown argument {other:?}; see the example's doc comment");
                std::process::exit(2);
            }
        }
    }
    args
}

struct Stage {
    renderer: MiniquadRenderer,
    ui: Ui,
    input: InputAdapter,
    scene: Scene,
    shared: Rc<RefCell<Shared>>,
    pad: WidgetId,
    tiles: Vec<WidgetId>,
    k: i32,
    start: Instant,
    frames: u64,
    max_frames: Option<u64>,
    check: Option<Option<PathBuf>>,
}

impl Stage {
    fn new(args: Args) -> Stage {
        let renderer = MiniquadRenderer::new(window::new_rendering_backend()).expect("shader");
        let info = renderer.context().info();
        println!(
            "backend {:?} {} (dpi scale {}, high dpi {})",
            info.backend,
            info.gl_version_string,
            window::dpi_scale(),
            window::high_dpi()
        );
        let k = (window::dpi_scale().round() as i32).max(1);
        let mut ui = Ui::new(Config::for_dpi(window_dpi()), Services::default());
        let shared = Rc::new(RefCell::new(Shared::default()));
        ui.add_listener(Box::new(Logger { shared: shared.clone() }));
        let root = ui.root();
        let pad = ui.add(root, Pad);
        let tiles: Vec<WidgetId> = (0..4)
            .map(|_| {
                let t = ui.add(pad, Tile);
                ui.set_focusable(t, true);
                t
            })
            .collect();
        ui.set_focus(tiles[0]);
        let mut stage = Stage {
            renderer,
            ui,
            input: InputAdapter::new(),
            scene: Scene::new(k, false),
            shared,
            pad,
            tiles,
            k,
            start: Instant::now(),
            frames: 0,
            max_frames: args.frames,
            check: args.check,
        };
        let (w, h) = window::screen_size();
        stage.resize_event(w, h);
        stage
    }

    /// Keep the Ui's clock current before every input event (click counting reads it).
    fn tick(&mut self) {
        self.ui.set_time(self.start.elapsed().as_millis() as u64);
    }

    fn layout(&mut self) {
        let k = self.k;
        let vp = self.ui.viewport();
        self.ui.set_rect(self.pad, Rect::from_size(vp));
        for (i, &t) in self.tiles.iter().enumerate() {
            self.ui.set_rect(t, Rect::new((16 + i as i32 * 112) * k, 304 * k, 96 * k, 64 * k));
        }
    }
}

impl EventHandler for Stage {
    fn update(&mut self) {
        self.tick();
        self.ui.update();
    }

    fn draw(&mut self) {
        if let Some(dir) = self.check.take() {
            let ok = run_check(&mut self.renderer, dir.as_deref());
            println!("{}", if ok { "check passed" } else { "check FAILED" });
            std::process::exit(if ok { 0 } else { 1 });
        }
        self.ui.take_repaint(); // the scene is drawn every frame anyway
        let vp = self.ui.viewport();
        self.renderer.context_mut().begin_default_pass(PassAction::clear_color(0.0, 0.0, 0.0, 1.0));
        {
            let mut p = Painter::new(&mut self.renderer, vp);
            self.scene.paint(&mut p);
            paint_input_overlay(&mut p, &self.ui, &self.tiles, &self.shared.borrow(), self.k);
        }
        let ctx = self.renderer.context_mut();
        ctx.end_render_pass();
        ctx.commit_frame();
        self.frames += 1;
        if self.frames == 1 {
            println!("first frame {}x{}: {:?}", vp.w, vp.h, self.renderer.stats());
        }
        if self.max_frames == Some(self.frames) {
            println!("{} frames drawn, quitting", self.frames);
            window::order_quit();
        }
    }

    fn resize_event(&mut self, width: f32, height: f32) {
        self.input.resize_event(&mut self.ui, width, height);
        self.layout();
    }

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        self.tick();
        self.input.mouse_motion_event(&mut self.ui, x, y);
    }

    fn mouse_wheel_event(&mut self, x: f32, y: f32) {
        self.tick();
        self.input.mouse_wheel_event(&mut self.ui, x, y);
    }

    fn mouse_button_down_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.tick();
        self.input.mouse_button_down_event(&mut self.ui, button, x, y);
    }

    fn mouse_button_up_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.tick();
        self.input.mouse_button_up_event(&mut self.ui, button, x, y);
    }

    fn char_event(&mut self, character: char, keymods: KeyMods, repeat: bool) {
        self.tick();
        self.input.char_event(&mut self.ui, character, keymods, repeat);
    }

    fn key_down_event(&mut self, keycode: KeyCode, keymods: KeyMods, repeat: bool) {
        self.tick();
        self.input.key_down_event(&mut self.ui, keycode, keymods, repeat);
    }

    fn key_up_event(&mut self, keycode: KeyCode, keymods: KeyMods) {
        self.tick();
        self.input.key_up_event(&mut self.ui, keycode, keymods);
    }

    fn touch_event(&mut self, phase: TouchPhase, id: u64, x: f32, y: f32) {
        self.tick();
        self.input.touch_event(&mut self.ui, phase, id, x, y);
    }

    fn files_dropped_event(&mut self) {
        self.input.files_dropped_event(&mut self.ui);
    }

    fn window_minimized_event(&mut self) {
        self.input.focus_lost(&mut self.ui);
    }
}

fn main() {
    let args = parse_args();
    if args.metal && args.check.is_some() {
        eprintln!("--check reads the render target back, which miniquad's Metal backend does not implement");
        std::process::exit(2);
    }
    let mut conf = Conf {
        window_title: "Turbo Ferret: miniquad painter".to_string(),
        window_width: 960,
        window_height: 640,
        high_dpi: true,
        ..Default::default()
    };
    if args.metal {
        conf.platform.apple_gfx_api = AppleGfxApi::Metal;
    }
    miniquad::start(conf, move || Box::new(Stage::new(args)));
}
