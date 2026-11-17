//! [`InputAdapter`]: miniquad input events to `Ui` calls.
//!
//! The app's `miniquad::EventHandler` forwards each callback to the method of the
//! same name, passing its `Ui`. Coordinates are used as miniquad reports them:
//! framebuffer pixels on every platform, the same units as
//! `miniquad::window::screen_size()` (see the crate docs on HiDPI).
//!
//! TB: Demo/platform/port_glfw.cpp (key_callback, char_callback, InvokeShortcut,
//! mouse_button_callback, scroll_callback, cursor_position_callback, drop_callback)

use miniquad::{KeyCode, KeyMods, MouseButton, TouchPhase};
use turbo_ferret::event::{Event, EventKind, Modifiers, SpecialKey};
use turbo_ferret::geom::{Point, Size};
use turbo_ferret::msg::Millis;
use turbo_ferret::{TbId, Ui, tbid};

/// The modifier that turns keys into shortcuts: Cmd on Apple platforms, Ctrl
/// elsewhere. TB: InvokeShortcut (TB_SUPER on TB_TARGET_MACOSX, else TB_CTRL)
pub const SHORTCUT_MODIFIER: Modifiers =
    if cfg!(any(target_os = "macos", target_os = "ios")) { Modifiers::SUPER } else { Modifiers::CTRL };

/// TB's `SPECIAL_KEY` for a miniquad key, or `SpecialKey::None` for character keys,
/// modifiers and keys TB has no name for. TB: key_callback
pub fn special_key(key: KeyCode) -> SpecialKey {
    use SpecialKey as S;
    match key {
        KeyCode::Up => S::Up,
        KeyCode::Down => S::Down,
        KeyCode::Left => S::Left,
        KeyCode::Right => S::Right,
        KeyCode::PageUp => S::PageUp,
        KeyCode::PageDown => S::PageDown,
        KeyCode::Home => S::Home,
        KeyCode::End => S::End,
        KeyCode::Tab => S::Tab,
        KeyCode::Backspace => S::Backspace,
        KeyCode::Insert => S::Insert,
        KeyCode::Delete => S::Delete,
        KeyCode::Enter | KeyCode::KpEnter => S::Enter,
        KeyCode::Escape => S::Esc,
        KeyCode::F1 => S::F1,
        KeyCode::F2 => S::F2,
        KeyCode::F3 => S::F3,
        KeyCode::F4 => S::F4,
        KeyCode::F5 => S::F5,
        KeyCode::F6 => S::F6,
        KeyCode::F7 => S::F7,
        KeyCode::F8 => S::F8,
        KeyCode::F9 => S::F9,
        KeyCode::F10 => S::F10,
        KeyCode::F11 => S::F11,
        KeyCode::F12 => S::F12,
        _ => S::None,
    }
}

/// The ASCII character a printable key produces without Shift, with letters in upper
/// case (TB sends GLFW key codes here, which are upper-case ASCII). Only used for
/// keys pressed with Ctrl or Cmd, which produce no usable `char_event`.
pub fn key_char(key: KeyCode) -> Option<char> {
    let c = match key {
        KeyCode::Kp0 => '0',
        KeyCode::Kp1 => '1',
        KeyCode::Kp2 => '2',
        KeyCode::Kp3 => '3',
        KeyCode::Kp4 => '4',
        KeyCode::Kp5 => '5',
        KeyCode::Kp6 => '6',
        KeyCode::Kp7 => '7',
        KeyCode::Kp8 => '8',
        KeyCode::Kp9 => '9',
        KeyCode::KpDecimal => '.',
        KeyCode::KpDivide => '/',
        KeyCode::KpMultiply => '*',
        KeyCode::KpSubtract => '-',
        KeyCode::KpAdd => '+',
        KeyCode::KpEqual => '=',
        // miniquad's key codes are X11 keysyms: printable keys are their ASCII code.
        k => {
            let v = k as u16;
            return (0x20..0x7f).contains(&v).then_some(v as u8 as char);
        }
    };
    Some(c)
}

/// TB's modifiers for miniquad's. TB: GetModifierKeys
pub fn modifiers(m: KeyMods) -> Modifiers {
    let mut r = Modifiers::NONE;
    r.set(Modifiers::SHIFT, m.shift);
    r.set(Modifiers::CTRL, m.ctrl);
    r.set(Modifiers::ALT, m.alt);
    r.set(Modifiers::SUPER, m.logo);
    r
}

fn modifier_of(key: KeyCode) -> Option<Modifiers> {
    match key {
        KeyCode::LeftShift | KeyCode::RightShift => Some(Modifiers::SHIFT),
        KeyCode::LeftControl | KeyCode::RightControl => Some(Modifiers::CTRL),
        KeyCode::LeftAlt | KeyCode::RightAlt => Some(Modifiers::ALT),
        KeyCode::LeftSuper | KeyCode::RightSuper => Some(Modifiers::SUPER),
        _ => None,
    }
}

/// The shortcut command for a key press, as TB's demo maps them: `cut`, `copy`,
/// `paste`, `selectall`, `undo`, `redo`, `new`, `open`, `save`, `close`, `prev_doc`,
/// `next_doc`. Requires `shortcut_mod` (see [`SHORTCUT_MODIFIER`]); Shift reverses
/// undo and redo. `ch` may be either case.
///
/// One change from TB: Shortcut+Shift+Insert is `paste`. TB tests Insert for `copy`
/// first, which leaves its `paste` case unreachable. TB: InvokeShortcut
pub fn shortcut_for(ch: Option<char>, special: SpecialKey, mods: Modifiers, shortcut_mod: Modifiers) -> Option<TbId> {
    if shortcut_mod.is_empty() || !mods.contains(shortcut_mod) {
        return None;
    }
    let reverse = mods.contains(Modifiers::SHIFT);
    let id = match (ch.map(|c| c.to_ascii_uppercase()), special) {
        (Some('X'), _) => tbid!("cut"),
        (Some('C'), _) => tbid!("copy"),
        (_, SpecialKey::Insert) if reverse => tbid!("paste"),
        (_, SpecialKey::Insert) => tbid!("copy"),
        (Some('V'), _) => tbid!("paste"),
        (Some('A'), _) => tbid!("selectall"),
        (Some(c @ ('Z' | 'Y')), _) => {
            if (c == 'Z') != reverse {
                tbid!("undo")
            } else {
                tbid!("redo")
            }
        }
        (Some('N'), _) => tbid!("new"),
        (Some('O'), _) => tbid!("open"),
        (Some('S'), _) => tbid!("save"),
        (Some('W'), _) => tbid!("close"),
        (_, SpecialKey::PageUp) => tbid!("prev_doc"),
        (_, SpecialKey::PageDown) => tbid!("next_doc"),
        _ => return None,
    };
    Some(id)
}

