# Turbo Ferret (Rust port of Turbo Badger): Decisions and Status

*Hand-off notes so that work can resume without re-asking. Last updated 2026-11-17 (design accepted, named Turbo Ferret). Project memory is off, so this file is the record.*

## Where things stand

| Step | Status | Output |
|---|---|---|
| 1. Break down the C++ architecture | Done | [turbobadger-architecture.md](turbobadger-architecture.md): subsystems plus hazard catalogue `H-01` to `H-22` |
| 2. Assess ECS-style widget storage | Done, agreed | [ecs-widget-storage-assessment.md](ecs-widget-storage-assessment.md): verdict, option C, `no_std` costs, questions `M-01` to `M-08` |
| 3. Mutability design | Done, agreed (five decisions below) | This file, section "Mutability" |
| 4. Full port design | Done, accepted by Steve 2026-11-17 | [port-design.md](port-design.md): crates, core types, traits, event/layout/paint flow, milestones M0 to M8 |
| 5. Implementation with Claude Code | **Next.** Starts at M0 in port-design.md section 20 | |

**Original project:** Turbo Badger by Emil Segerås, zlib licence (copyright 2011 to 2014, see `src/tb/tb_core.h`). The port must credit it and ship its notice: see port-design.md section 0.

**Source reference:** `https://github.com/fruxo/turbobadger`, `master` at commit `389baa6` (24 July 2016, the latest commit). Re-clone with `git clone https://github.com/fruxo/turbobadger && git -C turbobadger checkout 389baa6`.

## Decisions so far (agreed with Steve)

1. **Idiomatic Rust that keeps TB's feel:** a small, lightweight game-engine UI that the host drives (the host owns the loop, feeds input and calls process and paint).
2. **Widget storage is a small, custom ECS/arena**, not a framework crate (no bevy_ecs or hecs). This means:
   - a generational arena of widget nodes addressed by `WidgetId` (a stale ID fails lookup safely);
   - ordered `children: Vec<WidgetId>`, which replaces the intrusive lists;
   - shared widget state (rect, state flags, skin, font, layout cache) in the node, with per-type behaviour as a `Box<dyn Widget>` (trait with default hooks);
   - optional per-widget data (layout params, scroller, long-click timer, value connection, listeners) in sparse "component" tables keyed by `WidgetId`;
   - one explicit `Ui` context in place of TB's globals and singletons.
3. **The ECS owns the widgets.** All ownership goes through the arena, including former by-value member children (window mover, button text, scrollbars), which become ordinary child nodes referenced by ID.
4. **Minimal dependencies.** Hand-roll small things (the arena, the sparse map, and possibly a `u32`-keyed map). Any dependency has to justify itself.
5. **Keep the narrow renderer interface.** That means textured and colored quads, translate, clip and opacity, plus bitmap creation and context loss. The renderer is passed in at paint time, never stored globally.
6. **A `no_std` core with std services behind traits** (Steve, 2026-11-17). `no_std` is a nice-to-have that makes embedded use easier. The core crate is `#![no_std]` + `alloc`. Anything that needs std is a *service trait* with a default std implementation behind a `std` feature, and hosts without std swap in their own implementations. See "Service traits" below.
7. **Keep `TbId` (FNV-1a name hash) separate from `WidgetId` (storage handle).**
8. **No compatibility requirement with TB** (Steve, 2026-11-17). This covers IDs, names, the resource format, skins and fonts. Keep what TB does when there is no reason to change it, but don't add complexity just to stay compatible.
9. **Renderer backends: software first, then miniquad** (Steve, 2026-11-17). A zero-dependency `no_std` software renderer (`turbobadger-soft`) ships at M3 for headless golden-image tests and embedded use, then a `miniquad` GL backend at M4 for the demo. See port-design.md 18.1.
10. **First-milestone services: the minimal set, with time as data** (Steve, 2026-11-17). Resource loader (`MemLoader` + `FsLoader`), image decoder, TBBF fonts and a log sink. Clock and timer wake-up are not services: the host calls `ui.set_time(now)` and reads `ui.next_wake_time()`. Clipboard comes with the edit field (M7), TrueType at M8. See port-design.md 17 and 18.2.
11. **PNG decoding is hand-rolled** (Steve, 2026-11-17), in the core behind the `png` feature, so the core has zero dependencies. Scope: DEFLATE plus PNG unfiltering for non-interlaced 8-bit images (and low-bit palette and grayscale), which covers all of TB's assets. Interlaced and 16-bit images return an error. It is tested against the `png` crate as a dev-only oracle and fuzzed. See port-design.md 18.3.
12. **The port is called Turbo Ferret** (Steve, 2026-11-17), to set it apart from Turbo Badger. Crates: `turbo-ferret` (core, imported as `turbo_ferret`), `turbo-ferret-soft` and `turbo-ferret-miniquad`. The docs and every crate credit Turbo Badger and Emil Segerås and carry its zlib notice.
13. **The rest of port-design.md is accepted** (Steve, 2026-11-17). That includes: one core crate plus one crate per backend; a six-method renderer trait taking quads, with translate, opacity and batching in the core and premultiplied RGBA8 bitmaps; `arrange(old, new)` in place of `OnResized`; `BTreeMap` and hand-written float helpers in place of hash or math crates; a depth limit in place of the skin's recursion flags; and M0 to M4 as the first release.
14. **The Segoe-derived default font is for the demo and tests only.** Its own licence note warns it is rasterized from a copyrighted font, so it never ships inside a published crate.
15. **Turbo Ferret is licensed under zlib** (Steve, 2026-11-17), like Turbo Badger. `license = "Zlib"` in every crate, a `LICENSE` file with Turbo Ferret's notice and a `NOTICE` file with Turbo Badger's, both shipped in every crate. See port-design.md section 0.

