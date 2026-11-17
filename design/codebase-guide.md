# Codebase guide

For whoever works on this code next, human or agent: how it is put together, how to
do the common jobs, and where it bites. `port-design.md` is the design and
`decisions-and-status.md` the record of decisions; this file is the working
knowledge that neither holds. Keep it current when you change what it describes.

## 1. The shape of it

```
host (demo, a game)
  │  input:  ui.pointer_down/move/up, wheel, key, touch_*, shortcut
  │  time:   ui.set_time(ms)
  │  frame:  ui.update();  if ui.take_repaint() { ui.paint(&mut renderer) }
  ▼
Ui ── Tree (arena of nodes) ── behaviours (Box<dyn Widget>, one per node)
  │      side tables: layout params, scrollers, connections, listeners, busy
  ├─ input state (hover, capture, focus), message queue, animations, values
  ├─ Skin (elements + atlas), FontManager (faces + glyph cache), ImageManager
  └─ Services: resource loader, image decoder, clipboard, log
        ▼
Painter (batches quads) ──► Renderer trait (soft, miniquad, yours)
```

One `update()` runs, in order: animations, kinetic scrollers, the long-click
timer, due messages (one at a time), widget states, skin updates, then one walk of
the tree that arranges nodes marked for it and calls `on_process`.

## 2. Where things are (core crate, `crates/turbo-ferret/src`)

| Module | Holds |
|---|---|
| `arena.rs` | Generational `Arena`, `WidgetId`, `SideTable` |
| `ui/mod.rs` | `Ui`, `Config`, node property getters and setters, `set_rect`, `is_busy`, `when_free` |
| `ui/tree.rs`, `node.rs` | `Tree`, `Node`, `NodeFlags`, `BusyState` and the busy-aware lookups |
| `ui/ctx.rs` | `Ctx` (what hooks get), `call_hook` (take-out and put-back), `after_hook` |
| `ui/ops.rs` | create, add, attach, remove, die, flush, `invalidate_layout`, `preferred_size` |
| `ui/dispatch.rs` | `invoke_event`: listeners, value binding, bubbling |
| `ui/input.rs` | pointer, wheel, key, touch entry points; hover, capture, panning |
| `ui/focus.rs` | focus, Tab order, scroll helpers |
| `ui/process.rs` | `update`, `next_wake_time`, message delivery, the process walk |
| `ui/scroller.rs` | kinetic and smooth scrolling, snap |
| `ui/listen.rs`, `value_group.rs`, `glue.rs`, `text.rs` | listeners; bound values; skin glue; fonts, images, clipboard access |
| `widget.rs` | the `Widget` trait and small shared types (`Axis`, `Z`, `ScrollInfo` ...) |
| `event.rs`, `msg.rs`, `anim.rs`, `value.rs`, `id.rs`, `geom.rs`, `math.rs` | events; messages; animations; `Value`; `TbId` (FNV-1a hash of a name); saturating geometry; float math without `std` |
| `res/` | the `.tb.txt` parser, `ResNode`, ref trees, `Lookup` |
| `reader.rs` | inflation: factories, generic properties, `on_inflate` |
| `layout/` | size types, the stack layout (`stack.rs`), dimension conversion (`dim.rs`) |
| `skin/` | skin loading, conditions, 9-slice painting |
| `render/` | `Renderer` trait, `Painter`, `Atlas` |
| `paint.rs` | the paint walk and `PaintCtx` |
| `font/` | `FontManager`, glyph cache, bitmap fonts (`tbbf.rs`), blur (`effect.rs`), the built-in font (`default_font.rs`, `default/`) |
| `image/` | PNG decoder (`png.rs`, `inflate.rs`) |
| `images.rs` | `ImageManager`, `ImageHandle` |
| `edit/` | `StyleEdit`: the text model behind `EditField` |
| `widgets/` | every built-in widget; `mod.rs` registers them |
| `debug.rs` | feature `debug`: overlays, the debug window, id names |

## 3. The model, in the detail that matters

### Nodes and behaviours
A widget is a `Node` (rect, id, state, flags, children, skin, font ...) in the
arena plus a behaviour, `Box<dyn Widget>`. Node data is read through
`ui.node(id)` (`NodeRef`) and written through `Ui` methods, which do the side
effects (invalidate, arrange, skin update). Behaviour state is reached with
`ui.get::<T>(id)` (read), `ui.with_widget::<T, _>(id, |w, cx| ..)` (call methods
that need the `Ui`), `ui.with_behaviour(id, ..)` (any type).

### Hooks and "busy"
`call_hook(ui, id, f)` takes the behaviour out of the node, runs `f(&mut behaviour,
&mut Ctx)`, puts it back. While it is out the widget is **busy**:
- `get`, `with_widget`, `with_behaviour` and every hook on it give `None` or are
  skipped (a debug-level log line says so);
- queries about it answer from caches: `Tree::busy` (filled at hook entry, or by
  `cx.publish(self)` in the middle of a hook) and node fields refreshed at
  put-back (`extra_pad`, `axis`, `align`);