/// True for characters a `char_event` should not deliver: control characters (Enter,
/// Tab, Backspace, Escape and Ctrl+letter arrive this way on some platforms, and the
/// key events already cover them) and the private use block (macOS sends arrows and
/// function keys there). TB: char_callback
fn is_unprintable(ch: char) -> bool {
    ch.is_control() || ('\u{e000}'..='\u{f8ff}').contains(&ch)
}

/// Ctrl or Cmd without Alt: the key events deliver the key, and `char_event` must
/// not deliver it again. Ctrl+Alt is AltGr on Windows, which types characters.
/// TB: key_callback (default case)
fn is_command_chord(mods: Modifiers) -> bool {
    mods.intersects(Modifiers::CTRL | Modifiers::SUPER) && !mods.contains(Modifiers::ALT)
}

/// Counts consecutive presses for double and triple clicks: a press within
/// `interval_ms` of the previous one and within `max_distance` pixels of it (on each
/// axis) continues the series. The count keeps growing (`Event::count_cycle` wraps).
/// TB: mouse_button_callback ("n-click support"; the demo uses 600 ms and no slack).
#[derive(Copy, Clone, Debug)]
pub struct ClickCounter {
    pub interval_ms: Millis,
    pub max_distance: i32,
    last: Option<(Millis, Point)>,
    count: u32,
}

impl Default for ClickCounter {
    fn default() -> Self {
        ClickCounter::new(300, 4)
    }
}

impl ClickCounter {
    pub fn new(interval_ms: Millis, max_distance: i32) -> ClickCounter {
        ClickCounter { interval_ms, max_distance, last: None, count: 0 }
    }

    /// Register a press at time `now` and position `p`; returns its click count (1 =
    /// single).
    pub fn press(&mut self, now: Millis, p: Point) -> u32 {
        let continues = self.last.is_some_and(|(t, q)| {
            now >= t
                && now - t < self.interval_ms
                && (p.x - q.x).abs() <= self.max_distance
                && (p.y - q.y).abs() <= self.max_distance
        });
        self.count = if continues { self.count.saturating_add(1) } else { 1 };
        self.last = Some((now, p));
        self.count
    }

    /// Forget the series (the next press counts 1).
    pub fn reset(&mut self) {
        self.last = None;
        self.count = 0;
    }
}

/// Converts miniquad wheel deltas into TB wheel steps. miniquad passes the platform's
/// raw units with positive y = away from the user (scroll up); TB wants whole steps
/// with positive y = scroll down (TB's demo passes `-y`), and positive x = scroll
/// right. Fractions (trackpads) accumulate until they make a step.
///
/// Units per step by default: Windows 120 (`WHEEL_DELTA`), macOS 10 (miniquad scales
/// notched wheels by 10; trackpads send points), the web 100 (DOM pixels per notch in
/// most browsers), elsewhere 1 (X11 sends one unit per notch). miniquad's Wayland
/// backend sends about 10 per notch: call [`set_units_per_step`] if you enable it.
///
/// [`set_units_per_step`]: WheelConverter::set_units_per_step
#[derive(Copy, Clone, Debug)]
pub struct WheelConverter {
    units_per_step: f32,
    /// Sign applied to miniquad's x so that positive is "scroll right".
    x_sign: f32,
    acc: (f32, f32),
}

impl Default for WheelConverter {
    fn default() -> Self {
        WheelConverter::for_platform()
    }
}

impl WheelConverter {
    /// `units_per_step` raw units make one step. Positive raw x is taken to mean
    /// "scroll left", as on macOS, X11 and the web.
    pub fn new(units_per_step: f32) -> WheelConverter {
        let mut w = WheelConverter { units_per_step: 1.0, x_sign: -1.0, acc: (0.0, 0.0) };
        w.set_units_per_step(units_per_step);
        w
    }

    /// The defaults for the target platform (see the type docs).
    pub fn for_platform() -> WheelConverter {
        let units = if cfg!(target_os = "windows") {
            120.0
        } else if cfg!(target_os = "macos") {
            10.0
        } else if cfg!(target_arch = "wasm32") {
            100.0
        } else {
            1.0
        };
        let mut w = WheelConverter::new(units);
        // Windows reports WM_MOUSEHWHEEL as is: positive = tilted right.
        if cfg!(target_os = "windows") {
            w.x_sign = 1.0;
        }
        w
    }

    pub fn units_per_step(&self) -> f32 {
        self.units_per_step
    }

    pub fn set_units_per_step(&mut self, units: f32) {
        self.units_per_step = if units.is_finite() && units > 0.0 { units } else { 1.0 };
        self.acc = (0.0, 0.0);
    }

    /// Steps for one miniquad wheel event (zero until enough units accumulate).
    pub fn convert(&mut self, dx: f32, dy: f32) -> Point {
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        self.acc.0 += finite(dx) * self.x_sign / self.units_per_step;
        self.acc.1 += -finite(dy) / self.units_per_step;
        let steps = (self.acc.0.trunc(), self.acc.1.trunc());
        self.acc.0 -= steps.0;
        self.acc.1 -= steps.1;
        Point::new(steps.0 as i32, steps.1 as i32)
    }
}