## Still open

Nothing. The design is complete and accepted; implementation starts at M0.

---

## Mutability (decided; the design is in port-design.md sections 7 and 8)

Steve's point is that if the ECS owns the widgets, mutability can be handled *through* it, but there will still be issues. That is right: arena ownership makes every stale reference safe, so what is left is **borrowing and ordering policy**, not memory safety. These are thinking notes for when we pick this back up. They map onto `M-01` to `M-08` in the assessment doc.

### The core conflict (M-01)

A hook runs on node X. To call it, the `Ui` has to borrow X's `behaviour` mutably out of `ui.nodes`, but the hook itself wants `&mut Ui` to read other nodes, change focus, add or remove widgets, post messages and start animations. Rust won't lend both at once.

**Leading idea: take the behaviour out, call it, put it back.**

```rust
// sketch only
fn call_hook<R>(ui: &mut Ui, id: WidgetId, f: impl FnOnce(&mut dyn Widget, &mut Ctx) -> R) -> Option<R> {
    let mut b = ui.nodes.get_mut(id)?.behaviour.take()?;  // None = node gone, or busy (re-entrant)
    let r = f(&mut *b, &mut Ctx { ui, this: id });
    if let Some(node) = ui.nodes.get_mut(id) { node.behaviour = Some(b); } // else: node was removed; drop b
    Some(r)
}
```

Why it fits:

- While X's hook runs, **all of `Ui` is available**, including X's own shared node data (rect, state, layout cache), because only the behaviour box was taken.
- **Nested calls on *other* nodes work naturally**, for example a parent measuring its children (`M-04`) or bubbling up to an ancestor.
- **Self-removal is safe.** If X removes itself, its slot is freed or detached, the put-back sees that the ID no longer resolves, and the behaviour is dropped. This is TB's `Die()` / `delete this` without the danger.
- **Re-entry into the same node becomes visible as "busy"** (`take()` returns `None`) instead of aliasing, or panicking as a `RefCell` would. The policy for that case is still open: skip it, queue it, or treat it as an error.

Known costs:

- Inside its own hook, a widget cannot look *itself* up by type through `ui.get::<T>(self_id)`. It must use `self`. This is easy to document.
- Every hook call pays an `Option` take and restore. That is trivial.
- Value binding (`M-08`) re-enters: A's `CHANGED` pushes the value to B and C, and B's update may read A while A is busy. A likely answer is for the `CHANGED` event to carry the new value, or for the sync to be deferred until A's hook returns.

Alternatives to keep in view: per-node `RefCell` (simpler, but re-entry panics at runtime); splitting `Ui` into `tree` and `services` and passing only `&mut services` plus a restricted tree view to hooks; or giving hooks *only* a command queue.

### Mutating the tree while walking it (M-02, M-03)

TB's semantics: structural changes are **immediate**, and the safe iterators skip removed nodes.

Early leaning, a hybrid approach:

- **Creation and attachment stay immediate.** TB code relies on this. For example, it inflates a resource and immediately calls `GetWidgetByID`, or creates a window and focuses a child in it.
- **Removal splits into "detach now" and "free later":**
  - *Detach now:* take the node out of its parent's `children`, fire `on_remove`, notify listeners, fix up focus, hover and capture, and abort animations. Tree queries are then correct right away.
  - *Free later:* release the arena slots at a safe point (the end of the current top-level dispatch). This matches `Die()`, which already keeps dying widgets around for animations, and it means put-back after self-removal never races with slot reuse.