- `set_rect` on it defers the `arrange`; `resize_to_preferred` and
  `resize_to_fit_content` on it are deferred whole; measuring it gives its last
  size. All of that is replayed in `after_hook` when the hook returns.

`Ctx` derefs to `Ui`, so a hook can call anything; `cx.this()` is its own id.

### Events
`cx.emit(ev)` / `ui.invoke_event(ev)`: global listeners first (they can swallow),
then value binding for `Changed`, then `on_event` from the target up along
`event_destination` (the parent, unless a widget redirects: popups send to their
target). Return `true` to stop.
- `Changed` always carries the value (`cx.emit_changed(v)`).
- List and menu `Click`s carry the item index in `ev.value`, its id in `ev.ref_id`.
- A global listener is taken out while it is called: it does not hear events
  caused inside its own callback.

### Layout
`invalidate_layout(id)` clears measure caches up the parents (stopping at a
layout boundary) and marks them `NEEDS_ARRANGE`; the next `update` arranges.
`preferred_size(id, constraints)` caches one result per node. `arrange(old, new)`
runs at once when `set_rect` changes a size. The default `arrange` moves children
by gravity from the size delta; `Layout` (`layout/stack.rs`) is TB's `TBLayout`.

### Removal
`ui.remove(id)`: notifications (`on_removing` listeners, `on_removed` on the
widget and everything below it, `on_child_removed` on the parent), then detach,
then forget it everywhere (input, messages, animations, values). The nodes are
freed at `flush` (top level only), where each behaviour gets `on_free(&mut Ui)`.
`ui.die(id)` is the graceful form (windows fade out).

### Painting
`ui.paint(renderer)` uploads atlas pages, then walks the tree read-only. A widget
draws in local coordinates through `PaintCtx` (`draw_skin`, `draw_text`,
`fill_rect`, `draw_image`, `with_clip` ...). The painter batches quads by bitmap
and clip.

## 4. Recipes

### Add a widget
1. `widgets/your_widget.rs`: a struct, `impl Widget`. Give `type_name`. Build
   internal children in `on_create` (set their skins with `set_skin_quiet`, ids,
   `set_ignore_input` as needed) and keep their ids in fields.
2. Read resource properties in `on_inflate(cx, res)` with `res.get/str/int`
   (references and `@key` strings are already resolved for you).
3. Size: `measure_content`; children: `arrange`; drawing: `paint`.
4. Value and text: implement `value`/`set_value`, `text`/`set_text`; announce
   changes with `cx.emit_changed(value)`.
5. Register it in `widgets/mod.rs` with `register_both(ui, "TBName", "Name",
   Factory::of::<T>("Name"))` (add `.sync(ValueKind::..)` if it has a value) and
   re-export the type.
6. A widget that wraps a `Window` uses `forward_to_window!(field)` (see
   `message_window.rs`); one that registers a global listener or adds helper
   widgets elsewhere releases them in both `on_removed` and `on_free`.
7. Tests: behaviour in `crates/turbo-ferret/tests/`, a golden image in
   `crates/turbo-ferret-soft/tests/`.

### Add a property all widgets understand
`reader.rs`, `apply_generic`. Read it through the `Lookup` (`lk.value`, `lk.str`).

### Test a behaviour
```rust
let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
ui.services_mut().log = None;
ui.set_viewport(Size::new(800, 600));
ui.set_time(1000);
let root = ui.root();
let ids = ui.inflate(root, "TBButton: id: ok, text: OK\n").unwrap();
ui.set_rect(ids[0], Rect::new(10, 10, 80, 30));
ui.update();
ui.pointer_down(Point::new(20, 20), 1, Modifiers::NONE);
ui.pointer_up(Point::new(20, 20), Modifiers::NONE);
```
- With the real skin: `Services::with_loader(FsLoader::new(".../resources"))`,
  `ui.load_skin("default_skin/skin.tb.txt", None)`. Without a skin, widgets have
  no size of their own.
- A new `Ui` uses the built-in font. Tests that count pixels of text set a box
  font on the root instead: `ui.set_font(root, Some(FontDesc::new(TbId::NONE,
  27)))` gives 10 pixel wide characters, 27 high.
- Windows fade in: let time pass (`ui.set_time`, `ui.update`) or wrap the call in
  `ui.block_animations(|ui| ..)`.
- To record events, add a widget whose `on_event` logs, or a `UiListener`.

### Golden images
`crates/turbo-ferret-soft/tests/common/mod.rs`: `assert_golden("name", &renderer)`.
`BLESS=1` rewrites `tests/golden/name.png`; a mismatch writes `name.actual.png`
next to it. Always open the image you bless. One `SoftRenderer` per `Ui` (it
holds the uploaded bitmaps).