/// Maps miniquad touch ids (arbitrary `u64`s) to small `Ui` touch indices: each new
/// touch gets the lowest free index, so the first finger is touch 0, which `Ui`
/// routes through the pointer path.
#[derive(Clone, Debug, Default)]
struct TouchIds {
    slots: Vec<Option<u64>>,
}

impl TouchIds {
    fn start(&mut self, id: u64) -> u32 {
        if let Some(i) = self.get(id) {
            return i;
        }
        let i = match self.slots.iter().position(Option::is_none) {
            Some(i) => i,
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        self.slots[i] = Some(id);
        i as u32
    }

    fn get(&self, id: u64) -> Option<u32> {
        self.slots.iter().position(|s| *s == Some(id)).map(|i| i as u32)
    }

    fn end(&mut self, id: u64) -> Option<u32> {
        let i = self.get(id)?;
        self.slots[i as usize] = None;
        Some(i)
    }
}

/// Forwards miniquad input to a [`Ui`].
///
/// Call each method from the `miniquad::EventHandler` callback of the same name.
/// Methods that return `bool` return whether the UI handled the input, so the rest
/// of the app can take what the UI did not.
///
/// Click counting uses `ui.now()`, so keep the `Ui`'s time current (call
/// `ui.set_time` before forwarding, or at least every frame).
#[derive(Clone, Debug)]
pub struct InputAdapter {
    mods: Modifiers,
    pointer: Point,
    clicks: ClickCounter,
    wheel: WheelConverter,
    shortcut_mod: Modifiers,
    touches: TouchIds,
    /// Character keys delivered through the key events (Ctrl/Cmd chords): their key-up
    /// is delivered even if the modifier is released first.
    chord_keys: Vec<(KeyCode, char)>,
}

impl Default for InputAdapter {
    fn default() -> Self {
        InputAdapter::new()
    }
}

/// miniquad's float coordinates as whole pixels.
fn to_point(x: f32, y: f32) -> Point {
    let c = |v: f32| if v.is_finite() { v.floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32 } else { 0 };
    Point::new(c(x), c(y))
}

impl InputAdapter {
    /// Defaults: 300 ms / 4 px double-click window, the platform's wheel units and
    /// shortcut modifier.
    pub fn new() -> InputAdapter {
        InputAdapter {
            mods: Modifiers::NONE,
            pointer: Point::ZERO,
            clicks: ClickCounter::default(),
            wheel: WheelConverter::for_platform(),
            shortcut_mod: SHORTCUT_MODIFIER,
            touches: TouchIds::default(),
            chord_keys: Vec::new(),
        }
    }

    /// The modifiers held, as last seen in a key or char event (miniquad's mouse
    /// events carry none).
    pub fn modifiers(&self) -> Modifiers {
        self.mods
    }

    /// The last pointer position (framebuffer pixels).
    pub fn pointer(&self) -> Point {
        self.pointer
    }

    pub fn click_counter_mut(&mut self) -> &mut ClickCounter {
        &mut self.clicks
    }

    pub fn wheel_converter_mut(&mut self) -> &mut WheelConverter {
        &mut self.wheel
    }

    /// The modifier that makes keys shortcuts (default [`SHORTCUT_MODIFIER`]).
    pub fn set_shortcut_modifier(&mut self, m: Modifiers) {
        self.shortcut_mod = m;
    }

    /// `ui.set_viewport` with the framebuffer size miniquad reports.
    pub fn resize_event(&mut self, ui: &mut Ui, width: f32, height: f32) {
        let p = to_point(width.round(), height.round());
        ui.set_viewport(Size::new(p.x.max(0), p.y.max(0)));
    }

    /// TB: cursor_position_callback
    pub fn mouse_motion_event(&mut self, ui: &mut Ui, x: f32, y: f32) {
        self.pointer = to_point(x, y);
        ui.pointer_move(self.pointer, self.mods);
    }

    /// Left button: pointer down with a click count. Others: nothing (the right
    /// button acts on release). TB: mouse_button_callback
    pub fn mouse_button_down_event(&mut self, ui: &mut Ui, button: MouseButton, x: f32, y: f32) -> bool {
        self.pointer = to_point(x, y);
        if button != MouseButton::Left {
            return false;
        }
        let clicks = self.clicks.press(ui.now(), self.pointer);
        ui.pointer_down(self.pointer, clicks, self.mods)
    }

    /// Left button: pointer up. Right button: a move to the position, then
    /// `ContextMenu` to the hovered widget, as TB's demo does.
    /// TB: mouse_button_callback
    pub fn mouse_button_up_event(&mut self, ui: &mut Ui, button: MouseButton, x: f32, y: f32) -> bool {
        self.pointer = to_point(x, y);
        match button {
            MouseButton::Left => ui.pointer_up(self.pointer, self.mods),
            MouseButton::Right => {
                ui.pointer_move(self.pointer, self.mods);
                let Some(h) = ui.hovered() else { return false };
                let mut ev = Event::new(EventKind::ContextMenu, h).at(ui.from_root(h, self.pointer));
                ev.mods = self.mods;
                ui.invoke_event(ev)
            }
            _ => false,
        }
    }

    /// Wheel at the last pointer position, in steps (see [`WheelConverter`]).
    /// TB: scroll_callback
    pub fn mouse_wheel_event(&mut self, ui: &mut Ui, dx: f32, dy: f32) -> bool {
        let steps = self.wheel.convert(dx, dy);
        if steps == Point::ZERO {
            return false;
        }
        ui.wheel(self.pointer, steps, self.mods)
    }

