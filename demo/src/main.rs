//! The Turbo Ferret demo in a window (miniquad).
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author.
//!
//! Options: `--frames N` quits after N frames (smoke test).

use std::time::Instant;

use miniquad::conf::Conf;
use miniquad::{EventHandler, KeyCode, KeyMods, MouseButton, PassAction, TouchPhase, conf, window};
use turbo_ferret::geom::Size;
use turbo_ferret_demo::{Demo, default_resource_root};
use turbo_ferret_miniquad::{InputAdapter, MiniquadRenderer, window_dpi};

struct Stage {
    demo: Demo,
    renderer: MiniquadRenderer,
    input: InputAdapter,
    start: Instant,
    frames: u64,
    max_frames: Option<u64>,
}

impl Stage {
    fn new(max_frames: Option<u64>) -> Stage {
        let renderer = MiniquadRenderer::new(window::new_rendering_backend()).expect("shader compiles");
        let (w, h) = window::screen_size();
        let demo = Demo::new(&default_resource_root(), Size::new(w as i32, h as i32), window_dpi());
        Stage { demo, renderer, input: InputAdapter::new(), start: Instant::now(), frames: 0, max_frames }
    }

    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// The UI's clock is data: keep it current before input (click counting reads it).
    fn tick(&mut self) {
        let t = self.now();
        self.demo.ui.set_time(t);
    }
}

impl EventHandler for Stage {
    fn update(&mut self) {
        let t = self.now();
        self.demo.frame(t);
    }

    fn draw(&mut self) {
        // miniquad redraws every frame; the UI's repaint flag only matters for hosts
        // that sleep until `next_wake_time`.
        self.renderer.context_mut().begin_default_pass(PassAction::clear_color(0.0, 0.0, 0.0, 1.0));
        self.demo.ui.paint(&mut self.renderer);
        let ctx = self.renderer.context_mut();
        ctx.end_render_pass();
        ctx.commit_frame();
        self.frames += 1;
        if self.frames == 1 {
            let vp = self.demo.ui.viewport();
            println!("first frame {}x{} at {} dpi: {:?}", vp.w, vp.h, window_dpi(), self.renderer.stats());
        }
        if self.max_frames == Some(self.frames) {
            println!("{} frames drawn, last frame: {:?}; quitting", self.frames, self.renderer.stats());
            window::order_quit();
        }
    }

    fn resize_event(&mut self, width: f32, height: f32) {
        self.input.resize_event(&mut self.demo.ui, width, height);
    }

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        self.tick();
        self.input.mouse_motion_event(&mut self.demo.ui, x, y);
    }

    fn mouse_wheel_event(&mut self, x: f32, y: f32) {
        self.tick();
        self.input.mouse_wheel_event(&mut self.demo.ui, x, y);
    }

    fn mouse_button_down_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.tick();
        self.input.mouse_button_down_event(&mut self.demo.ui, button, x, y);
    }

    fn mouse_button_up_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.tick();
        self.input.mouse_button_up_event(&mut self.demo.ui, button, x, y);
    }

    fn char_event(&mut self, character: char, keymods: KeyMods, repeat: bool) {
        self.tick();
        self.input.char_event(&mut self.demo.ui, character, keymods, repeat);
    }

    fn key_down_event(&mut self, keycode: KeyCode, keymods: KeyMods, repeat: bool) {
        self.tick();
        self.input.key_down_event(&mut self.demo.ui, keycode, keymods, repeat);
    }

    fn key_up_event(&mut self, keycode: KeyCode, keymods: KeyMods) {
        self.tick();
        self.input.key_up_event(&mut self.demo.ui, keycode, keymods);
    }

    fn touch_event(&mut self, phase: TouchPhase, id: u64, x: f32, y: f32) {
        self.tick();
        self.input.touch_event(&mut self.demo.ui, phase, id, x, y);
    }

    fn files_dropped_event(&mut self) {
        self.input.files_dropped_event(&mut self.demo.ui);
    }

    fn window_minimized_event(&mut self) {
        self.input.focus_lost(&mut self.demo.ui);
    }
}

fn main() {
    let mut max_frames = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--frames" {
            max_frames = args.next().and_then(|n| n.parse().ok());
        }
    }
    let conf = Conf {
        window_title: "Turbo Ferret demo".to_string(),
        window_width: 1280,
        window_height: 800,
        high_dpi: true,
        platform: conf::Platform::default(),
        ..Default::default()
    };
    miniquad::start(conf, move || Box::new(Stage::new(max_frames)));
}