### Check something in the demo
`demo/tests/screenshot.rs` builds the real `Demo`, steps time with
`demo.frame(t)`, clicks by widget id, and writes `target/demo_*.png`. Add a flow
there, run `cargo test -p turbo-ferret-demo`, and look at the PNG. This is the
only way an agent can see the demo; nobody can click the live window for you.

### Change the built-in font or the font tool
Regenerate with the command in `CLAUDE.md`; the test
`the_font_in_resources_is_up_to_date` (tools/fontgen) fails if the files and the
tool disagree.

## 5. Traps (each of these was a real bug)

- **Reading the sender.** A handler for a dropdown's `Changed` that asks the
  dropdown for its value gets nothing: it is busy. Use `ev.value`.
- **Asking a window from its own event.** `ui.get::<MessageWindow>(target)` in a
  listener that hears the window's click fails. Use node data or the busy-aware
  lookups: `ui.event_destination(id)`, `ui.popup_target(id)`.
- **Cleanup only in `on_removed`.** Skipped when the widget is removed from its
  own hook. Do it in `on_free` too.
- **Timers that re-post themselves.** A message handler that always posts the
  next tick keeps a stopped timer alive. Check the state first (see
  `ProgressSpinner::on_message`).
- **Measuring before the first `update`.** Anything a widget fixes up in
  `on_process` is not done yet when a window is sized to its content right after
  inflation. Do it in `measure_content` as well (see `Button`).
- **A new `SoftRenderer` per screenshot** shows a blank frame: bitmaps are
  uploaded once per `Ui`.
- **Byte offsets in text.** `StyleEdit` positions are byte offsets on `char`
  boundaries. A position just behind a block's line break means the start of the
  next block. `review_edit_fuzz.rs` checks every operation against a plain
  `String`; run it after touching `edit/`.
- **Numbers from resources and hosts are arbitrary.** Geometry sums saturate
  (`Point`, `Rect`); dimensions are clamped (`MAX_PX`). Use `saturating_*` when
  adding sizes or times.
- **Platform defaults in tests.** The miniquad adapter's defaults depend on the
  platform (`SHORTCUT_MODIFIER`, `KeyQuirks`). Tests set what they need.
- **Generic functions that recurse into `call_hook`.** `call_hook` is generic
  over its closure; calling it from inside itself with a new closure does not
  compile. Post-hook work lives in the non-generic `after_hook`.
- **Feature unification.** The demo turns on the core's `debug` feature, so
  `cargo test --workspace` never tests the core without it. Run the core alone
  too (the commands in `CLAUDE.md`).

## 6. Platforms: what has been run where

Everything so far was built and tested on **macOS (Apple Silicon)**. The
windowed demo ran there through its scripted `--frames` mode; nobody has yet
clicked or typed in the live window.

**Linux, Windows and the web have not been run.** What exists for them is read
from miniquad 0.4.11's sources, not observed:
- the input adapter's `KeyQuirks` and wheel units per platform;
- the clipboard: miniquad's own on X11 and Windows, `pbcopy`/`pbpaste` on macOS;
- CI (`.github/workflows/ci.yml`, Ubuntu) has never run.

First run on Linux, in this order:
```sh
sudo apt-get install -y libx11-dev libxi-dev libgl1-mesa-dev libasound2-dev   # miniquad's needs
cargo test --workspace            # everything headless; the macOS-only tests skip themselves
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p turbo-ferret-demo    # then by hand:
```
Things to try by hand, because no test can: typing (also AltGr and dead keys),
Ctrl+C / Ctrl+V with another application, Alt+letter not typing the letter,
wheel direction and speed, window resize, HiDPI scale, the right mouse button
(context menu in the text window), holding shift while clicking (touch
emulation). Record what you find in `decisions-and-status.md`.

## 7. Names: Turbo Badger to Turbo Ferret

| TB | Here |
|---|---|
| `TBWidget` subclass | a struct implementing `Widget`, in a node |
| `TBWidget *` | `WidgetId` |
| `TBID`, `TBIDC("x")` | `TbId`, `tbid!("x")` |
| `OnEvent`, `OnPaint`, `OnProcess` | `on_event`, `paint`, `on_process` |
| `OnCalculatePreferredContentSize` | `measure_content` |
| `OnResized` | `arrange(old, new)` |
| destructor | `on_free` |
| `TBWidgetListener` (global) | `UiListener` + `ui.add_listener` |
| `TBMessageHandler::PostMessage` | `ui.post_message` / `post_message_delayed` |
| `TBWidgetValue` | bound values: `create_bound_value`, `connection` in resources |
| `g_tb_skin`, `g_font_manager`, `g_image_manager` | `ui.skin_data()`, `ui.fonts()`, `ui.images()` |
| `TBRenderer` | `Renderer` (three calls for bitmaps, one `draw`) |
| `TBStyleEdit` | `edit::StyleEdit` (reports `Effects` instead of calling a listener) |
| `TBBF` font | `font::tbbf` (also with a glyph table) |
| `TB_RUNTIME_DEBUG_INFO` | feature `debug` |
