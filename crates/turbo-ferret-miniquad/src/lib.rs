//! miniquad backend for Turbo Ferret: a [`Renderer`](turbo_ferret::Renderer) on
//! miniquad's `RenderingBackend` (OpenGL 2.1+/3, GLES 2+, WebGL 1, Metal) and an
//! adapter from miniquad's input events to `Ui` calls.
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author.
//!
//! # Using it
//!
//! ```ignore
//! struct App { ui: Ui, renderer: MiniquadRenderer, input: InputAdapter }
//!
//! // In miniquad::start's closure:
//! let renderer = MiniquadRenderer::new(miniquad::window::new_rendering_backend())?;
//! let mut ui = Ui::new(Config::for_dpi(window_dpi()), Services::default());
//! let (w, h) = miniquad::window::screen_size();
//! input.resize_event(&mut ui, w, h);
//!
//! impl EventHandler for App {
//!     fn mouse_motion_event(&mut self, x: f32, y: f32) { self.input.mouse_motion_event(&mut self.ui, x, y) }
//!     fn char_event(&mut self, c: char, m: KeyMods, r: bool) { self.input.char_event(&mut self.ui, c, m, r); }
//!     // ... one line per callback ...
//! }
//! ```
//!
//! See `examples/painter.rs` for a complete app.
//!
//! # HiDPI: the UI works in framebuffer pixels
//!
//! miniquad reports pointer and touch positions, `resize_event` sizes and
//! `window::screen_size()` in framebuffer (physical) pixels on every platform, so
//! the `Ui` uses those units too: [`InputAdapter`] passes coordinates through
//! unchanged and `resize_event` sets the viewport to the framebuffer size, and
//! [`MiniquadRenderer`] maps one unit to one framebuffer pixel. Nothing is scaled,
//! so text and bitmaps are pixel exact. To make the UI as large as on a 96 DPI
//! screen, tell the core the DPI with `Config::for_dpi(window_dpi())`, which scales
//! TB's DPI-dependent metrics (pan threshold, pixels per wheel line, skin selection
//! by DPI). Set `Conf::high_dpi = true` to get a full-resolution framebuffer.
//!
//! # Filtering
//!
//! Linear with clamp-to-edge (TB's GL renderer uses linear too). Quads map texel
//! centers exactly at 1:1, so unscaled blits are not blurred; see
//! [`MiniquadRenderer::set_filter`] for nearest.

#![forbid(unsafe_code)]

mod input;
mod renderer;

pub use input::{
    ClickCounter, InputAdapter, SHORTCUT_MODIFIER, WheelConverter, key_char, modifiers, shortcut_for, special_key,
};
pub use renderer::{FrameStats, MAX_QUADS, MiniquadRenderer};

/// The window's DPI for `turbo_ferret::Config::for_dpi`: 96 × miniquad's
/// `window::dpi_scale()`. Call it once miniquad is running (inside
/// `miniquad::start`'s closure or later).
pub fn window_dpi() -> i32 {
    let scale = miniquad::window::dpi_scale();
    if scale.is_finite() && scale > 0.0 { (96.0 * scale).round() as i32 } else { 96 }
}