    /// Special keys (arrows, Enter, F1 ...) go to `ui.key` with `SpecialKey`; character
    /// keys only when Ctrl or Cmd is held (no `char_event` delivers those), with the
    /// key's ASCII character. Either may become a shortcut first. Menu opens the
    /// focused widget's context menu on release. TB: key_callback
    pub fn key_down_event(&mut self, ui: &mut Ui, key: KeyCode, mods: KeyMods, _repeat: bool) -> bool {
        self.key_event(ui, key, mods, true)
    }

    pub fn key_up_event(&mut self, ui: &mut Ui, key: KeyCode, mods: KeyMods) -> bool {
        self.key_event(ui, key, mods, false)
    }

    fn key_event(&mut self, ui: &mut Ui, key: KeyCode, mods: KeyMods, down: bool) -> bool {
        self.mods = modifiers(mods);
        if let Some(m) = modifier_of(key) {
            // Platforms differ on whether a modifier's own event includes it.
            self.mods.set(m, down);
            return false;
        }
        if key == KeyCode::Menu {
            let Some(f) = ui.focused().filter(|_| !down) else { return false };
            let mut ev = Event::new(EventKind::ContextMenu, f);
            ev.mods = self.mods;
            return ui.invoke_event(ev);
        }
        let special = special_key(key);
        if special != SpecialKey::None {
            return self.invoke_key(ui, None, special, down);
        }
        if down {
            let Some(ch) = key_char(key).filter(|_| is_command_chord(self.mods)) else { return false };
            if !self.chord_keys.iter().any(|&(k, _)| k == key) {
                self.chord_keys.push((key, ch));
            }
            self.invoke_key(ui, Some(ch), SpecialKey::None, true)
        } else {
            let Some(i) = self.chord_keys.iter().position(|&(k, _)| k == key) else { return false };
            let (_, ch) = self.chord_keys.swap_remove(i);
            self.invoke_key(ui, Some(ch), SpecialKey::None, false)
        }
    }

    /// A typed character: delivered as key down and key up at once, as TB's demo
    /// does. Control and private-use characters, and characters typed with Ctrl or
    /// Cmd (but not Alt), are dropped: the key events deliver those keys, so nothing
    /// arrives twice. TB: char_callback
    pub fn char_event(&mut self, ui: &mut Ui, ch: char, mods: KeyMods, _repeat: bool) -> bool {
        self.mods = modifiers(mods);
        if is_unprintable(ch) || is_command_chord(self.mods) {
            return false;
        }
        let handled = self.invoke_key(ui, Some(ch), SpecialKey::None, true);
        self.invoke_key(ui, Some(ch), SpecialKey::None, false) || handled
    }

    /// Shortcut first (on press), then the key. TB: InvokeKey
    fn invoke_key(&mut self, ui: &mut Ui, ch: Option<char>, special: SpecialKey, down: bool) -> bool {
        if down {
            if let Some(id) = shortcut_for(ch, special, self.mods, self.shortcut_mod) {
                if ui.shortcut(id, self.mods) {
                    return true;
                }
            }
        }
        ui.key(ch, special, self.mods, down)
    }

    /// Touches, with the first finger as touch 0 (the pointer path, with click
    /// counting) and further fingers as touches 1, 2 ... Overriding
    /// `EventHandler::touch_event` to call this replaces miniquad's default, which
    /// emulates the mouse.
    pub fn touch_event(&mut self, ui: &mut Ui, phase: TouchPhase, id: u64, x: f32, y: f32) -> bool {
        let p = to_point(x, y);
        match phase {
            TouchPhase::Started => {
                let t = self.touches.start(id);
                let clicks = if t == 0 { self.clicks.press(ui.now(), p) } else { 1 };
                ui.touch_down(t, p, clicks, self.mods)
            }
            TouchPhase::Moved => {
                let Some(t) = self.touches.get(id) else { return false };
                ui.touch_move(t, p, self.mods);
                true
            }
            TouchPhase::Ended => {
                let Some(t) = self.touches.end(id) else { return false };
                ui.touch_up(t, p, self.mods)
            }
            TouchPhase::Cancelled => {
                let Some(t) = self.touches.end(id) else { return false };
                ui.touch_cancel(t);
                true
            }
        }
    }

    /// Files dropped on the window, delivered at the pointer position. Reads the
    /// paths from `miniquad::window` (needs a running miniquad app).
    /// TB: drop_callback
    pub fn files_dropped_event(&mut self, ui: &mut Ui) -> bool {
        let n = miniquad::window::dropped_file_count();
        let files: Vec<String> =
            (0..n).filter_map(miniquad::window::dropped_file_path).map(|p| p.to_string_lossy().into_owned()).collect();
        if files.is_empty() {
            return false;
        }
        ui.file_drop(self.pointer, files)
    }

