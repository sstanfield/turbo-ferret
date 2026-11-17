//! Platform quirks of key input, as event sequences taken from miniquad 0.4.11's
//! platform sources: AltGr, Option, paired modifier keys, chords, the web's char events.
use miniquad::{KeyCode, KeyMods};
use turbo_ferret::widgets::*;
use turbo_ferret::*;
use turbo_ferret_miniquad::{InputAdapter, KeyQuirks};

const NONE: KeyMods = KeyMods { shift: false, ctrl: false, alt: false, logo: false };
const CTRL: KeyMods = KeyMods { shift: false, ctrl: true, alt: false, logo: false };
const ALT: KeyMods = KeyMods { shift: false, ctrl: false, alt: true, logo: false };
const CTRL_ALT: KeyMods = KeyMods { shift: false, ctrl: true, alt: true, logo: false };

fn setup() -> (Ui, WidgetId, InputAdapter) {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    let mut input = InputAdapter::new();
    input.resize_event(&mut ui, 400.0, 300.0);
    ui.set_time(1000);
    let root = ui.root();
    let e = ui.add(root, EditField::default());
    ui.set_rect(e, Rect::new(10, 10, 200, 27));
    ui.update();
    assert!(ui.set_focus(e));
    (ui, e, input)
}

/// Windows: AltGr is reported as Ctrl+Alt (miniquad's key_mods reads VK_CONTROL and
/// VK_MENU), and WM_CHAR delivers the character. German layout: AltGr+Q = '@'.
#[test]
fn altgr_character_is_typed_into_an_edit_field() {
    let (mut ui, e, mut input) = setup();
    input.char_event(&mut ui, 'a', NONE, false);
    assert_eq!(ui.text(e), Some("a"));
    // The key events of AltGr+Q as Windows sends them.
    input.key_down_event(&mut ui, KeyCode::LeftControl, CTRL, false);
    input.key_down_event(&mut ui, KeyCode::RightAlt, CTRL_ALT, false);
    input.key_down_event(&mut ui, KeyCode::Q, CTRL_ALT, false);
    input.char_event(&mut ui, '@', CTRL_ALT, false);
    input.key_up_event(&mut ui, KeyCode::Q, CTRL_ALT);
    assert_eq!(ui.text(e), Some("a@"), "AltGr+Q must type '@'");
}

/// macOS: Option+key types a character (Option+2 = '™' on a US layout).
#[test]
fn option_character_is_typed() {
    let (mut ui, e, mut input) = setup();
    input.set_key_quirks(KeyQuirks { alt_types_text: true, ..KeyQuirks::for_platform() });
    input.key_down_event(&mut ui, KeyCode::LeftAlt, ALT, false);
    input.key_down_event(&mut ui, KeyCode::Key2, ALT, false);
    input.char_event(&mut ui, '™', ALT, false);
    assert_eq!(ui.text(e), Some("™"));
}

// ---------------------------------------------------------------------------
// A probe that records what it gets.
// ---------------------------------------------------------------------------
use miniquad::MouseButton;
use std::cell::RefCell;
use std::rc::Rc;

type Log = Rc<RefCell<Vec<(EventKind, Option<char>, Modifiers, TbId)>>>;

struct Probe(Log);
impl Widget for Probe {
    fn type_name(&self) -> &'static str {
        "Probe"
    }
    fn on_event(&mut self, _cx: &mut Ctx, ev: &Event) -> bool {
        self.0.borrow_mut().push((ev.kind, ev.key, ev.mods, ev.ref_id));
        ev.kind == EventKind::Shortcut
    }
}

fn probe() -> (Ui, Log, InputAdapter) {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    let mut input = InputAdapter::new();
    input.resize_event(&mut ui, 400.0, 300.0);
    input.set_shortcut_modifier(Modifiers::CTRL);
    let log: Log = Rc::default();
    let root = ui.root();
    let w = ui.add(root, Probe(log.clone()));
    ui.set_rect(w, Rect::new(10, 10, 100, 100));
    ui.set_focusable(w, true);
    assert!(ui.set_focus(w));
    log.borrow_mut().clear();
    (ui, log, input)
}

/// Linux/X11: Ctrl+Alt+V delivers char 'v' with ctrl+alt. The key path refuses
/// Ctrl+Alt chords (TB: `!key_alt`), so the char path should not turn it into a
/// shortcut either.
#[test]
fn ctrl_alt_char_is_not_a_shortcut() {
    let (mut ui, log, mut input) = probe();
    assert!(!input.key_down_event(&mut ui, KeyCode::V, CTRL_ALT, false));
    input.char_event(&mut ui, 'v', CTRL_ALT, false);
    let got: Vec<TbId> = log.borrow().iter().filter(|r| r.0 == EventKind::Shortcut).map(|r| r.3).collect();
    assert_eq!(got, [], "Ctrl+Alt+V must not paste");
}

/// Web: miniquad's wasm backend calls char_event with KeyMods::default() whatever is
/// held. A shift+click after typing a shifted character must still carry Shift.
#[test]
fn char_event_without_mods_keeps_held_modifiers() {
    let (mut ui, log, mut input) = probe();
    input.set_key_quirks(KeyQuirks { char_events_carry_modifiers: false, ..KeyQuirks::for_platform() });
    let shift = KeyMods { shift: true, ..NONE };
    input.key_down_event(&mut ui, KeyCode::LeftShift, shift, false);
    input.key_down_event(&mut ui, KeyCode::B, shift, false);
    input.char_event(&mut ui, 'B', NONE, false); // wasm: no modifiers on char events
    log.borrow_mut().clear();
    // Shift is still held when the button goes down.
    input.mouse_button_down_event(&mut ui, MouseButton::Left, 20.0, 20.0);
    let down: Vec<Modifiers> = log.borrow().iter().filter(|r| r.0 == EventKind::PointerDown).map(|r| r.2).collect();
    assert_eq!(down, [Modifiers::SHIFT]);
}