- **Walks iterate over a snapshot of child IDs** (copied into a reusable scratch buffer per level, to avoid allocating), and skip any ID that no longer resolves or is no longer a child. This reproduces "removed children are skipped". One small difference: children added during a walk aren't visited in that pass, while TB's linked list would visit them if they were appended after the cursor. It is probably harmless, but worth checking against widgets such as the select list, which rebuild their items.
- **Event bubbling** is a loop that calls one hook at a time, so it never nests on the same node. After each call it re-reads `event_destination(cur)`. If the target vanished mid-event, the event counts as handled, as in TB.

### Callbacks and listeners (M-05)

- **Global listeners** (popups watching focus and clicks, the value group, debug tools) become `Vec<Box<dyn UiListener>>` on `Ui`. To call them, `mem::take` the vec, call each one with `&mut Ctx`, then merge it back, keeping any listeners registered during the call. This is the same take-out idea as for widgets.
- **Per-widget listeners** become a sparse table keyed by `WidgetId`, invoked the same way.
- **`TBWidgetSafePointer` disappears**, because a `WidgetId` is already a weak reference.
- **Animations** target a `WidgetId` and write to node data (opacity, rect), not to the behaviour, so they don't conflict with hooks. An update pass does `mem::take` on the animation list. A stale target means the animation is dropped. Completion callbacks (for example "die after fade") go through the deferred-free path.
- **Messages** come from a queue keyed by target ID. Processing pops one message and calls the target's `on_message` through `call_hook`. The handler can post more messages freely, because the queue isn't borrowed during the call.

### Deferred commands

There should be a small command queue on `Ui`, flushed at the end of each top-level entry point (`invoke_pointer_*`, `invoke_key`, `process`, `process_messages`, the animation update):

- **Use it for:** freeing removed subtrees, actions requested during **paint** (which should be read-only for the tree), and possibly z-order changes and focus changes requested mid-walk.
- **Avoid it for** anything TB code expects to see take effect immediately, such as creation, attachment, property sets and `set_focus` outside of paint. Deferring those would change behaviour in ways that are hard to spot.
- **Open question:** should commands be public API (hooks can choose deferred or immediate), or an internal detail?

### Paint (M-06, M-07)

- `paint(&mut self, renderer: &mut impl Renderer)` splits `Ui` into disjoint borrows: nodes read-only, `&mut FontManager` (the glyph cache fills lazily and creates bitmaps through the renderer), `&Skin`, and `&mut renderer`.
- The paint hook could take `&self` behaviour plus a read-only `PaintCtx`, so paint never needs take-out.
- The paint walk passes translation, opacity and clip as parameters, so the renderer stays narrow and state is never left unbalanced (H-17).
- The skin's `is_painting` / `is_getting` recursion guards can become a small visited-set on the stack or a `Cell`. That decision can wait.

### Layout (M-04)

- The layout cache lives in node data, not in the behaviour.
- A parent's measure hook calls `ctx.preferred_size(child, constraints)`, which checks the child's cache and otherwise calls the child's hook through `call_hook`. That is a nested call on a *different* node, so the take-out approach handles it.
- Invalidation walks `parent` IDs upward. That is plain data access, with no hooks involved, except `TBLayout`'s override, which could become a flag on the node.

### Questions to put to Steve when we resume

1. Is the take-out / put-back hook model acceptable as the default, with "busy" re-entry handled by a stated policy?
   - **Steve (2026-11-17):** Take-out/put-back will be needed for some operations. He asked whether callbacks should always receive the widget already taken out, or only receive the widget's ID and take it themselves when needed.
   - **Decision:** use both, split by the kind of callback.
     - *The widget's own trait hooks* (`on_event`, `on_process`, `on_message`, measure, paint and so on) always get `&mut self` from the framework, plus `ctx`, with `ctx.this()` giving the ID. These hooks exist to work on the widget's own data, and the take is just a pointer swap.
     - *External callbacks* (global and per-widget listeners, app event handlers, animations, value-group listeners) get a `WidgetId` plus `&mut Ctx`. They read and write node data directly (rect, opacity, state, id, type), with no take needed, and they reach the behaviour only through a scoped `ctx.with_widget::<T>(id, |w, ctx| ..)` that does the take and put-back internally.
     - Callbacks like that exist in TB: popups only check ancestry, the animation manager checks the widget type and sets opacity or rect, the safe pointer disappears, and focus and add/remove listeners mostly read IDs and node state.
     - **Accepted by Steve (2026-11-17).**
     - "Busy" policy (accepted): a call that tries to take a widget that is already out returns `None` (not handled) and logs a warning in debug builds.