    /// The window lost focus: forget held modifiers and chord keys, cancel a press
    /// and touches still down (their release events may never arrive). miniquad sends
    /// `window_minimized_event` when a desktop window loses focus; call it from there.
    pub fn focus_lost(&mut self, ui: &mut Ui) {
        self.mods = Modifiers::NONE;
        self.chord_keys.clear();
        self.clicks.reset();
        if ui.captured().is_some() {
            ui.pointer_cancel();
        }
        for (t, slot) in self.touches.slots.iter_mut().enumerate() {
            if slot.take().is_some() && t != 0 {
                ui.touch_cancel(t as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use turbo_ferret::geom::Rect;
    use turbo_ferret::{Config, Ctx, MemLoader, Services, Widget, WidgetId};

    const NONE: KeyMods = KeyMods { shift: false, ctrl: false, alt: false, logo: false };
    const SHIFT: KeyMods = KeyMods { shift: true, ctrl: false, alt: false, logo: false };
    const CTRL: KeyMods = KeyMods { shift: false, ctrl: true, alt: false, logo: false };
    const CMD: KeyMods = KeyMods { shift: false, ctrl: false, alt: false, logo: true };
    const CTRL_ALT: KeyMods = KeyMods { shift: false, ctrl: true, alt: true, logo: false };

    #[test]
    fn special_key_table() {
        use SpecialKey as S;
        let table = [
            (KeyCode::Up, S::Up),
            (KeyCode::Down, S::Down),
            (KeyCode::Left, S::Left),
            (KeyCode::Right, S::Right),
            (KeyCode::PageUp, S::PageUp),
            (KeyCode::PageDown, S::PageDown),
            (KeyCode::Home, S::Home),
            (KeyCode::End, S::End),
            (KeyCode::Tab, S::Tab),
            (KeyCode::Backspace, S::Backspace),
            (KeyCode::Insert, S::Insert),
            (KeyCode::Delete, S::Delete),
            (KeyCode::Enter, S::Enter),
            (KeyCode::KpEnter, S::Enter),
            (KeyCode::Escape, S::Esc),
            (KeyCode::F1, S::F1),
            (KeyCode::F2, S::F2),
            (KeyCode::F3, S::F3),
            (KeyCode::F4, S::F4),
            (KeyCode::F5, S::F5),
            (KeyCode::F6, S::F6),
            (KeyCode::F7, S::F7),
            (KeyCode::F8, S::F8),
            (KeyCode::F9, S::F9),
            (KeyCode::F10, S::F10),
            (KeyCode::F11, S::F11),
            (KeyCode::F12, S::F12),
        ];
        for (k, s) in table {
            assert_eq!(special_key(k), s, "{k:?}");
            assert_eq!(key_char(k), None, "{k:?}");
        }
        for k in [KeyCode::A, KeyCode::Key1, KeyCode::Space, KeyCode::LeftShift, KeyCode::F13, KeyCode::Menu] {
            assert_eq!(special_key(k), S::None, "{k:?}");
        }
    }

    #[test]
    fn key_char_table() {
        assert_eq!(key_char(KeyCode::A), Some('A'));
        assert_eq!(key_char(KeyCode::Z), Some('Z'));
        assert_eq!(key_char(KeyCode::Key0), Some('0'));
        assert_eq!(key_char(KeyCode::Space), Some(' '));
        assert_eq!(key_char(KeyCode::Slash), Some('/'));
        assert_eq!(key_char(KeyCode::GraveAccent), Some('`'));
        assert_eq!(key_char(KeyCode::Kp7), Some('7'));
        assert_eq!(key_char(KeyCode::KpAdd), Some('+'));
        assert_eq!(key_char(KeyCode::World1), None);
        assert_eq!(key_char(KeyCode::LeftControl), None);
        assert_eq!(key_char(KeyCode::Unknown), None);
    }

    #[test]
    fn modifier_mapping() {
        assert_eq!(modifiers(NONE), Modifiers::NONE);
        assert_eq!(modifiers(SHIFT), Modifiers::SHIFT);
        assert_eq!(modifiers(CMD), Modifiers::SUPER);
        let all = KeyMods { shift: true, ctrl: true, alt: true, logo: true };
        assert_eq!(modifiers(all), Modifiers::SHIFT | Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER);
        assert_eq!(modifiers(CTRL_ALT), Modifiers::CTRL | Modifiers::ALT);
    }

    #[test]
    fn shortcut_table() {
        let c = Modifiers::CTRL;
        let cs = Modifiers::CTRL | Modifiers::SHIFT;
        let sc = |ch: char, m: Modifiers| shortcut_for(Some(ch), SpecialKey::None, m, Modifiers::CTRL);
        assert_eq!(sc('x', c), Some(tbid!("cut")));
        assert_eq!(sc('C', c), Some(tbid!("copy")));
        assert_eq!(sc('v', c), Some(tbid!("paste")));
        assert_eq!(sc('a', c), Some(tbid!("selectall")));
        assert_eq!(sc('z', c), Some(tbid!("undo")));
        assert_eq!(sc('y', c), Some(tbid!("redo")));
        assert_eq!(sc('z', cs), Some(tbid!("redo")));
        assert_eq!(sc('y', cs), Some(tbid!("undo")));
        assert_eq!(sc('n', c), Some(tbid!("new")));
        assert_eq!(sc('o', c), Some(tbid!("open")));
        assert_eq!(sc('s', c), Some(tbid!("save")));
        assert_eq!(sc('w', c), Some(tbid!("close")));
        assert_eq!(sc('q', c), None);
        let sp = |s: SpecialKey, m: Modifiers| shortcut_for(None, s, m, Modifiers::CTRL);
        assert_eq!(sp(SpecialKey::Insert, c), Some(tbid!("copy")));
        assert_eq!(sp(SpecialKey::Insert, cs), Some(tbid!("paste")));
        assert_eq!(sp(SpecialKey::PageUp, c), Some(tbid!("prev_doc")));
        assert_eq!(sp(SpecialKey::PageDown, c), Some(tbid!("next_doc")));
        assert_eq!(sp(SpecialKey::Left, c), None);
        // The shortcut modifier is required; others may be added.
        assert_eq!(sc('c', Modifiers::NONE), None);
        assert_eq!(sc('c', Modifiers::SHIFT), None);
        assert_eq!(sc('c', Modifiers::SUPER), None);
        assert_eq!(sc('c', Modifiers::CTRL | Modifiers::ALT), Some(tbid!("copy")));
        assert_eq!(shortcut_for(Some('c'), SpecialKey::None, Modifiers::SUPER, Modifiers::SUPER), Some(tbid!("copy")));
        assert_eq!(shortcut_for(Some('c'), SpecialKey::None, Modifiers::CTRL, Modifiers::NONE), None);
    }

    #[test]
    fn click_counting() {
        let mut c = ClickCounter::default();
        let p = Point::new(100, 100);
        assert_eq!(c.press(1000, p), 1);
        assert_eq!(c.press(1100, p), 2);
        assert_eq!(c.press(1399, Point::new(104, 96)), 3); // 299 ms, 4 px: still the series
        assert_eq!(c.press(1500, p), 4); // the window is from the previous press
        assert_eq!(c.press(1800, p), 1); // 300 ms: too slow
        assert_eq!(c.press(1900, Point::new(105, 100)), 1); // 5 px: too far
        assert_eq!(c.press(2000, Point::new(109, 104)), 2); // 4 px on each axis
        assert_eq!(c.press(1000, Point::new(109, 104)), 1); // time went backwards
        c.reset();
        assert_eq!(c.press(1001, Point::new(105, 105)), 1);
        let mut tb = ClickCounter::new(600, 0); // TB's demo
        assert_eq!(tb.press(0, p), 1);
        assert_eq!(tb.press(599, p), 2);
        assert_eq!(tb.press(600, Point::new(101, 100)), 1);
    }

    #[test]
    fn wheel_steps() {
        let mut w = WheelConverter::new(120.0);
        // Away from the user (positive in miniquad) scrolls up: negative TB y.
        assert_eq!(w.convert(0.0, 120.0), Point::new(0, -1));
        assert_eq!(w.convert(0.0, -240.0), Point::new(0, 2));
        // Fractions accumulate.
        assert_eq!(w.convert(0.0, -60.0), Point::ZERO);
        assert_eq!(w.convert(0.0, -60.0), Point::new(0, 1));
        // Positive raw x is "scroll left".
        assert_eq!(w.convert(-120.0, 0.0), Point::new(1, 0));
        assert_eq!(w.convert(120.0, 0.0), Point::new(-1, 0));
        let mut w = WheelConverter::new(10.0);
        assert_eq!(w.convert(0.0, -4.0), Point::ZERO);
        assert_eq!(w.convert(0.0, -4.0), Point::ZERO);
        assert_eq!(w.convert(0.0, -4.0), Point::new(0, 1)); // 12 units: 1 step, 2 left over
        assert_eq!(w.convert(0.0, -8.0), Point::new(0, 1));
        assert_eq!(w.convert(0.0, f32::NAN), Point::ZERO);
        let mut x11 = WheelConverter::new(1.0);
        assert_eq!(x11.convert(0.0, -1.0), Point::new(0, 1));
        assert_eq!(x11.convert(0.0, 1.0), Point::new(0, -1));
        let mut bad = WheelConverter::new(0.0);
        assert_eq!(bad.units_per_step(), 1.0);
        assert_eq!(bad.convert(0.0, -2.0), Point::new(0, 2));
    }

    #[test]
    fn touch_ids() {
        let mut t = TouchIds::default();
        assert_eq!(t.start(0xdead), 0);
        assert_eq!(t.start(0xbeef), 1);
        assert_eq!(t.start(0xdead), 0); // repeated start: same slot
        assert_eq!(t.get(0xbeef), Some(1));
        assert_eq!(t.end(0xdead), Some(0));
        assert_eq!(t.get(0xdead), None);
        assert_eq!(t.start(7), 0); // lowest free
        assert_eq!(t.start(8), 2);
        assert_eq!(t.end(99), None);
    }

    #[test]
    fn unprintable_chars() {
        for ch in ['\r', '\t', '\u{8}', '\u{1b}', '\u{7f}', '\u{3}', '\u{f700}', '\u{f728}', '\u{e000}', '\u{f8ff}'] {
            assert!(is_unprintable(ch), "{:#x}", ch as u32);
        }
        for ch in ['a', ' ', 'é', '€', '\u{f900}'] {
            assert!(!is_unprintable(ch), "{ch}");
        }
    }

    // ------------------------------------------------------------------
    // Through a real (headless) Ui
    // ------------------------------------------------------------------

    #[derive(Clone, Debug, PartialEq)]
    struct Rec {
        kind: EventKind,
        key: Option<char>,
        special: SpecialKey,
        mods: Modifiers,
        count: u32,
        pos: Point,
        delta: Point,
        ref_id: TbId,
        touch: bool,
    }

    type Log = Rc<RefCell<Vec<Rec>>>;

    struct Probe {
        log: Log,
        handle_shortcuts: bool,
    }

    impl Widget for Probe {
        fn type_name(&self) -> &'static str {
            "Probe"
        }
        fn on_event(&mut self, _cx: &mut Ctx, ev: &Event) -> bool {
            self.log.borrow_mut().push(Rec {
                kind: ev.kind,
                key: ev.key,
                special: ev.special,
                mods: ev.mods,
                count: ev.count,
                pos: ev.pos,
                delta: ev.delta,
                ref_id: ev.ref_id,
                touch: ev.touch,
            });
            ev.kind == EventKind::Shortcut && self.handle_shortcuts
        }
    }

    /// A Ui with one focused probe at (10, 10, 100, 100).
    fn setup(handle_shortcuts: bool) -> (Ui, WidgetId, Log, InputAdapter) {
        let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
        ui.services_mut().log = None;
        let mut input = InputAdapter::new();
        input.resize_event(&mut ui, 400.0, 300.0);
        input.set_shortcut_modifier(Modifiers::CTRL);
        let log: Log = Rc::default();
        let root = ui.root();
        let w = ui.add(root, Probe { log: log.clone(), handle_shortcuts });
        ui.set_rect(w, Rect::new(10, 10, 100, 100));
        ui.set_focusable(w, true);
        assert!(ui.set_focus(w));
        log.borrow_mut().clear();
        (ui, w, log, input)
    }

    fn take(log: &Log) -> Vec<Rec> {
        std::mem::take(&mut *log.borrow_mut())
    }

    fn keys(log: &Log) -> Vec<(EventKind, Option<char>, SpecialKey, Modifiers)> {
        take(log).into_iter().map(|r| (r.kind, r.key, r.special, r.mods)).collect()
    }

    use EventKind::{KeyDown, KeyUp};

    #[test]
    fn resize_sets_viewport() {
        let (mut ui, _, _, mut input) = setup(false);
        input.resize_event(&mut ui, 1280.0, 719.6);
        assert_eq!(ui.viewport(), Size::new(1280, 720));
    }

    #[test]
    fn printable_key_arrives_once() {
        let (mut ui, _, log, mut input) = setup(false);
        // miniquad: key_down, then char_event, then key_up.
        assert!(!input.key_down_event(&mut ui, KeyCode::A, NONE, false));
        input.char_event(&mut ui, 'a', NONE, false);
        assert!(!input.key_up_event(&mut ui, KeyCode::A, NONE));
        let n = Modifiers::NONE;
        assert_eq!(keys(&log), [(KeyDown, Some('a'), SpecialKey::None, n), (KeyUp, Some('a'), SpecialKey::None, n)]);
        // Shifted characters come from char_event as typed.
        input.key_down_event(&mut ui, KeyCode::LeftShift, NONE, false);
        input.key_down_event(&mut ui, KeyCode::Key1, SHIFT, false);
        input.char_event(&mut ui, '!', SHIFT, false);
        input.key_up_event(&mut ui, KeyCode::Key1, SHIFT);
        input.key_up_event(&mut ui, KeyCode::LeftShift, SHIFT);
        let s = Modifiers::SHIFT;
        assert_eq!(keys(&log), [(KeyDown, Some('!'), SpecialKey::None, s), (KeyUp, Some('!'), SpecialKey::None, s)]);
        assert_eq!(input.modifiers(), Modifiers::NONE);
        // AltGr (Ctrl+Alt on Windows) types characters.
        input.char_event(&mut ui, '@', CTRL_ALT, false);
        assert_eq!(take(&log).len(), 2);
    }

    #[test]
    fn special_keys_arrive_once() {
        let (mut ui, _, log, mut input) = setup(false);
        // macOS sends these as private-use or control characters too.
        input.key_down_event(&mut ui, KeyCode::Left, NONE, false);
        input.char_event(&mut ui, '\u{f702}', NONE, false);
        input.key_up_event(&mut ui, KeyCode::Left, NONE);
        input.key_down_event(&mut ui, KeyCode::Enter, NONE, false);
        input.char_event(&mut ui, '\r', NONE, false);
        input.key_down_event(&mut ui, KeyCode::Enter, NONE, true); // repeat
        input.key_up_event(&mut ui, KeyCode::Enter, NONE);
        input.key_down_event(&mut ui, KeyCode::Backspace, NONE, false);
        input.char_event(&mut ui, '\u{7f}', NONE, false);
        let n = Modifiers::NONE;
        assert_eq!(
            keys(&log),
            [
                (KeyDown, None, SpecialKey::Left, n),
                (KeyUp, None, SpecialKey::Left, n),
                (KeyDown, None, SpecialKey::Enter, n),
                (KeyDown, None, SpecialKey::Enter, n),
                (KeyUp, None, SpecialKey::Enter, n),
                (KeyDown, None, SpecialKey::Backspace, n),
            ]
        );
    }

    #[test]
    fn shortcuts_handled_and_unhandled() {
        let (mut ui, _, log, mut input) = setup(true);
        // Handled: the key itself is not delivered on press; its release is (as TB).
        assert!(input.key_down_event(&mut ui, KeyCode::C, CTRL, false));
        input.char_event(&mut ui, 'c', CTRL, false); // X11 / macOS-style char: dropped
        input.char_event(&mut ui, '\u{3}', CTRL, false); // Windows-style control char: dropped
        input.key_up_event(&mut ui, KeyCode::C, CTRL);
        let r = take(&log);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].kind, r[0].ref_id, r[0].mods), (EventKind::Shortcut, tbid!("copy"), Modifiers::CTRL));
        assert_eq!((r[1].kind, r[1].key), (KeyUp, Some('C')));

        // Unhandled: the shortcut is offered, then the key goes through.
        let (mut ui, _, log, mut input) = setup(false);
        assert!(!input.key_down_event(&mut ui, KeyCode::V, CTRL, false));
        let r = take(&log);
        assert_eq!(
            r.iter().map(|r| (r.kind, r.ref_id)).collect::<Vec<_>>(),
            [(EventKind::Shortcut, tbid!("paste")), (KeyDown, TbId::NONE)]
        );
        assert_eq!((r[1].key, r[1].mods), (Some('V'), Modifiers::CTRL));
        // Ctrl released before the key: the key-up still arrives.
        input.key_up_event(&mut ui, KeyCode::LeftControl, CTRL);
        input.key_up_event(&mut ui, KeyCode::V, NONE);
        assert_eq!(keys(&log), [(KeyUp, Some('V'), SpecialKey::None, Modifiers::NONE)]);
        // Ctrl+key that is no shortcut: just the key.
        input.key_down_event(&mut ui, KeyCode::Key8, CTRL, false);
        assert_eq!(keys(&log), [(KeyDown, Some('8'), SpecialKey::None, Modifiers::CTRL)]);
        // Shortcut with a special key.
        input.key_down_event(&mut ui, KeyCode::PageDown, CTRL, false);
        let r = take(&log);
        assert_eq!((r[0].kind, r[0].ref_id), (EventKind::Shortcut, tbid!("next_doc")));
        assert_eq!((r[1].kind, r[1].special), (KeyDown, SpecialKey::PageDown));
        // Cmd is not the shortcut modifier here: Cmd+C is a plain chord key.
        input.key_down_event(&mut ui, KeyCode::C, CMD, false);
        assert_eq!(keys(&log), [(KeyDown, Some('C'), SpecialKey::None, Modifiers::SUPER)]);
    }

