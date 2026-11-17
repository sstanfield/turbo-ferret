# Turbo Ferret: Design

*Turbo Ferret is a Rust port of [Turbo Badger](https://github.com/fruxo/turbobadger), the UI toolkit by Emil Segerås. Step 4 of the plan in [decisions-and-status.md](decisions-and-status.md). This is the design that implementation (with Claude Code) will follow. Written 2026-11-17, accepted by Steve 2026-11-17.*

**Inputs.** [decisions-and-status.md](decisions-and-status.md) is the source of truth for every decision referenced here. [turbobadger-architecture.md](turbobadger-architecture.md) describes the C++ original and its hazard catalogue (`H-01` to `H-22`). [ecs-widget-storage-assessment.md](ecs-widget-storage-assessment.md) explains the storage choice and the mutability questions (`M-01` to `M-08`). TB source: `github.com/fruxo/turbobadger` at `389baa6`.

**How to read this doc.** Sections 1 to 3 give the shape (principles, crates, foundation types). Sections 4 to 8 are the core: the tree, the `Ui` context, the widget trait and the mutation rules. Sections 9 to 16 walk through each subsystem (events, messages, animation, layout, skin, rendering, fonts, resources). Section 17 covers services, section 18 the three open choices, section 19 maps every hazard to where it is handled, and section 20 is the milestone plan. Rust code is a design sketch: names and signatures are meant to be followed, but bodies are illustrative.

**Status.** Steve accepted the whole design on 2026-11-17, including the items first put forward here (marked **(decided)**). Nothing is open; the port's licence is zlib (section 0).

---

## Contents

0. [Origin, credits and licence](#0-origin-credits-and-licence)
1. [Principles](#1-principles)
2. [Crate layout](#2-crate-layout)
3. [Foundation types](#3-foundation-types)
4. [The widget tree](#4-the-widget-tree)
5. [The `Ui` context and the host loop](#5-the-ui-context-and-the-host-loop)
6. [The `Widget` trait](#6-the-widget-trait)
7. [Calling hooks: `Ctx`, take-out and external callbacks](#7-calling-hooks-ctx-take-out-and-external-callbacks)
8. [Changing the tree](#8-changing-the-tree)
9. [Events and input](#9-events-and-input)
10. [Messages, timers and time](#10-messages-timers-and-time)
11. [Animation](#11-animation)
12. [Layout](#12-layout)
13. [Skin](#13-skin)
14. [Rendering](#14-rendering)
15. [Fonts](#15-fonts)
16. [Resources, inflation and language](#16-resources-inflation-and-language)
17. [Services](#17-services)
18. [The three open choices](#18-the-three-open-choices)
19. [Hazard coverage](#19-hazard-coverage)
20. [Milestone plan](#20-milestone-plan)
21. [Implementation conventions](#21-implementation-conventions)
22. [Deliberate differences from TB](#22-deliberate-differences-from-tb)

---

## 0. Origin, credits and licence

**Turbo Ferret is a port, not the original.** It is a Rust reimplementation of **Turbo Badger** (`https://github.com/fruxo/turbobadger`), the fast UI toolkit for games written by **Emil Segerås** (copyright 2011 to 2014, per the notice in `src/tb/tb_core.h`). The design follows Turbo Badger's architecture closely, and the reference is `master` at commit `389baa6` (24 July 2016, the latest commit). The new name sets the port apart from the original, as the licence asks.

**Turbo Badger's licence is the zlib licence** (full text in `src/tb/tb_core.h` and `readme.md`). Its three conditions shape what the port must do:

1. *The origin must not be misrepresented.* Every crate's README and the crate-level docs (`lib.rs`) open with: "Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås (https://github.com/fruxo/turbobadger). It is not the original software, and it is not endorsed by its author."
2. *Altered versions must be plainly marked as such.* The new name, that sentence, and the `// TB: <file> <function>` comments on ported code (section 21) mark it.
3. *The notice must not be removed from source distributions.* The workspace root carries a `NOTICE` (or `LICENSE-turbobadger`) file with Turbo Badger's copyright line and full zlib text, and every published crate includes it (Cargo `include`).

**Assets reused from Turbo Badger.** The default skin (`resources/default_skin`) is public domain, per its `license.txt`. The default font folder (`resources/default_font`) is marked public domain too, but its own `license.txt` warns that the bitmaps are rasterized from a copyrighted font (Segoe), with "use at your own risk". So the port uses that font **only in the demo and tests**, never inside a published crate. A published default font, if one is wanted, is rasterized from an openly licensed font, which can be done at M8 alongside TrueType.

**The port's own licence: zlib** (decided by Steve, 2026-11-17), the same as Turbo Badger. Each crate's `Cargo.toml` sets `license = "Zlib"`. The workspace root has a `LICENSE` file with Turbo Ferret's own zlib notice, next to the `NOTICE` file carrying Turbo Badger's copyright line and licence text, and both ship in every published crate.

---

## 1. Principles

These come straight from the agreed decisions and Steve's priorities, and they settle most small questions during implementation.

1. **Small engine.** Prefer one simple mechanism over two clever ones. If a TB feature needs machinery out of proportion to its use, cut it or defer it.
2. **Few dependencies.** The core has no dependencies at all (the PNG decoder is hand-rolled, decision 11). Backends carry their own.
3. **Narrow renderer.** The backend draws textured, colored rectangles inside a clip rect and manages bitmaps. Everything else (translation, opacity, batching, atlasing) lives in the core.
4. **Host-driven.** The host owns the loop, feeds input and time, and calls `update` and `paint`. The core never sleeps, spawns or reads the clock itself.
5. **One `Ui`, no globals.** Every piece of engine state is a field of `Ui`. Two `Ui`s in one process are independent.
6. **`no_std` + `alloc` core.** Anything that needs `std` is behind a service trait with a `std` default.
7. **TB as the reference, not a contract.** Keep TB's behaviour where there is no reason to change it, and change it where the change makes the port simpler (section 22 lists every deliberate change).

---

## 2. Crate layout

### Workspace (decided)

```
turbo-ferret/                (workspace root)
├── crates/
│   ├── turbo-ferret/        core: #![no_std] + alloc. Everything except backends.
│   ├── turbo-ferret-soft/    software renderer into an RGBA buffer. #![no_std] + alloc, zero deps.
│   └── turbo-ferret-miniquad/ OpenGL/GLES/WebGL backend + input adapter on miniquad.
├── demo/                    the demo app (not published): miniquad window, demo resources.
├── resources/               default skin, default font, demo layouts (TB's, reused).
└── tests/golden/            reference images for paint tests (software renderer output).
```

**Why one core crate and not many.** Widgets, layout, skin and the tree all need the crate-private internals of `Ui`. Splitting the parser, skin or fonts into separate crates would force those internals to become public API, and would add version coupling for no gain. The only boundaries that carry their own dependencies are the backends, so those are the only separate crates. Modules can still be split out later (the parser is the most self-contained) if someone wants them on their own.

### Core crate features

| Feature | Default | What it adds | Dependency |
|---|---|---|---|
| `std` | on | `FsLoader` (files through `std::fs`), and the stderr log sink | none |
| `png` | on | `PngDecoder`, the default `ImageDecoder` (hand-rolled, `png.rs` + `inflate.rs`) | none |
| `debug` | off | Debug overlay (layout bounds, atlas view), recorded ID strings for collision warnings (H-20) | none |
| `truetype` | off, added at M8 | TrueType `FontBackend` | `fontdue` or `ttf-parser`, to be chosen at M8 |

A host without `std` uses `default-features = false, features = ["png"]` and supplies its own `ResourceLoader`. The PNG decoder needs only `alloc`.

### Dependency budget

| Crate | Normal dependencies |
|---|---|
| `turbo-ferret` | none |
| `turbo-ferret-soft` | `turbo-ferret` |
| `turbo-ferret-miniquad` | `turbo-ferret`, `miniquad` (which itself only pulls platform system crates) |
| `demo` | the three above |

Dev-dependencies (tests only, never shipped) may use `png` for writing failed golden images, and nothing else without a reason.

### Core module tree

```
turbo-ferret/src/
├── lib.rs            #![no_std], re-exports, prelude
├── id.rs             TbId, const fn fnv1a, tbid!() macro
├── geom.rs           Point, Size, Rect, Edges, Color (+ "#rrggbb[aa]" parse)
├── value.rs          Value, parse_auto()
├── arena.rs          Arena<T>, WidgetId, SideTable<T>
├── math.rs           floor/ceil/round/sqrt/exp helpers for f32 (no libm)
├── services.rs       ResourceLoader, ImageDecoder, LogSink; MemLoader; std impls
├── image/            hand-rolled PNG decoder (feature `png`)
│   ├── inflate.rs    zlib/DEFLATE decompressor (stored, fixed and dynamic Huffman blocks)
│   └── png.rs        chunks, IHDR/PLTE/tRNS/IDAT, unfiltering, conversion to RGBA8
├── res/              the TB text format
│   ├── parser.rs     line parser → ResNode
│   ├── node.rs       ResNode, path lookup "a>b>c"
│   └── refs.rs       ref trees, @include, @file, @if/@else
├── lang.rs           Language (string table)
├── ui/               the context
│   ├── mod.rs        Ui struct, construction, public API surface
│   ├── node.rs       Node, NodeFlags, WidgetState
│   ├── tree.rs       create/attach/detach/free, z-order, queries, walks
│   ├── ctx.rs        Ctx, call_hook, with_widget
│   ├── input.rs      InputState, pointer/key/wheel/touch entry points, hit test
│   ├── focus.rs      focus, windows, activation, tab navigation
│   ├── dispatch.rs   invoke_event, bubbling, listeners
│   ├── process.rs    update(): animations, timers, messages, states, layout, on_process
│   ├── pending.rs    internal deferred queue and flush
│   └── value_group.rs value binding (connection:)
├── widget.rs         Widget trait, DieAction, ScrollInfo, HitStatus …
├── event.rs          Event, EventKind, keys, modifiers
├── msg.rs            MessageQueue
├── anim.rs           Animations, curves, AnimSpec
├── layout/
│   ├── size.rs       SizeConstraints, PreferredSize, LayoutParams
│   ├── measure.rs    preferred_size() with cache, default measure
│   ├── arrange.rs    gravity anchoring (default arrange)
│   ├── stack.rs      the TBLayout algorithm
│   └── dim.rs        DimensionConverter (dp/px/mm)
├── skin/
│   ├── element.rs    SkinElement, StateElement, Condition
│   ├── load.rs       skin file → Skin, DPI variants
│   ├── condition.rs  condition evaluation against the tree
│   └── paint.rs      nine-slice etc. → Painter
├── render/
│   ├── renderer.rs   Renderer trait, Quad, BitmapId
│   ├── painter.rs    batching, translation, opacity, clip stack
│   └── atlas.rs      Atlas, FragmentId, shelf allocator
├── font/
│   ├── desc.rs       FontDesc
│   ├── manager.rs    FontManager, FontFace
│   ├── backend.rs    FontBackend, GlyphSource traits
│   ├── tbbf.rs       TB bitmap font backend (built in)
│   └── cache.rs      glyph cache on an Atlas
├── reader.rs         widget registry and inflation
└── widgets/          built-in widgets, one file each
```

---

## 3. Foundation types

### `TbId`

```rust
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub struct TbId(pub u32);              // 0 = "none"

impl TbId {
    pub const NONE: TbId = TbId(0);
    pub const fn new(s: &str) -> TbId { TbId(fnv1a(s.as_bytes())) }   // usable in const
}
#[macro_export] macro_rules! tbid { ($s:literal) => { { const ID: TbId = TbId::new($s); ID } } }
```

FNV-1a 32 with TB's basis and prime, and the empty string hashing to 0 (H-21). Keeping the same hash costs nothing, so resources that use integer IDs keep working. Under the `debug` feature, `Ui` keeps a `BTreeMap<TbId, String>` of every string hashed through the parser or `TbId::named()`, and warns on collisions. `TbId` itself never changes size (H-20).

### Geometry and color

`Point { x: i32, y: i32 }`, `Size { w: i32, h: i32 }`, `Rect { x, y, w, h }` (integer pixels, as in TB), `Edges { left, top, right, bottom }` for padding and expand. `Color(u32)` stored as straight (non-premultiplied) RGBA for API use, with `Color::parse("#rrggbb[aa]")`. Premultiplication happens only when the core writes pixels or quads (section 14).

### `Value`

```rust
#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default] Null,
    Int(i32),
    Float(f32),
    Str(Rc<str>),
    Array(Rc<[Value]>),
    Object(Rc<dyn Any>),
}
```

- Getters do not mutate (H-13): `as_int()`, `as_float()` and `as_str()` convert on the fly and return a new value or a `Cow`.
- `Value::parse_auto(&str)` reproduces `SetFromStringAuto`: int, float, number arrays (`"6 8"`, `"0, 0, 100, 50"`), otherwise a string.
- `Rc` makes every clone a refcount bump, which is what lets `CHANGED` carry its value cheaply (decision 5 of the mutability set). `Object` replaces `TBTypedObject` payloads.

### Arena, `WidgetId` and `SideTable`

```rust
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WidgetId { index: u32, gen: NonZeroU32 }     // Option<WidgetId> is 8 bytes

pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free_head: u32,          // u32::MAX = empty
    len: u32,
}
enum Slot<T> { Occupied { gen: NonZeroU32, value: T }, Vacant { gen: NonZeroU32, next_free: u32 } }

impl<T> Arena<T> {
    pub fn insert(&mut self, value: T) -> WidgetId;
    pub fn remove(&mut self, id: WidgetId) -> Option<T>;    // bumps gen
    pub fn get(&self, id: WidgetId) -> Option<&T>;          // None if gen differs
    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut T>;
}

/// Optional per-widget data ("components"). Dense by slot index, generation-checked.
pub struct SideTable<T> { slots: Vec<Option<(NonZeroU32, T)>> }
impl<T> SideTable<T> {
    pub fn get(&self, id: WidgetId) -> Option<&T>;
    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut T>;
    pub fn insert(&mut self, id: WidgetId, v: T) -> Option<T>;
    pub fn remove(&mut self, id: WidgetId) -> Option<T>;
}
```

- Hand-rolled, about 250 lines with tests (decision 4).
- A generation that would wrap retires the slot instead of reusing it, so an old ID can never match again.
- `SideTable` is dense (one `Option` per arena slot) rather than sparse. At UI sizes (hundreds of slots) this is a few KB and gives O(1) lookups with no hashing. If memory ever matters, the type can switch to a sorted `Vec` without changing its API.

### Maps and math (decided)

- **Maps:** `alloc::collections::BTreeMap` everywhere a keyed map is needed (skin elements, fonts, language, glyph lookup, registry). Keys are `TbId` or small tuples. No hashing crate. If profiling shows a hot spot (the glyph lookup is the likeliest), replace that one map with a small open-addressing `u32` map in `util`.
- **Float math:** `math.rs` provides `floor`, `ceil`, `round`, `abs`, `sqrt` (bit trick plus two Newton steps) and `exp` (range reduction plus a polynomial), which covers dimension rounding, animation curves, scroller physics and font blur. `libm` is the fallback if any of them proves inaccurate, not a starting dependency.

---

## 4. The widget tree

### `Node`

Shared widget state lives in the node. Per-type behaviour lives in `behaviour` (decision 2).

```rust
pub struct Node {
    // tree
    pub(crate) parent: Option<WidgetId>,
    pub(crate) children: Vec<WidgetId>,     // index 0 = bottom of z-order (painted first)
    // identity
    pub(crate) id: TbId,                     // name hash from `id:` (decision 7)
    pub(crate) group_id: TbId,
    // geometry, appearance, state
    pub(crate) rect: Rect,                   // relative to parent's content origin
    pub(crate) skin: TbId,                   // background skin element
    pub(crate) font: Option<FontDesc>,       // None = inherit
    pub(crate) opacity: f32,
    pub(crate) gravity: Gravity,
    pub(crate) visibility: Visibility,       // Visible / Invisible / Gone
    pub(crate) state: WidgetState,           // DISABLED | SELECTED | PRESSED(explicit) … (bitflags)
    pub(crate) flags: NodeFlags,             // see below
    // layout cache (H-18 lives here, as plain data)
    pub(crate) measure_cache: Option<(SizeConstraints, PreferredSize)>,
    // user slot (TB's `data`)
    pub data: Value,
    // behaviour; None only while a hook is running (take-out)
    pub(crate) behaviour: Option<Box<dyn Widget>>,
}

bitflags-style (hand-rolled, H-14):
NodeFlags: FOCUSABLE, CLICK_BY_KEY, WANT_LONG_CLICK, IGNORE_INPUT, GROUP_ROOT,
           WINDOW, ACTIVE_WINDOW, DYING, DETACHED, NEEDS_ARRANGE, SKIN_STATE_DIRTY
```

`WidgetState` keeps TB's bit layout so that skin state masks and widget state masks are the same type (`SkinState` is an alias). Automatic states (hovered, pressed while captured, focused) are computed from `InputState` when needed, never stored.

Public read access is through `Ui::node(id) -> Option<NodeRef<'_>>`, a read-only view with getters (`rect()`, `id()`, `state()`, `opacity()`, `parent()`, `children()` …). Writes go through `Ui` methods (`set_rect`, `set_opacity`, `set_state` …) because most writes have side effects (invalidate, arrange, skin update).

### Optional parts (side tables)

| Side table on `Tree` | Replaces | Created when |
|---|---|---|
| `layout_params: SideTable<LayoutParams>` | `TBWidget::m_layout_params` | `lp>` in resources, or `set_layout_params` |
| `scrollers: SideTable<Scroller>` | `TBScroller` mixin (H-08) | first kinetic or smooth scroll |
| `connections: SideTable<TbId>` | `TBWidgetValueConnection` (H-03) | `connection:` in resources, or `connect_value` |
| `listeners: SideTable<Vec<ListenerId>>` | per-widget `m_listeners` | `add_widget_listener` |
| `last_focus: SideTable<WidgetId>` | `TBWindow::m_last_focus` | a window's descendant gains focus |

The long-click timer is no longer per widget: only one pointer can be in a long-click wait at a time (one per touch), so it lives in `InputState` (section 9).

### Two IDs

- `WidgetId` is the storage handle and the weak reference used everywhere (replaces `TBWidget*` and `TBWidgetSafePointer`, H-03, H-10).
- `TbId` is the name. `ui.find(root, tbid!("ok")) -> Option<WidgetId>` is a depth-first subtree search, as in TB. An index is deferred (M8) until someone measures a need.

---

## 5. The `Ui` context and the host loop

### Struct

```rust
pub struct Ui {
    pub(crate) tree: Tree,                 // nodes arena + side tables + root id
    pub(crate) input: InputState,          // hovered, captured, focused, touches, long-click, key repeat
    pub(crate) skin: Skin,
    pub(crate) fonts: FontManager,
    pub(crate) lang: Language,
    pub(crate) refs: RefTrees,             // named resource trees for @refs
    pub(crate) values: ValueGroup,
    pub(crate) anims: Animations,
    pub(crate) msgs: MessageQueue,
    pub(crate) registry: Registry,         // widget factories (H-06)
    pub(crate) listeners: Listeners,       // global listeners
    pub(crate) services: Services,         // loader, image decoder, log sink
    pub(crate) config: Config,             // DPI, long-click delay, pan threshold, pixels per line
    pub(crate) now: Millis,                // set by the host
    pub(crate) pending: Pending,           // internal deferred queue
    pub(crate) scratch: Scratch,           // reusable Vec<WidgetId> buffers for snapshot walks
    pub(crate) repaint: bool,
}
pub type Millis = u64;
```

Fields are disjoint on purpose: paint borrows `tree` and `skin` immutably and `fonts` mutably at the same time (M-06), which Rust allows because they are separate fields.

### Construction

```rust
let mut ui = Ui::new(Config::default(), Services::default());   // std+png: FsLoader("."), PngDecoder
ui.load_skin("resources/default_skin/skin.tb.txt")?;
ui.fonts_mut().add_font("Segoe", "resources/default_font/segoe_white_with_shadow.tb.txt");
ui.fonts_mut().set_default(FontDesc::new(tbid!("Segoe"), 14));
ui.set_viewport(Size::new(1280, 720));                             // sizes the root node
let win = ui.add(ui.root(), Window::default());
ui.inflate_file(ui.content_root(win), "ui/main.tb.txt")?;
```

`Ui::new` registers the built-in widget factories (H-06). Apps add their own with `ui.register::<MyWidget>("MyWidget")`.

### Host loop

```rust
loop {
    ui.set_time(now_ms());                    // time is data, not a service (section 10)
    for e in platform.poll_events() {          // the backend's input adapter does this mapping
        match e {
            PointerDown(x, y, clicks, mods) => { ui.pointer_down(Point::new(x, y), clicks, mods); }
            Key(ch, special, mods, down)    => { ui.key(ch, special, mods, down); }
            Resize(w, h)                    => ui.set_viewport(Size::new(w, h)),
            ...
        }
    }
    ui.update();                              // animations, timers, messages, states, layout, on_process
    if ui.take_repaint() { ui.paint(&mut renderer); }
    platform.wait_until(ui.next_wake_time()); // None = wait for input only
}
```

- **Entry points** (each is a "top-level call" that ends with a flush, section 8): `pointer_down/up/move`, `wheel`, `key`, `touch_down/up/move/cancel`, `file_drop`, `update`, `paint`, `inflate*`, plus every public mutating method when called from outside a hook.
- `update()` merges TB's `TBAnimationManager::Update`, `TBMessageHandler::ProcessMessages`, `InvokeProcessStates` and `InvokeProcess` into one call, in that order.
- `next_wake_time()` replaces `TBSystem::RescheduleTimer`: it returns `Some(now)` while animations or scrollers are running or immediate messages are queued, otherwise the earliest delayed message or long-click deadline, otherwise `None`.
- `take_repaint()` replaces the `OnInvalid` chain: any invalidation sets one flag. No dirty regions (TB has none either).

---

## 6. The `Widget` trait

One trait with default methods for every hook. Built-in and app widgets implement it the same way. `Widget: Any` makes typed access work through trait upcasting (`&dyn Widget as &dyn Any`, stable since Rust 1.86), so no `as_any` boilerplate is needed (H-09).

```rust
pub trait Widget: Any {
    // ---- identity ----
    fn type_name(&self) -> &'static str;                      // "Button"; used by skin defaults and debug

    // ---- lifecycle (all get &mut self + ctx; ctx.this() is the node) ----
    fn on_create(&mut self, cx: &mut Ctx) {}                   // build internal child nodes (H-11)
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {}   // custom properties; generic ones already applied
    fn on_added(&mut self, cx: &mut Ctx) {}
    fn on_removed(&mut self, cx: &mut Ctx) {}                  // after detach; descendants still resolve
    fn on_child_added(&mut self, cx: &mut Ctx, child: WidgetId) {}
    fn on_child_removed(&mut self, cx: &mut Ctx, child: WidgetId) {}
    fn on_die(&mut self, cx: &mut Ctx) -> DieAction { DieAction::Remove }
    fn on_focus_changed(&mut self, cx: &mut Ctx, focused: bool) {}
    fn on_capture_changed(&mut self, cx: &mut Ctx, captured: bool) {}
    fn on_visibility_changed(&mut self, cx: &mut Ctx) {}
    fn on_skin_changed(&mut self, cx: &mut Ctx) {}
    fn on_font_changed(&mut self, cx: &mut Ctx) {}
    fn on_scroll(&mut self, cx: &mut Ctx, delta: Point) {}

    // ---- frame ----
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool { false }   // true = handled, stop bubbling
    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {}
    fn on_process_states(&mut self, cx: &mut Ctx) {}
    fn on_process(&mut self, cx: &mut Ctx) {}
    fn on_process_after_children(&mut self, cx: &mut Ctx) {}

    // ---- layout ----
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        cx.default_measure_content(sc)                          // union of children
    }
    fn arrange(&mut self, cx: &mut Ctx, old: Size, new: Size) {
        cx.default_arrange(old, new)                            // gravity anchoring
    }

    // ---- paint (read-only, decision 3) ----
    fn paint(&self, p: &mut PaintCtx) {}
    fn paint_after_children(&self, p: &mut PaintCtx) {}

    // ---- generic properties (H-07: the old property interface, typed) ----
    fn value(&self) -> Value { Value::Null }
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {}
    fn text(&self) -> Option<&str> { None }
    fn set_text(&mut self, cx: &mut Ctx, text: &str) {}
    fn set_axis(&mut self, cx: &mut Ctx, axis: Axis) {}
    fn axis(&self) -> Axis { Axis::X }

    // ---- structure and routing queries (&self: no take-out needed) ----
    fn content_root(&self) -> Option<WidgetId> { None }         // None = self
    fn event_destination(&self, this: NodeRef) -> Option<WidgetId> { this.parent() }
    fn hit_test(&self, this: NodeRef, p: Point) -> HitStatus { HitStatus::default_for(this, p) }
    fn child_translation(&self) -> Point { Point::ZERO }        // scroll offset applied to children
    fn clips_children(&self) -> bool { false }
    fn scroll_info(&self) -> Option<ScrollInfo> { None }
    fn scroll_to(&mut self, cx: &mut Ctx, p: Point) {}
    fn scroll_root(&self) -> Option<WidgetId> { None }
    fn custom_skin_condition(&self, cond: &Condition) -> bool { false }
}

pub enum DieAction { Remove, KeepAlive /* the widget started an animation that removes it */ }
```

Notes:

- **No "call the base" rule.** The framework applies the generic part before or after each hook (generic inflate properties before `on_inflate`, attaching to the parent before `on_added`, skin padding around `measure_content`). Overriding a hook never breaks the framework (H-07).
- `SetValue`, `SetValueDouble` and `GetValue` collapse into `value()` and `set_value()` on `Value`.
- Query methods take `&self` (and a `NodeRef` where they need node data) so that the framework can ask them without a take-out, including during paint and hit testing.
- The roughly 35 TB hooks map onto these; `OnResized` became `arrange` (section 12), `OnPaintChildren` became the framework walk plus `paint_after_children` (section 14), `OnInvalid` is gone (one repaint flag), `OnInflateChild` is gone (the framework sizes inflated children unless the parent's `arrange` does it).

### Typed access

```rust
ui.get::<Button>(id) -> Option<&Button>
ui.get_mut::<Button>(id) -> Option<&mut Button>            // direct, outside hooks; no side effects
ui.with_widget::<Button, R>(id, |b, cx| ...) -> Option<R>  // take-out, gives a Ctx (section 7)
ui.find_as::<Button>(root, tbid!("ok")) -> Option<WidgetId>
```

`get_mut` is a plain borrow of the behaviour and cannot reach `Ui`, so it is only for setting fields. Anything that needs side effects goes through `with_widget`.

### Registry

```rust
pub struct Factory {
    pub name: &'static str,
    pub create: fn() -> Box<dyn Widget>,
    pub sync: ValueKind,            // which Value kind `connection:` binds (Int, Float, Str)
    pub inflate_z: Z,               // where inflated children go (Button: Bottom)
}
ui.register::<W: Widget + Default>("Name")        // or ui.register_factory(Factory { .. })
```

Factories are keyed by `TbId::new(name)` in a `BTreeMap`, so lookup during inflation is an integer compare rather than TB's linear string scan.

---

## 7. Calling hooks: `Ctx`, take-out and external callbacks

This section implements the agreed mutability decisions 1 to 4.

### `Ctx`

```rust
pub struct Ctx<'a> { ui: &'a mut Ui, this: WidgetId }
impl Ctx<'_> { pub fn this(&self) -> WidgetId { self.this } }
impl Deref for Ctx<'_> { type Target = Ui; ... }        // the full Ui API is available
impl DerefMut for Ctx<'_> { ... }
```

A hook gets `&mut self` (its own behaviour, already taken out) and `&mut Ctx` (all of `Ui`, including its own node's shared data). The one thing it cannot do is reach its own behaviour through `cx.get::<Self>(cx.this())`, which returns `None` because the box is out. That is documented on `get`.

### `call_hook`

```rust
pub(crate) fn call_hook<R>(ui: &mut Ui, id: WidgetId,
                           f: impl FnOnce(&mut dyn Widget, &mut Ctx) -> R) -> Option<R> {
    let node = ui.tree.nodes.get_mut(id)?;               // stale id → None
    let Some(mut b) = node.behaviour.take() else {       // busy (re-entrant on the same node)
        ui.warn_busy(id);                                 // debug builds only
        return None;
    };
    let r = f(&mut *b, &mut Ctx { ui, this: id });
    if let Some(node) = ui.tree.nodes.get_mut(id) { node.behaviour = Some(b); }
    // else: unreachable in practice, because slots are only freed at flush (section 8)
    Some(r)
}
```

- `None` means "not handled": an event bubbling into a busy node skips it and continues to the next destination; a measure on a busy node returns the cached size, or zero with a warning.
- Because removal only detaches and freeing waits for the flush, the slot always still exists at put-back time. The `else` branch is defensive only.

### External callbacks

Global and per-widget listeners, animation callbacks and app event handlers are not methods of the widget they look at. They receive `&mut Ui` and a `WidgetId`:

```rust
pub trait UiListener {
    fn on_added(&mut self, ui: &mut Ui, id: WidgetId) {}
    fn on_removing(&mut self, ui: &mut Ui, id: WidgetId) {}
    fn on_dying(&mut self, ui: &mut Ui, id: WidgetId) -> bool { false }   // true = I will remove it
    fn on_focus_changed(&mut self, ui: &mut Ui, old: Option<WidgetId>, new: Option<WidgetId>) {}
    fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool { false } // true = swallow
}
ui.add_listener(Box<dyn UiListener>) -> ListenerId;             // global
ui.add_widget_listener(id, Box<dyn UiListener>) -> ListenerId;  // only for events on `id`
ui.remove_listener(ListenerId);

// Sugar for app code: a per-widget event handler closure
ui.on_event(id, |ui, ev| { if ev.kind == EventKind::Click { ...; true } else { false } });
```

They read and write node data directly and reach a behaviour only through `ui.with_widget::<T>(id, |w, cx| ..)`, which does the take-out internally.

**Calling listeners safely (M-05).** Listener boxes live in an arena keyed by `ListenerId`. To call them, the dispatcher copies the relevant `ListenerId`s into a scratch buffer, and for each one takes the box out of its slot, calls it, and puts it back if the slot still exists. A listener removed during the call is dropped at put-back; a listener added during the call is not called in this round. This is the same take-out rule as for widgets.

---

## 8. Changing the tree

### Creating and attaching (immediate)

```rust
ui.add(parent, W) -> WidgetId                     // create + attach on top; the common case
ui.add_at(parent, W, Z::Bottom | Z::Top) -> WidgetId
ui.add_relative(reference, W, Before | After) -> WidgetId
ui.create(W) -> WidgetId                          // unattached; attach later
ui.attach(parent, child, z) / ui.attach_relative(reference, child, rel)
ui.set_z(id, Z::Top | Z::Bottom)
```

Order of effects for `add`: insert the node, call `on_create` (which may add internal children), attach to `content_root(parent)` (if the parent redirects), call the parent's `on_child_added`, the child's `on_added`, global listeners' `on_added`, then `invalidate_layout(parent)`. All immediate, so code right after `add` or `inflate` can find and focus the new widgets (as TB code expects).

A node created with `create` and never attached stays in the arena until `remove`. It is not part of the tree, so it gets no input, layout or paint.

### Removing (detach now, free later)

```rust
ui.remove(id)      // immediate detach
ui.die(id)         // graceful: lets the widget animate out first
```

`remove(id)` runs, in order:

1. Global listeners' `on_removing(id)`, then the node's own `on_removed`, then the parent's `on_child_removed`.
2. Take `id` out of its parent's `children` and clear `parent`.
3. Input fix-up: if hovered, captured or focused is `id` or inside its subtree, clear it (focus moves to the window's next focusable, as TB's `RemoveFocus` does); same for touch entries and a pending long-click.
4. Abort animations that target the subtree, cancel its queued messages, remove its listeners.
5. Mark every node in the subtree `DETACHED` and push `id` onto `pending.free`.

From step 5 on, public lookups (`node`, `get`, `find`, `with_widget`) treat `DETACHED` nodes as gone, so behaviour is immediately as if the widget was deleted. Only the framework (for put-back) still sees the slot.

**Flush.** At the end of every top-level call, `flush()` frees each pending subtree (post-order: arena slots and every side-table entry). Freeing calls no hooks. Behaviours are dropped with normal Rust `Drop`, and there is no `OnWidgetDelete` (H-19). `flush()` is a no-op when called inside a hook, so nested public calls never free early. A depth counter on `Ui` tells "top-level" from "nested".

### Dying (`die`)

1. If `DYING` or detached already, return.
2. Set `DYING`. A dying node and its subtree get no input and cannot take focus (focus moves away now).
3. Call `on_die`. `DieAction::KeepAlive` means the widget itself started an animation with `OnDone::Remove` (section 11), for example `Window` fading out. Otherwise ask global listeners' `on_dying`; any `true` keeps it alive.
4. If nobody kept it alive, `remove(id)`.

This replaces TB's global animation listener that type-checked `TBWindow`, `TBMessageWindow` and `TBDimmer` (H-09): those widgets now decide for themselves in `on_die`.

### Walking while changing (M-02, M-03)

Every framework walk (process, process states, event fan-out, listener fan-out, removal cascade) iterates over a snapshot of the child IDs in a scratch buffer, and skips any ID that no longer resolves or whose parent changed. Children added during a walk are not visited in that pass. TB's linked list would visit children appended after the cursor; this is accepted as a minor difference (check `SelectList` rebuilds against it at M6). Paint and hit testing borrow the tree immutably and need no snapshot.

### Internal deferred queue

`pending` holds only what cannot happen immediately:

- subtrees waiting to be freed;
- a repaint request from paint (a flag);
- skin-state recomputation requests (`SKIN_STATE_DIRTY` nodes), consumed by the next `update`.

It has no public API (decision 4). Paint is read-only, so nothing else needs queueing yet. If real widgets need more during paint, the rule is relaxed then (decision 3), not designed for now.

---

## 9. Events and input

### `Event`

```rust
pub struct Event {
    pub kind: EventKind,
    pub target: WidgetId,           // the original target; stays fixed while bubbling
    pub pos: Point,                 // in the target's coordinates (TB does not convert while bubbling)
    pub delta: Point,               // wheel and move deltas
    pub count: u32,                 // click count (helper: count_cycle(n))
    pub key: Option<char>,
    pub special: SpecialKey,
    pub mods: Modifiers,
    pub ref_id: TbId,               // "copy", "paste", button id in MessageWindow …
    pub touch: bool,
    pub value: Option<Value>,       // CHANGED only, and only when the widget has a connection
    pub payload: Option<Rc<dyn Any>>, // FileDrop paths, Custom data (replaces event subclasses, H-09)
}
pub enum EventKind { Click, LongClick, PointerDown, PointerUp, PointerMove, Wheel, Changed,
                     KeyDown, KeyUp, Shortcut, ContextMenu,
                     TouchDown, TouchUp, TouchMove, TouchCancel, FileDrop, Custom(TbId) }
```

### Dispatch (`ui.invoke_event(target, ev) -> bool`)

1. Per-widget listeners for `target`, then global listeners (`on_event`). Any `true` swallows the event. If `target` is now gone, return `true` (TB: "we got removed, so we handled it").
2. `Changed`: mark skin states dirty, and if `ev.value` is set, sync the value group (below).
3. `Click`, `LongClick`, `Changed`, `KeyDown`, `KeyUp`: mark widget states dirty for the next `update`.
4. Bubble: `cur = target`; loop: `call_hook(cur, on_event)`; stop if handled; else `cur = event_destination(cur)` (re-read after each call, so a hook that moved or removed things is respected); stop at `None`. A busy node is skipped.

Widgets fire events at themselves through `cx.emit(ev)`, which sets `target = cx.this()`.

### Pointer pipeline

| Entry point | Behaviour (TB's, in the new structure) |
|---|---|
| `pointer_down(p, clicks, mods)` | If nothing is captured: hit-test from the root (front to back, skipping `IGNORE_INPUT`, invisible, dying and disabled nodes, using `hit_test`), set `captured` and `hovered`. Activate the containing window (bring to top, `ACTIVE_WINDOW`, restore its `last_focus`). Stop running scrollers up the chain (and suppress the click if one was moving). Focus the nearest focusable ancestor. Arm the long-click deadline if `WANT_LONG_CLICK`. Send `PointerDown` to the captured node in its coordinates. |
| `pointer_move(p, mods)` | Update `hovered`. Send `PointerMove` to captured (or hovered). If unhandled while a button is down, start panning past `config.pan_threshold`: scroll the nearest ancestor with `scroll_info`, set `panning`, cancel the click and long-click. |
| `pointer_up(p, mods)` | Send `PointerUp`. If not cancelled and `hit_test` still hits the captured node, send `Click`. Release capture. |
| `wheel(p, delta, mods)` | Update hover, send `Wheel` to captured or hovered (scrollables handle it). |
| Long click | `update()` checks `input.long_click: Option<(WidgetId, Millis)>`. On expiry: send `LongClick`, and if unhandled `ContextMenu`; if either is handled, the coming click is cancelled. |
| `touch_*(touch_id, …)` | Touch 0 maps to the pointer path; others go to `input.touches: BTreeMap<u32, TouchInfo>` and send `Touch*` events. |

### Keyboard

`key(ch, special, mods, down)` goes to `focused`. `CLICK_BY_KEY` nodes turn Enter and Space into press and click, tracked in `input.key_click` (replacing TB's function-local statics, H-04). Otherwise `KeyDown`/`KeyUp` bubbles. An unhandled Tab moves focus through the window (`next_deep` / `prev_deep`). The host sends `Shortcut` events itself (`ui.shortcut(ref_id)`), as in TB.

### Focus

`set_focus(id) -> bool`: refuse if not focusable, disabled, invisible, dying or detached. Swap `input.focused`, call `on_focus_changed(false)` on the old and `on_focus_changed(true)` on the new, then listeners, re-checking after each call whether focus changed again (TB's re-entrancy rule, H-12). Record `last_focus` for the containing window.

### Value binding (M-08)

`connections: SideTable<TbId>` maps a widget to a value name in `ValueGroup { values: BTreeMap<TbId, Bound { value: Value, widgets: Vec<WidgetId>, syncing: bool }> }`.

- Widgets fire `Changed` with `cx.emit_changed(self.value())`. The framework keeps the value in `ev.value` only when the node has a connection, and drops it otherwise (a refcount bump at most).
- Dispatch step 2 stores the value and, with `syncing` set, calls `set_value` on every other connected widget through `call_hook`. Those widgets may fire their own `Changed`; `syncing` stops the loop. No widget ever reads back from the busy sender.
- Strings are `Rc<str>`, so this costs a refcount bump per widget (decision 5).

Radio groups keep TB's mechanism: `group_id` plus the nearest `GROUP_ROOT` ancestor; a radio button deselects the others in that subtree.

---

## 10. Messages, timers and time

### Time is data (decided)

There is no `Clock` service. The host calls `ui.set_time(now_ms)` (any monotonic milliseconds) before feeding input and calling `update`. Everything that needs time (animations, delayed messages, long click, kinetic scroll, caret blink) reads `ui.now()`. This removes a trait, keeps the core free of `std::time`, and makes tests deterministic: a test sets the time it wants. The demo uses `std::time::Instant`.

### Messages

```rust
pub struct Message { pub id: TbId, pub data: Value }      // TB's v1/v2/id1/id2 fit in Value::Array or Object
ui.post_message(target, id, data);                          // delivered on the next update
ui.post_message_delayed(target, id, data, ms);
ui.post_message_at(target, id, data, at);
ui.cancel_messages(target, Some(id) | None);
ui.has_message(target, id) -> bool;
```

- The queue is `immediate: VecDeque<Entry>` plus `delayed: Vec<Entry>` sorted by `(fire_at, seq)`. Entries name their target `WidgetId` (H-01, H-03).
- `update()` pops due delayed entries first, then immediate ones, one at a time, and calls `on_message` through `call_hook`. Handlers can post freely because the queue is not borrowed during the call (M-05). Entries for detached targets are dropped.
- Messages posted during delivery wait for the next `update`, so a widget that re-posts to itself every frame cannot starve the loop.
- **Only widgets receive messages.** TB's non-widget handlers become framework code: kinetic scrolling and long click are driven directly by `update()` rather than by private message handlers (H-08). App code that wants timers uses a widget (for example its window).

---

## 11. Animation

```rust
pub struct AnimSpec {
    pub target: WidgetId,
    pub prop: AnimProp,           // Opacity { from, to } | Rect { from, to } | RectDelta { in/out, delta }
    pub curve: Curve,             // Linear, SlowDown, SpeedUp, Bezier, Smooth (TB's five)
    pub duration_ms: u32,
    pub start: Start,             // Immediately | FirstUpdate (TB's ANIMATION_TIME_* flags)
    pub on_done: OnDone,          // Nothing | Remove | Die
}
ui.animate(spec) -> AnimId;
ui.animate_custom(Box<dyn Animate>, curve, duration) -> AnimId;   // app animations
ui.abort_animation(AnimId, jump_to_end: bool);
ui.abort_animations_for(target);
ui.block_animations(|ui| ...);                                     // new animations finish instantly
```

- Built-in animations write node data (`opacity`, `rect` through `set_rect` so layout follows) and never touch behaviours, so they cannot collide with hooks.
- `Animate` is an external callback: `fn update(&mut self, ui: &mut Ui, progress: f32)` and `fn stop(&mut self, ui: &mut Ui, aborted: bool)`.
- `update()` takes the animation list with `mem::take`, advances each, applies completions (`OnDone::Remove` → `remove(target)`), then merges back any animations started during the pass. A stale target drops its animation silently.
- Fade-in on add and fade-out on die are done by `Window`, `MessageWindow` and `Dimmer` themselves (`on_added` and `on_die`), not by a global listener.

---

## 12. Layout

### Types

```rust
pub struct SizeConstraints { pub available: Size }        // NO_RESTRICTION = 10_000, as TB
pub struct PreferredSize { pub min: Size, pub max: Size, pub pref: Size, pub dependency: SizeDep }
pub struct LayoutParams { pub min: Option<Size>, pub max: Option<Size>, pub pref: Option<Size> }  // per axis Option<i32> in practice
pub enum Gravity bits: LEFT, TOP, RIGHT, BOTTOM (ALL, LEFT_RIGHT, …)
pub enum Visibility { Visible, Invisible, Gone }
```

### Measure (M-04)

```rust
ui.preferred_size(id, sc) -> PreferredSize
```

1. `Gone` → zero size.
2. Apply the node's `LayoutParams` to `sc`.
3. If `measure_cache` holds `(sc', ps)` with `sc' == sc` or `ps.dependency == None`, return `ps`.
4. Otherwise call `measure_content(sc - padding)` through `call_hook`, then add the skin element's padding and apply the skin's `width`/`height`/`min-*`/`max-*`/`pref-*` and the intrinsic bitmap size, as TB's default `OnCalculatePreferredSize` does.
5. Apply `LayoutParams` overrides, store the cache, return.

A parent's `measure_content` calls `cx.preferred_size(child, sc)` for each child. That is a nested `call_hook` on a different node, which the take-out model allows. The cache is plain node data, so there is no hidden mutation behind a `&self` getter (H-18).

### Arrange (decided: replaces `OnResized` as the layout hook)

- `ui.set_rect(id, r)` stores the rect; if the size changed it calls `arrange(old, new)` immediately (TB lays out in `OnResized` immediately) and requests a repaint.
- `ui.invalidate_layout(id)` clears `measure_cache` on `id` and every ancestor and sets `NEEDS_ARRANGE` on them (TB's recursive invalidation, walking `parent` IDs, H-03).
- The process pass in `update()` walks top-down and calls `arrange(size, size)` on every `NEEDS_ARRANGE` node before its `on_process`, then clears the flag. Children arranged by their parent are reached later in the same pass.
- The default `arrange` is TB's gravity anchoring for plain parents (a child with only right gravity follows the right edge, left and right gravity stretches, and so on), which uses the `old` and `new` sizes. Inflated children with an empty rect are first sized to their preferred size (TB's `OnInflateChild`).

### The stack layout

`Layout` (TB's `TBLayout`) keeps TB's algorithm and settings table (axis, spacing, size, position, distribution, distribution-position, overflow clip or scroll, reverse order). `stack.rs` is written once for the X axis with the same rotate-in, rotate-out helpers (`rot_rect`, `rot_size`, …) as TB. In calculate mode (`measure_content`) and in arrange mode it is the same function, as in TB. Overflow scroll uses `child_translation`, `scroll_info` and `scroll_to`.

### Dimensions

`DimensionConverter` converts `dp` (default, relative to the skin's `base-dpi`), `px` and `mm`, with the skin choosing the destination DPI from `supported-dpi` (which also picks the `@192`/`@288`/`@384` bitmap variants). It lives in `Skin` and is passed to the reader.

---

## 13. Skin

### Data

```rust
pub struct Skin {
    elements: BTreeMap<TbId, SkinElement>,
    atlas: Atlas,                         // CPU-side pages; uploaded at paint (section 14)
    dim: DimensionConverter,
    defaults: SkinDefaults,               // text color, disabled/placeholder opacity, spacing
}
pub struct SkinElement {
    pub id: TbId,
    pub bitmap: Option<FragmentId>,
    pub kind: SkinKind,                    // StretchBox | StretchBorder | StretchImage | Tile | Image
    pub cut: i32, pub expand: i32, pub padding: Edges,
    pub sizes: SkinSizes,                  // width/height/min/max/pref, intrinsic
    pub spacing: Option<i32>, pub content_ofs: Point, pub img_pos: ImgPos,
    pub flip_x: bool, pub flip_y: bool, pub opacity: f32,
    pub text_color: Option<Color>, pub bg_color: Option<Color>,
    pub tag: Value,
    pub overrides: Vec<StateElement>,
    pub strong_overrides: Vec<StateElement>,
    pub children: Vec<StateElement>,
    pub overlays: Vec<StateElement>,
}
pub struct StateElement { pub element: TbId, pub state: SkinState, pub conditions: Vec<Condition> }
pub struct Condition { pub target: CondTarget, pub prop: CondProp, pub value: TbId, pub test: Test }
```

### Loading

`ui.load_skin(path)` (optionally with an override skin on top, as TB) parses the file, resolves `clone`, decodes each bitmap through the `ImageDecoder` (choosing the DPI variant), converts it to premultiplied RGBA and packs it into the skin atlas. No renderer is needed at load time: bitmaps are created on the next paint. A `SkinListener`-style hook for custom properties is not carried over until someone needs it.

### Conditions

`CondCtx { tree: &Tree, input: &InputState, id: WidgetId }` evaluates targets (this, parent, ancestors, previous and next sibling) and properties (skin, window-active, axis, align, id, state, value, hover, capture, focus, custom) by reading the tree. `custom` calls `custom_skin_condition(&self)` on the behaviour, which is a read-only query. Evaluation needs no mutation.

### Recursion guard (M-07, decided)

TB's `is_painting` / `is_getting` flags guard against override chains that loop. The port passes a depth counter down instead and stops at a fixed limit (8). It needs no mutable state inside a read, and a looping skin simply stops resolving at the limit (debug builds warn once).

### Strong overrides

At `update()`, nodes with `SKIN_STATE_DIRTY` re-resolve their effective element. If a strong override changed it, the framework calls `on_skin_changed` and `invalidate_layout` (TB's `InvokeSkinUpdatesInternal`).

---

## 14. Rendering

### The renderer interface (decided)

```rust
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct BitmapId(pub u32);              // chosen by the renderer

#[derive(Copy, Clone)]
pub struct Quad {
    pub dst: Rect,                         // pixels, screen space (translation already applied)
    pub src: Rect,                         // texels in the bitmap (ignored for solid fills)
    pub color: Color,                      // premultiplied; multiplies the texel (opacity is folded in)
}

pub trait Renderer {
    fn begin_frame(&mut self, viewport: Size) {}
    fn end_frame(&mut self) {}
    /// RGBA8, premultiplied alpha, row-major, `size.w * size.h * 4` bytes. Any size (no power of two).
    fn create_bitmap(&mut self, size: Size, rgba: &[u8]) -> Option<BitmapId>;
    fn update_bitmap(&mut self, bitmap: BitmapId, rgba: &[u8]);      // whole contents, same size
    fn destroy_bitmap(&mut self, bitmap: BitmapId);
    /// Draw quads with one bitmap (None = solid color) inside one clip rect.
    /// Output = texel × color, blended with (ONE, ONE_MINUS_SRC_ALPHA).
    fn draw(&mut self, bitmap: Option<BitmapId>, clip: Rect, quads: &[Quad]);
}
```

This is TB's surface made narrower, not wider (decision 5):

- **Translate, opacity and clip state move into the core.** The paint walk carries translation, opacity and clip as parameters (H-17) and bakes them into each quad and draw call. The backend never sees a state machine, so it can never be left unbalanced.
- **Batching moves into the core.** The core's `Painter` collects quads and calls `draw` when the bitmap or clip changes or the buffer fills (TB's `TBRendererBatcher` logic, kept in the core so every backend gets it for free).
- **One blend mode, one color model.** `DrawBitmap` and `DrawBitmapColored` are the same operation (texel × color). Glyph bitmaps are white with alpha, so the text color does the rest. Solid fills pass `None`.
- **Flush before free is automatic (H-22).** Only the core calls `destroy_bitmap` and `update_bitmap`, and the `Painter` flushes any pending batch first.
- **Context loss is a call, not a listener list.** The host calls `ui.renderer_lost()`: every atlas forgets its `BitmapId`s and marks its pages dirty, and the next `paint` re-creates them. No `TBRendererListener` (H-08, H-22).
- A backend is therefore about 150 lines: three bitmap methods and one `draw`.

### Atlas

```rust
pub struct Atlas { pages: Vec<Page>, page_size: Size }
struct Page { pixels: Vec<u8>, bitmap: Option<BitmapId>, dirty: bool, shelves: ShelfAllocator }
pub struct FragmentId { page: u16, rect: Rect }      // plain value; no back pointers (H-03)
```

TB's row-based space allocator (`TBFragmentSpaceAllocator` / `TBSpaceAllocator`) is kept as a `Vec`-based shelf allocator with TB's space-allocator test ported. The skin, the glyph cache and (later) the image manager each own one `Atlas`. At paint time the `Painter` uploads dirty pages (`create_bitmap` the first time, `update_bitmap` afterwards) before the first draw that uses them. Glyph eviction only happens between frames or after a flush, so a pending batch never references overwritten texels.

### The paint walk

```rust
pub fn paint(&mut self, r: &mut dyn Renderer) {
    let Ui { tree, skin, fonts, input, .. } = self;           // disjoint borrows (M-06)
    let mut painter = Painter::new(r, viewport);
    paint_node(&Env { tree, skin, input }, fonts, &mut painter, tree.root, PaintState::root());
    painter.finish();
}

fn paint_node(env, fonts, painter, id, st: PaintState /* offset, opacity, clip */) {
    let node = env.tree.get(id);
    skip if !visible || opacity == 0 || rect empty || DETACHED;
    let state   = auto_state(node, env.input);              // + hovered/pressed/focused
    let opacity = st.opacity * node.opacity * disabled_factor(state);
    let offset  = st.offset + node.rect.origin();
    let elem    = skin::paint(env, painter, node.skin, local_rect, state, depth=0);   // background
    let b       = node.behaviour.as_deref();                 // &dyn Widget: read-only, no take-out
    b.paint(&mut PaintCtx { .. });
    let child_offset = offset + elem.content_ofs + b.child_translation();
    let clip = if b.clips_children() { st.clip ∩ padding_rect } else { st.clip };
    for &child in &node.children { cull against clip; paint_node(.., child, PaintState { child_offset, opacity, clip }) }
    paint overlays; paint generic focus if focused and the skin has no focus state;
    b.paint_after_children(&mut PaintCtx { .. });
}
```

- Paint borrows the tree immutably, so it needs no snapshot and no take-out (decision 3).
- `PaintCtx` gives the widget its local rect, resolved skin state and element, text color, a read-only `NodeRef` to any node, `&Skin`, `&mut FontManager` (glyphs render lazily, M-06), and drawing helpers: `draw_skin`, `draw_text`, `text_width`, `fill_rect`, `draw_rect`, `draw_fragment`, `push_clip`/`pop_clip` (scoped, with a closure, so it always balances), and `request_repaint` (a flag; the only allowed write).
- Per-widget opacity multiplies vertex color, as TB does. No offscreen targets.

---

## 15. Fonts

```rust
pub struct FontDesc { pub id: TbId, pub size: u16 }      // TB's unused bold/italic bits dropped

pub trait FontBackend {
    /// Try to open `data` (loaded from `path`) at `size`. None = not my format.
    fn open(&self, path: &str, data: &[u8], size: u16, ui: &mut LoadCtx) -> Option<Box<dyn GlyphSource>>;
}
pub trait GlyphSource {
    fn metrics(&self) -> FontMetrics;                          // ascent, descent, height
    fn glyph_metrics(&mut self, c: char) -> Option<GlyphMetrics>;  // advance, offsets, size
    fn rasterize(&mut self, c: char, out: &mut GlyphImage) -> bool; // alpha or premultiplied RGBA
}

pub struct FontManager {
    backends: Vec<Box<dyn FontBackend>>,          // TBBF registered by default
    infos: BTreeMap<TbId, FontInfo>,              // name → path
    faces: BTreeMap<(TbId, u16), FontFace>,       // created on first use
    cache: GlyphCache,                            // one Atlas + BTreeMap<(face, char), Glyph>
    default: FontDesc,
}
```

- `LoadCtx { loader: &mut dyn ResourceLoader, images: &mut dyn ImageDecoder }` lets a backend load companion files (TBBF's PNG strips).
- **TBBF** (TB's bitmap font: a `.tb.txt` description plus a PNG strip per size, with glyph boundaries found from transparent columns) is built in and needs nothing beyond the image decoder.
- Measurement (`text_width`, line height) needs only metrics, so it works outside paint. Rasterization into the glyph cache's CPU atlas happens on first draw; upload happens at paint.
- Font ID 0 stays a built-in fallback that draws boxes, so `face()` never fails.
- The background "shadow" font becomes `Option<(TbId, u16)>` on the face (a key, not a pointer, H-03). Blur (`TBFontEffect`) is deferred to M8 with TrueType, since TBBF fonts bake their shadow.
- Text stays UTF-8 `&str`, one code point per glyph, no shaping, kerning or bidi (as TB).

---

## 16. Resources, inflation and language

### Parser and `ResNode`

```rust
pub struct ResNode { pub name: Rc<str>, pub value: Value, pub children: Vec<ResNode> }
pub fn parse(src: &str, ctx: &mut ParseCtx) -> Result<ResNode, ParseError>   // ParseError { file, line, msg }
impl ResNode { pub fn get(&self, path: &str) -> Option<&ResNode> /* "a>b>c" */ ; ... }
```

- Reads `&str` and never mutates the input (H-15). Same syntax: tab indentation, `name value`, compact `Name: k: v, k: v`, quoted and multi-line values, backslash escapes, comments.
- `@file` loads through the `ResourceLoader`; `@include` clones from a ref tree or earlier in the file; `@if`/`@else` resolve after parsing; cycles are caught with an explicit stack of include paths (no parent pointers).
- Ref trees: `ui.refs: BTreeMap<TbId, ResNode>`, set with `ui.set_ref_tree(name, node)`. Live change notification (`TBNodeRefTreeListener`) is deferred until a widget needs it.
- Port TB's `test_tb_parser.cpp` and its three `.tb.txt` files first; they are the best spec of the format.

### Inflation

```rust
ui.inflate(parent, src: &str) -> Result<(), InflateError>
ui.inflate_file(parent, path) -> Result<(), InflateError>
ui.inflate_node(parent, &ResNode) -> Result<(), InflateError>
```

For each node: look up the factory by `TbId::new(name)`, create the behaviour, insert it and run `on_create`, apply the generic properties (`id`, `group-id`, `value`, `data`, `is-group-root`, `is-focusable`, `want-long-click`, `ignore-input`, `opacity`, `text`, `connection`, `axis`, `gravity`, `visibility`, `state`, `skin`, `rect`, `lp>*`, `font>name|size`), call `on_inflate` with the node for custom properties, attach at the factory's `inflate_z`, recurse into children (into `content_root`), and finally apply `autofocus`. Unknown widget names are an error that names the line. Attaching is done by the framework, never by a hook (H-07).

### Language

`Language { strings: BTreeMap<TbId, Rc<str>> }`, loaded additively from TB-format files. `@key` strings are resolved when a resource value is read. A missing key returns `"<TRANSLATE!>"` (debug builds: `"<TRANSLATE:key>"`, built per call rather than in a static, H-04).

---

## 17. Services

The services table in [decisions-and-status.md](decisions-and-status.md) is refined here (decided, Steve 2026-11-17).

| Service | Trait | M1 implementations | Notes |
|---|---|---|---|
| Resource loader | `ResourceLoader { fn load(&mut self, path: &str) -> Result<Cow<'static, [u8]>, LoadError>; fn resolve(&self, base: &str, rel: &str) -> String }` | `MemLoader` (a table of `include_bytes!` entries, `no_std`), `FsLoader` (`std`) | Needed for skins, fonts, `@file` |
| Image decoder | `ImageDecoder { fn decode(&mut self, bytes: &[u8]) -> Result<Image, DecodeError> }` | `PngDecoder` (`png` feature, hand-rolled; `no_std`) | `Image { size, rgba: Vec<u8> }`, straight alpha; the core premultiplies |
| Font rasteriser | `FontBackend` (section 15) | TBBF (built in) | TrueType at M8 |
| Log sink | `type LogSink = fn(Level, core::fmt::Arguments)` | stderr (`std`), or none | Used for busy-hook warnings, skin and inflate warnings |
| Clock | **none**: `ui.set_time(now)` | | Section 10 |
| Timer wake-up | **none**: `ui.next_wake_time()` | | Section 5 |
| Clipboard | `Clipboard { get, set, has_text }` | **not in M1** (arrives with the edit field, M7, with an in-memory default) | |
| Platform metrics | `Config` struct (not a trait) | TB's defaults | DPI, long-click delay, pan threshold, pixels per line |

```rust
pub struct Services {
    pub loader: Box<dyn ResourceLoader>,
    pub images: Box<dyn ImageDecoder>,
    pub log: Option<LogSink>,
}
impl Default for Services { /* std: FsLoader::new("."), png: PngDecoder, std: stderr */ }
```

Without `std`, `Services::default()` does not exist and the host builds the struct itself.

---

## 18. The three open choices

These were put to Steve on 2026-11-17 and all three are decided. The outcome is recorded in [decisions-and-status.md](decisions-and-status.md).

### 18.1 Which renderer backend ships first

**Decided (Steve, 2026-11-17): a software renderer first (M3), then a miniquad backend (M4).**

- **`turbo-ferret-soft`** draws quads into an RGBA buffer: a clipped, scaled blit with premultiplied blending. It has zero dependencies and is `no_std`, so it doubles as the embedded story (write into a framebuffer). Its main job is testing: every paint test renders headless and compares against a golden image, so layout, skin and font work can be verified in CI before any window exists.
- **`turbo-ferret-miniquad`** is the first GPU backend and drives the demo. miniquad gives a window, input and GL 3 / GLES 2+ / WebGL on desktop, Android and the web, and its dependencies are only platform system crates (`libc`, `winapi`, `objc-rs`, `ndk-sys`, per target). The backend is one shader (texel × color), one dynamic vertex buffer and a scissor, plus an adapter that maps miniquad input events to `Ui` calls.
- Considered and not proposed first: `glow` + `winit`/`glutin` (more crates, and windowing becomes the demo's problem); `wgpu` (a large dependency tree, against the "few deps" priority). Either can be added later as its own crate, since the trait is six methods.

### 18.2 Which services go into the first milestone

**Decided (Steve, 2026-11-17): resource loader, image decoder and TBBF fonts only; drop Clock and Timer as services; defer Clipboard and TrueType.**

- In: `ResourceLoader` (`MemLoader` + `FsLoader`), `ImageDecoder` (`PngDecoder`), TBBF as the only `FontBackend`, and the log sink.
- Removed as services: Clock becomes `ui.set_time(now)`, Timer wake-up becomes `ui.next_wake_time()`. Both are plain data, which is simpler and makes tests deterministic.
- Deferred: Clipboard (nothing needs it until the edit field, M7) and TrueType (M8). The basic widgets render with the default TBBF font.

### 18.3 Which PNG decoder

**Decided (Steve, 2026-11-17): hand-rolled, in the core, behind the `png` feature.** This keeps the core at zero dependencies. The other options considered were `minipng` (no deps, but still a crate), `zune-png` (two deps) and `png` (five deps, needs `std`).

**Scope**, sized to what TB's assets need (all 120 PNGs in `resources/default_skin`, `resources/default_font` and `Demo/fonts` are 8-bit and non-interlaced: gray+alpha, grayscale, palette and RGBA):

- `inflate.rs`: a zlib/DEFLATE decompressor supporting stored, fixed-Huffman and dynamic-Huffman blocks, with table-driven decoding. Check the Adler-32 at the end. About 350 to 450 lines.
- `png.rs`: signature, chunk walk, `IHDR`, `PLTE`, `tRNS` (palette and single-color transparency), concatenated `IDAT`s, `IEND`. The five scanline filters (None, Sub, Up, Average, Paeth). Color types 0, 2, 3, 4 and 6 at bit depth 8, plus palette and grayscale at 1, 2 and 4 bits (cheap, and common in small icons). Output is straight-alpha RGBA8; the core premultiplies. About 250 to 350 lines.
- **Not supported**, each failing with a clear `DecodeError`: Adam7 interlacing, 16-bit channels, and ancillary chunks (gamma, ICC, text), which are skipped. Chunk CRCs are skipped too, since the Adler-32 covers the image data. A host that needs more can plug another decoder in through `ImageDecoder`.
- **Robustness:** no `unsafe`, no panics on any input (all indexing checked or bounded), and limits on image size and decompressed length so a crafted file cannot exhaust memory.
- **Testing:** see M3 in section 20 (comparison with the `png` crate as a dev-only oracle, corrupt-input tests and a fuzz target).

---

## 19. Hazard coverage

Every hazard from the architecture doc, and where this design handles it.

| Hazard | Handled by | Section |
|---|---|---|
| H-01 Intrusive lists | `Vec<WidgetId>` children; arenas and `Vec`s for listeners, messages, animations, atlas shelves, glyphs | 4, 7, 10, 11, 14 |
| H-02 Safe iterators | Snapshot walks that skip stale IDs; take-out for listeners | 7, 8 |
| H-03 Raw back pointers | `parent: Option<WidgetId>`; `FragmentId` is a value; font shadow is a key; connections are a side table | 4, 9, 14, 15 |
| H-04 Static input state | `InputState` on `Ui`, including key-click and touches | 5, 9 |
| H-05 Singletons | Fields of `Ui`; renderer passed to `paint` | 5, 14 |
| H-06 Static-init factories | `Registry` filled in `Ui::new` and by `ui.register` | 6 |
| H-07 Fat base, "call base" | `Node` data + `Widget` trait with defaults; framework does the generic part | 4, 6 |
| H-08 Mixins | Side tables, framework-driven scroller and long click, listeners as external callbacks | 4, 7, 9, 10 |
| H-09 Custom RTTI | `Widget: Any` + trait upcasting; event payload instead of subclasses; die handled by the widget, not a type check | 6, 8, 9 |
| H-10 Self-deletion | Detach now, free at flush; IDs are weak references | 8 |
| H-11 Children by value | Internal parts are child nodes made in `on_create`, stored as `WidgetId` fields | 6, 8 |
| H-12 Re-entrancy | Take-out/put-back, busy = `None`; snapshot walks; focus re-checks | 7, 8, 9 |
| H-13 `TBValue` flags | `Value` enum with `Rc`; non-mutating getters | 3 |
| H-14 Packed bitfields | Hand-rolled flag types | 4 |
| H-15 C strings | `&str`, `Rc<str>`; parser does not mutate input | 3, 16 |
| H-16 Out-params, bool errors | Return values, `Option`, `Result` | throughout |
| H-17 Renderer state machine | Translation, opacity, clip are walk parameters; scoped `push_clip` | 14 |
| H-18 Mutating reads | Measure cache is plain node data written by `Ui`; skin recursion is a depth parameter | 12, 13 |
| H-19 Destructor callbacks | Removal fires hooks before detaching; freeing calls nothing | 8 |
| H-20 Build-flag layout | Cargo features; `TbId` size fixed; debug strings in a side map | 2, 3 |
| H-21 Compile-time hashing | `const fn` + `tbid!` | 3 |
| H-22 Context loss, flush before free | Core owns batching and bitmap lifetime; `ui.renderer_lost()` | 14 |

---

## 20. Milestone plan

Each milestone ends with green tests and a working `cargo build -p turbo-ferret --no-default-features --features png --target thumbv7em-none-eabihf` (the `no_std` check). M0 to M4 together are the "first milestone" scope from the status doc (core + layout + skin + TBBF fonts + basic widgets). M5 onward is later work, in a suggested order.

### M0. Foundations

- Workspace, CI (fmt, clippy, tests, `no_std` target build), MSRV pinned.
- `id`, `geom` (with `Color::parse`), `value`, `arena` (`Arena`, `WidgetId`, `SideTable`), `math`.
- `res`: parser, `ResNode`, `@file`/`@include`/`@if`, ref trees; `lang`; `services` with `MemLoader` and `FsLoader`.
- **Tests ported from TB:** `test_tb_parser` (with its three `.tb.txt` files), `test_tb_value`, `test_tb_node_ref_tree`, `test_tb_color`, `test_tb_geometry`. New: arena generation and stale-ID tests, `SideTable` tests, FNV-1a known values.
- **Done when** the TB resource files in `resources/` and `Demo/demo01/ui_resources/` all parse.

### M1. Tree and dispatch (headless)

- `Node`, `Tree`, `Ui` skeleton, `Ctx`, `call_hook`, `with_widget`, `get`/`get_mut`, `find`.
- `add`/`create`/`attach`/z-order, `remove`, `die`, `flush`, snapshot walks, `pending`.
- `InputState`, hit testing, pointer/wheel/key/touch entry points, capture, hover, focus, windows and activation, Tab navigation, long click.
- `invoke_event` with bubbling, listeners (global, per widget, closures), value group, messages, `set_time`/`update`/`next_wake_time`.
- **Tests** with small stub widgets: self-removal inside `on_event`; removing a sibling during a walk; busy re-entry returns `None`; stale IDs after flush; focus moves on removal; `die` with `KeepAlive`; bubbling through `event_destination`; message delivery order and cancellation on removal; value binding with no read-back from the sender.

### M2. Layout and inflation

- `layout/` (size types, measure with cache, default arrange, `stack.rs`, `dim.rs`), `set_rect`, `invalidate_layout`, the process pass.
- `reader.rs`: registry, generic properties, `on_inflate`, `inflate_z`, `autofocus`. A `Layout` widget and a plain `Container` to exercise it.
- **Tests:** `test_tb_dimension`; stack layout cases for each setting (distribution, gravity, size, position, overflow, reverse); measure cache hits and invalidation up the chain; inflate the demo layout files with stub factories and check rects.

### M3. Rendering, skin and fonts

- `render/` (trait, `Painter`, `Atlas` + shelf allocator), `skin/` (load, DPI variants, conditions, paint, strong overrides), `font/` (manager, TBBF, glyph cache), the paint walk and `PaintCtx`, `renderer_lost`.
- The hand-rolled `PngDecoder` (section 18.3 gives its scope). Tests: every PNG in TB's resources decodes to the same pixels as the `png` crate (a dev-dependency used only as the test oracle); truncated and corrupt inputs return errors and never panic; a `cargo fuzz` target for `inflate` and `decode` (a dev tool, not a dependency).
- `turbo-ferret-soft` and the golden-image harness (`tests/golden/`, with a `BLESS=1` env var to rewrite).
- **Tests:** `test_tb_space_allocator`; the default skin loads with every bitmap; golden images for nine-slice, tile, overrides, children and overlays, opacity, clip, and text in the default font; context-loss round trip (lose, repaint, same image).

### M4. Basic widgets, animation and the demo (first usable release)

- Widgets: `TextField`, `Button` (toggle mode, auto-repeat by messages), `ClickLabel`, `SkinImage`, `Separator`, `ProgressSpinner`, `CheckBox`, `RadioButton`, `ScrollBar`, `Slider`, `Container`, `Layout`, `Mover`, `Resizer`, `Dimmer`, `Window`.
- `anim.rs` with the five curves, rect and opacity animations, window fade in and out through `on_added`/`on_die`.
- `turbo-ferret-miniquad` (renderer + input adapter) and `demo/` showing TB's demo01 main window and its basic-widgets layouts.
- **Tests:** golden images per widget in its main states; click, toggle and radio-group behaviour; slider and scrollbar values with `connection:`; window close fades and then frees.
- **Done when** the demo runs on desktop with the default skin and font, and the core still builds for the `no_std` target.

### Later milestones

| Milestone | Content |
|---|---|
| M5. Scrolling and containers | Kinetic `Scroller` (driven by `update`), panning, `ScrollContainer`, `TabContainer`, `ToggleContainer`, `Section`/`SectionHeader`, overflow-scroll fade-out in `Layout` |
| M6. Lists and popups | `SelectItemSource` with viewer IDs, `SelectList`, `SelectDropdown`, `InlineSelect`, `PopupWindow` (listener closes on outside focus or click), `MenuWindow`, `MessageWindow` |
| M7. Text editing | `StyleEdit` (blocks and fragments as `Vec`s), `EditField`, caret blink by messages, undo/redo, `Clipboard` service with an in-memory default; port `test_tb_style_edit` |
| M8. Extras | TrueType backend (feature `truetype`), font blur effect, image manager and `ImageWidget` (refcounted fragments), `debug` overlay, optional `TbId → WidgetId` index |

---

## 21. Implementation conventions

For Claude Code (or anyone) implementing this:

- **Edition 2024, MSRV 1.86** (trait upcasting to `Any`).
- Core and soft renderer: `#![no_std]`, `extern crate alloc`, `#![forbid(unsafe_code)]`. The miniquad crate may use `unsafe` only where miniquad's API requires it.
- **Cite the source.** When porting a TB function, put a one-line comment with the TB file and function (`// TB: tb_layout.cpp ValidateLayout`) and, where it applies, the hazard ID (`// H-12`). This keeps the port reviewable against the reference commit.
- **Behavioural tests first** for ported subsystems: port TB's test where one exists, then implement.
- **Public API is `Ui` + `Ctx` + the `Widget` trait + the widget structs.** `Tree`, `Node` internals, `Painter` internals and `pending` stay `pub(crate)`.
- **No new dependencies** without a line in [decisions-and-status.md](decisions-and-status.md) saying why.
- Keep built-in widgets free of privileged access: anything a built-in widget needs from `Ui` should be available to app widgets too. That is how the trait stays honest.
- **Credit the original.** Keep the README sentence, the `NOTICE` file and the `// TB:` comments from section 0 and this section in place. They are licence obligations, not style.
- Prefer TB's names translated to Rust style (`TBSelectDropdown` → `SelectDropdown`, `GetPreferredSize` → `preferred_size`), and drop the `TB` prefix.

---

## 22. Deliberate differences from TB

Since compatibility is not required (decision 8), these changes are made because they simplify the port. Everything not listed should behave as TB does.

| Area | TB | Port | Why |
|---|---|---|---|
| Time | `TBSystem::GetTimeMS`, `RescheduleTimer` | `ui.set_time`, `ui.next_wake_time` | No service, deterministic tests |
| Renderer | Stateful translate, opacity, clip; two draw calls; power-of-two bitmaps; listener list for context loss | Stateless quads, one draw call, any size, premultiplied RGBA8; `renderer_lost()` | Narrower backend, no unbalanced state |
| Batching | `TBRendererBatcher` base class for backends | In the core | Every backend gets it |
| Long click, kinetic scroll | Private message handlers per widget | Driven by `update()` | Removes two mixins |
| Messages | Any `TBMessageHandler` | Widgets only | One target type |
| Event subclasses | `TBWidgetEventFileDrop` | `payload: Option<Rc<dyn Any>>` | No downcast hierarchy |
| Die animations | Global listener type-checks windows and dimmers | `on_die` returns `KeepAlive` | Widget decides for itself |
| Delete notification | `OnWidgetDelete` from the destructor | None; IDs fail safely | Not needed |
| Layout hook | `OnResized` + `OnProcess` + `InvalidateLayout` override | `arrange(old, new)` + `NEEDS_ARRANGE` | One hook, one flag |
| Paint children | Overridable `OnPaintChildren` | Framework walk + `clips_children` + `child_translation` + `paint_after_children` | Keeps the walk balanced |
| Skin recursion guard | Mutable flags | Depth limit | Reads stay reads |
| Factory lookup | Linear string compare | `TbId` map | Simpler and faster |
| Font description | Unused bold and italic bits | `id + size` | Unused |
| Children added during a walk | Visited if appended after the cursor | Not visited in that pass | Snapshot walks |