/// Both Shift keys held, one released: every platform still reports shift in the
/// event's modifiers, and Shift is still down.
#[test]
fn releasing_one_of_two_shifts_keeps_shift() {
    let (mut ui, log, mut input) = probe();
    let shift = KeyMods { shift: true, ..NONE };
    input.key_down_event(&mut ui, KeyCode::LeftShift, shift, false);
    input.key_down_event(&mut ui, KeyCode::RightShift, shift, false);
    input.key_up_event(&mut ui, KeyCode::LeftShift, shift); // right shift is still down
    input.mouse_button_down_event(&mut ui, MouseButton::Left, 20.0, 20.0);
    let down: Vec<Modifiers> = log.borrow().iter().filter(|r| r.0 == EventKind::PointerDown).map(|r| r.2).collect();
    assert_eq!(down, [Modifiers::SHIFT]);
}

/// macOS: Cocoa does not deliver keyUp while Cmd is held, so a Cmd+C chord never
/// gets its key-up. A later plain 'c' must not produce an extra KeyUp 'C'.
#[test]
fn no_stray_key_up_after_cmd_chord() {
    let (mut ui, log, mut input) = probe();
    input.set_shortcut_modifier(Modifiers::SUPER);
    input.set_key_quirks(KeyQuirks { chords_end_with_modifier: true, ..KeyQuirks::for_platform() });
    let cmd = KeyMods { logo: true, ..NONE };
    input.key_down_event(&mut ui, KeyCode::LeftSuper, cmd, false);
    input.key_down_event(&mut ui, KeyCode::C, cmd, false);
    input.char_event(&mut ui, 'c', cmd, false);
    // (no key_up for C: Cocoa swallows it)
    input.key_up_event(&mut ui, KeyCode::LeftSuper, NONE);
    log.borrow_mut().clear();
    input.key_down_event(&mut ui, KeyCode::C, NONE, false);
    input.char_event(&mut ui, 'c', NONE, false);
    input.key_up_event(&mut ui, KeyCode::C, NONE);
    let got: Vec<(EventKind, Option<char>)> = log.borrow().iter().map(|r| (r.0, r.1)).collect();
    assert_eq!(got, [(EventKind::KeyDown, Some('c')), (EventKind::KeyUp, Some('c'))]);
}

/// The chord's key-up is sent when Cmd is released, so every key down has its up.
#[test]
fn a_cmd_chord_ends_when_cmd_is_released() {
    let (mut ui, log, mut input) = probe();
    input.set_shortcut_modifier(Modifiers::SUPER);
    input.set_key_quirks(KeyQuirks { chords_end_with_modifier: true, ..KeyQuirks::for_platform() });
    let cmd = KeyMods { logo: true, ..NONE };
    input.key_down_event(&mut ui, KeyCode::LeftSuper, cmd, false);
    input.key_down_event(&mut ui, KeyCode::B, cmd, false); // not a shortcut: a key event
    input.key_up_event(&mut ui, KeyCode::LeftSuper, NONE);
    let got: Vec<(EventKind, Option<char>)> = log.borrow().iter().map(|r| (r.0, r.1)).collect();
    assert_eq!(got, [(EventKind::KeyDown, Some('B')), (EventKind::KeyUp, Some('B'))]);
    // Elsewhere the key's own key-up ends it, also after the modifier went up.
    let (mut ui, log, mut input) = probe();
    input.set_key_quirks(KeyQuirks { chords_end_with_modifier: false, ..KeyQuirks::for_platform() });
    input.key_down_event(&mut ui, KeyCode::LeftControl, CTRL, false);
    input.key_down_event(&mut ui, KeyCode::B, CTRL, false);
    input.key_up_event(&mut ui, KeyCode::LeftControl, NONE);
    assert_eq!(log.borrow().len(), 1, "only the key down so far");
    input.key_up_event(&mut ui, KeyCode::B, NONE);
    let got: Vec<(EventKind, Option<char>)> = log.borrow().iter().map(|r| (r.0, r.1)).collect();
    assert_eq!(got, [(EventKind::KeyDown, Some('B')), (EventKind::KeyUp, Some('B'))]);
}

/// X11 sends Alt+F as the character 'f' with alt. That is a menu chord, not text.
/// On macOS Option types characters, so there it is text.
#[test]
fn alt_with_a_letter_is_text_only_where_alt_types() {
    for (alt_types_text, want) in [(false, Some("")), (true, Some("f"))] {
        let (mut ui, e, mut input) = setup();
        input.set_key_quirks(KeyQuirks { alt_types_text, ..KeyQuirks::for_platform() });
        input.key_down_event(&mut ui, KeyCode::LeftAlt, ALT, false);
        input.char_event(&mut ui, 'f', ALT, false);
        assert_eq!(ui.text(e), want);
    }
}

/// Losing the focus forgets the held keys.
#[test]
fn focus_lost_forgets_held_modifier_keys() {
    let (mut ui, _log, mut input) = probe();
    let shift = KeyMods { shift: true, ..NONE };
    input.key_down_event(&mut ui, KeyCode::LeftShift, shift, false);
    input.key_down_event(&mut ui, KeyCode::RightShift, shift, false);
    input.focus_lost(&mut ui);
    input.key_up_event(&mut ui, KeyCode::LeftShift, NONE);
    assert_eq!(input.modifiers(), Modifiers::NONE);
}