    #[test]
    fn menu_key_opens_context_menu_on_release() {
        let (mut ui, _, log, mut input) = setup(false);
        input.key_down_event(&mut ui, KeyCode::Menu, NONE, false);
        assert!(take(&log).is_empty());
        input.key_up_event(&mut ui, KeyCode::Menu, NONE);
        assert_eq!(take(&log).iter().map(|r| r.kind).collect::<Vec<_>>(), [EventKind::ContextMenu]);
    }

    #[test]
    fn clicks_wheel_and_modifiers() {
        let (mut ui, _, log, mut input) = setup(false);
        input.wheel_converter_mut().set_units_per_step(120.0);
        ui.set_time(1000);
        input.mouse_motion_event(&mut ui, 30.7, 40.2);
        assert_eq!(input.pointer(), Point::new(30, 40));
        assert!(input.mouse_button_down_event(&mut ui, MouseButton::Left, 30.0, 40.0));
        assert!(input.mouse_button_up_event(&mut ui, MouseButton::Left, 30.0, 40.0));
        ui.set_time(1200);
        input.mouse_button_down_event(&mut ui, MouseButton::Left, 31.0, 41.0);
        input.mouse_button_up_event(&mut ui, MouseButton::Left, 31.0, 41.0);
        ui.set_time(2000);
        input.key_down_event(&mut ui, KeyCode::LeftShift, NONE, false);
        input.mouse_button_down_event(&mut ui, MouseButton::Left, 31.0, 41.0);
        input.mouse_button_up_event(&mut ui, MouseButton::Left, 31.0, 41.0);
        input.mouse_wheel_event(&mut ui, 0.0, -120.0);
        assert!(!input.mouse_wheel_event(&mut ui, 0.0, -60.0)); // half a step: nothing yet
        let r = take(&log);
        let downs: Vec<(u32, Point, Modifiers)> =
            r.iter().filter(|r| r.kind == EventKind::PointerDown).map(|r| (r.count, r.pos, r.mods)).collect();
        // Positions are in the probe's coordinates (it sits at 10, 10).
        assert_eq!(
            downs,
            [
                (1, Point::new(20, 30), Modifiers::NONE),
                (2, Point::new(21, 31), Modifiers::NONE),
                (1, Point::new(21, 31), Modifiers::SHIFT),
            ]
        );
        assert_eq!(r.iter().filter(|r| r.kind == EventKind::Click).count(), 3);
        let wheel: Vec<_> = r.iter().filter(|r| r.kind == EventKind::Wheel).map(|r| (r.delta, r.mods)).collect();
        assert_eq!(wheel, [(Point::new(0, 1), Modifiers::SHIFT)]);
        input.key_up_event(&mut ui, KeyCode::LeftShift, SHIFT);
        assert_eq!(input.modifiers(), Modifiers::NONE);
    }