2. Should removal be "detach immediately, free at a safe point"?
   - **Steve (2026-11-17): tentatively yes.** It "seems reasonable at this point". Working assumption: removing a widget takes it out of the tree right away (parent's children, focus, hover and capture fixed up, `on_remove` and listeners fired, and public lookups of its ID fail from then on), and the arena slot is freed at the end of the current top-level call. Revisit if it causes trouble.
3. Should paint be read-only for the tree, with any changes requested during paint deferred?
   - **Steve (2026-11-17): try read-only first**, but don't overcomplicate. Plan: the paint hook gets `&self` and a read-only view of the tree. Allowed exceptions are requesting a repaint (a flag) and the font glyph cache filling lazily (`&mut FontManager` is passed separately). If real widgets need more than that during paint, relax the rule rather than building machinery around it.
4. Should the command queue be public or internal?
   - **Steve (2026-11-17):** Why would it be public? A proper API is better.
   - **Decision: internal.** The only reason to expose it was to let callers choose "do this later" over "do this now". Normal `ctx` API methods (`remove`, `die`, `set_focus`, `add_child` and so on) cover that: the framework decides internally which ones apply immediately and which are queued (for example, anything requested during paint, and freeing removed slots).
5. Should `CHANGED` events carry their value, so that value binding needs no read-back from a busy widget?
   - **Steve (2026-11-17): yes, unless values are huge.**
   - **Decision (accepted by Steve, 2026-11-17):** in practice only text is ever large (a multi-line edit field bound with `connection:`); the others are ints, floats and short strings. The event carries `value: Option<TbValue>`, and the widget fills it in only when it has a value connection. Strings are shared (`Rc<str>`), so passing the event along costs a refcount bump, not a copy. This is no worse than TB, which already copies the whole text on every change of a connected field.

## Service traits (no_std core, std defaults)

These were the first thoughts on what sits behind traits. The refined proposal is in port-design.md section 17 (Clock and timer wake-up become plain data there). Each trait has a std default implementation behind the `std` feature, and any of them can be swapped out.

| Service | TB origin | Std default | Without std |
|---|---|---|---|
| **Clock** (`now_ms`) | `TBSystem::GetTimeMS` | `std::time::Instant` | Host provides a tick source |
| **Timer wake-up** (when to call `process_messages` next) | `TBSystem::RescheduleTimer` | No-op; the host polls a "next fire time" query | Same |
| **Resource loader** (bytes for a path, relative paths for `@file`) | `TBFile` | `std::fs` | Embedded assets (`include_bytes!` tables), flash and so on |
| **Image decoder** (bytes → RGBA) | `TBImageLoader` | Hand-rolled PNG decoder behind the `png` feature (decision 11) | Host decoder, or images pre-converted at build time |
| **Font rasteriser** (one per font type) | `TBFontRenderer` | TB's bitmap-font format, built in and already `no_std`; TrueType through an optional `no_std` crate (`fontdue` or `ttf-parser`) | Same, since both work without std |
| **Clipboard** | `TBClipboard` | In-memory clipboard (needs no std); OS clipboards come from the host or platform crates | In-memory |
| **Log / debug output** | `TBDebugOut` | stderr | Host sink, or nothing |
| **Platform metrics** (DPI, long-click delay, pan threshold, pixels per line) | Other `TBSystem` methods | A plain config struct with TB's defaults | Same (no trait needed) |

These are **not services**, just internal choices: the hash map (`BTreeMap`, `hashbrown` or a small `u32`-keyed map), float math (`libm` or a few helpers), and string formatting (`alloc::format!`). The **renderer** is already a host-implemented trait passed in at paint time, so it has no std question.

## File index (`/mnt/project-files/architecture/`)

- `turbobadger-architecture.md`: the C++ architecture breakdown and Rust hazard catalogue (H-01 to H-22).
- `ecs-widget-storage-assessment.md`: the ECS versus arena assessment, verdict, `no_std` costs and mutability questions (M-01 to M-08).
- `port-design.md`: the full port design (crates, types, traits, flows, milestones). Implementation follows this.
- `decisions-and-status.md`: this file. Decisions, open items and mutability notes.
