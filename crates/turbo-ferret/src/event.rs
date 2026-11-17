//! Widget events, keys and modifiers.
//!
//! TB: tb_widgets.h (TBWidgetEvent, EVENT_TYPE, SPECIAL_KEY, MODIFIER_KEYS). Event
//! subclasses become an optional payload (H-09).

use alloc::rc::Rc;
use core::any::Any;

use crate::arena::WidgetId;
use crate::flags::flags;
use crate::geom::Point;
use crate::id::TbId;
use crate::value::Value;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EventKind {
    /// Pointer released over the widget that got the pointer down (or Enter/Space on a
    /// click-by-key widget).
    Click,
    LongClick,
    PointerDown,
    PointerUp,
    PointerMove,
    Wheel,
    /// The widget's value changed. `Event::value` carries it when the widget has a
    /// value connection.
    Changed,
    KeyDown,
    KeyUp,
    /// Sent by the host (`ui.shortcut`); `ref_id` names the command ("copy", "paste"...).
    Shortcut,
    ContextMenu,
    TouchDown,
    TouchUp,
    TouchMove,
    TouchCancel,
    /// `payload` holds a `Vec<String>` of paths.
    FileDrop,
    Custom(TbId),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum SpecialKey {
    #[default]
    None,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Tab,
    Backspace,
    Insert,
    Delete,
    Enter,
    Esc,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

flags! {
    /// Keyboard modifiers. TB: MODIFIER_KEYS
    pub struct Modifiers: u8 {
        const CTRL = 1;
        const SHIFT = 2;
        const ALT = 4;
        const SUPER = 8;
    }
}

/// A widget event. `pos` is in the target's coordinates and stays fixed while bubbling.
#[derive(Clone)]
pub struct Event {
    pub kind: EventKind,
    /// The original target; fixed while bubbling.
    pub target: WidgetId,
    pub pos: Point,
    /// Wheel and move deltas. Positive y is a wheel turn towards the user (scroll down).
    pub delta: Point,
    /// Click count (1 single, 2 double ...).
    pub count: u32,
    pub key: Option<char>,
    pub special: SpecialKey,
    pub mods: Modifiers,
    /// Command for `Shortcut`, button id in a message window, touch id for touch events.
    pub ref_id: TbId,
    pub touch: bool,
    /// `Changed` only, and only when the widget has a value connection.
    pub value: Option<Value>,
    pub payload: Option<Rc<dyn Any>>,
}

impl core::fmt::Debug for Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Event")
            .field("kind", &self.kind)
            .field("target", &self.target)
            .field("pos", &self.pos)
            .field("count", &self.count)
            .field("key", &self.key)
            .field("special", &self.special)
            .finish_non_exhaustive()
    }
}

impl Event {
    pub fn new(kind: EventKind, target: WidgetId) -> Event {
        Event {
            kind,
            target,
            pos: Point::ZERO,
            delta: Point::ZERO,
            count: 1,
            key: None,
            special: SpecialKey::None,
            mods: Modifiers::NONE,
            ref_id: TbId::NONE,
            touch: false,
            value: None,
            payload: None,
        }
    }

    pub fn at(mut self, pos: Point) -> Event {
        self.pos = pos;
        self
    }

    pub fn is_pointer(&self) -> bool {
        matches!(self.kind, EventKind::PointerDown | EventKind::PointerUp | EventKind::PointerMove)
    }

    pub fn is_touch(&self) -> bool {
        matches!(self.kind, EventKind::TouchDown | EventKind::TouchUp | EventKind::TouchMove | EventKind::TouchCancel)
    }

    pub fn is_key(&self) -> bool {
        matches!(self.kind, EventKind::KeyDown | EventKind::KeyUp)
    }

    /// 1-based click count cycled within `n` (for "triple click selects line" etc.).
    /// TB: TBWidgetEvent::count with modulo.
    pub fn count_cycle(&self, n: u32) -> u32 {
        if n == 0 { self.count } else { (self.count.max(1) - 1) % n + 1 }
    }
}