    #[test]
    fn right_button_sends_context_menu() {
        let (mut ui, w, log, mut input) = setup(false);
        assert!(!input.mouse_button_down_event(&mut ui, MouseButton::Right, 50.0, 60.0));
        assert!(take(&log).is_empty());
        input.mouse_button_up_event(&mut ui, MouseButton::Right, 50.0, 60.0);
        let r = take(&log);
        assert_eq!(r.iter().map(|r| r.kind).collect::<Vec<_>>(), [EventKind::PointerMove, EventKind::ContextMenu]);
        assert_eq!(r[1].pos, Point::new(40, 50));
        assert_eq!(ui.hovered(), Some(w));
        // Outside every widget: nothing to send it to.
        input.mouse_button_up_event(&mut ui, MouseButton::Right, 300.0, 250.0);
        assert!(take(&log).iter().all(|r| r.kind != EventKind::ContextMenu));
        // Middle button: ignored.
        assert!(!input.mouse_button_down_event(&mut ui, MouseButton::Middle, 50.0, 60.0));
        assert!(take(&log).is_empty());
    }

    #[test]
    fn touches() {
        let (mut ui, _, log, mut input) = setup(false);
        ui.set_time(500);
        input.touch_event(&mut ui, TouchPhase::Started, 1234, 20.0, 20.0);
        input.touch_event(&mut ui, TouchPhase::Started, 999, 30.0, 30.0);
        input.touch_event(&mut ui, TouchPhase::Moved, 999, 32.0, 30.0);
        input.touch_event(&mut ui, TouchPhase::Ended, 999, 32.0, 30.0);
        input.touch_event(&mut ui, TouchPhase::Ended, 1234, 20.0, 20.0);
        assert!(!input.touch_event(&mut ui, TouchPhase::Moved, 555, 1.0, 1.0)); // unknown
        let r = take(&log);
        let got: Vec<(EventKind, TbId, bool)> = r.iter().map(|r| (r.kind, r.ref_id, r.touch)).collect();
        assert_eq!(
            got,
            [
                (EventKind::PointerDown, TbId::NONE, true), // first finger: the pointer path
                (EventKind::TouchDown, TbId(1), true),
                (EventKind::TouchMove, TbId(1), true),
                (EventKind::TouchUp, TbId(1), true),
                (EventKind::PointerUp, TbId::NONE, true),
                (EventKind::Click, TbId::NONE, true),
            ]
        );
        // A quick second tap of the first finger is a double click.
        ui.set_time(600);
        input.touch_event(&mut ui, TouchPhase::Started, 77, 21.0, 20.0);
        let r = take(&log);
        assert_eq!((r[0].kind, r[0].count), (EventKind::PointerDown, 2));
        input.touch_event(&mut ui, TouchPhase::Cancelled, 77, 21.0, 20.0);
        assert_eq!(ui.captured(), None);
    }

    #[test]
    fn focus_lost_releases_everything() {
        let (mut ui, _, log, mut input) = setup(false);
        input.key_down_event(&mut ui, KeyCode::LeftControl, CTRL, false);
        input.key_down_event(&mut ui, KeyCode::B, CTRL, false);
        input.mouse_button_down_event(&mut ui, MouseButton::Left, 20.0, 20.0);
        assert!(ui.captured().is_some());
        take(&log);
        input.focus_lost(&mut ui);
        assert_eq!(input.modifiers(), Modifiers::NONE);
        assert_eq!(ui.captured(), None);
        // The chord key was forgotten: its late release is not delivered.
        input.key_up_event(&mut ui, KeyCode::B, NONE);
        assert!(take(&log).is_empty());
    }
}
