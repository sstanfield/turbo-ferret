# Can widgets live in an ECS-style store?

*Part of the Turbo Ferret design, a Rust port of Turbo Badger by Emil Segerås (github.com/fruxo/turbobadger). Follow-up to [turbobadger-architecture.md](turbobadger-architecture.md). Hazard IDs (`H-nn`) refer to that doc's catalogue. Mutability is deliberately left open: the questions are listed in section 6 (tagged `M-nn`) for the next step.*

## Verdict

**Viable, and it removes most of the High-severity hazards.** The shape that fits TurboBadger best is not a general-purpose ECS framework. It is a **generational arena of widget nodes, addressed by `WidgetId`, with optional "components" kept in sparse side tables**. You could call it "ECS-lite". That gives Steve the core idea he asked for, `Vec`s and IDs instead of linked lists and pointers, without pulling in an ECS crate's dependencies or its query and system model, which doesn't match how UI code walks a tree.

The quick summary per option:

| Option | Fits TB's tree-ordered work | Dependencies | no_std + alloc | Recommendation |
|---|---|---|---|---|
| **A. Full ECS framework** (bevy_ecs, hecs) for all widget data | Poor: hierarchy, sibling order and bubbling are bolted on | bevy_ecs: about 20 direct deps. hecs: small (hashbrown, foldhash, spin) | hecs yes, bevy_ecs partially | No, not as the core |
| **B. Generational arena tree** (slotmap-style), node struct + `Box<dyn Widget>` behaviour | Good: direct parent and child IDs, ordered `Vec` children | Zero (hand-rolled, about 200 lines) or `slotmap` (zero deps itself) | Yes | Yes, as the base |
| **C. B plus sparse component tables** for optional per-widget data | Good | Same as B | Yes | **Yes. This is the recommendation** |

Option C is B with the ECS idea applied where it actually pays off. TB already treats several things as optional per-widget attachments that it allocates lazily: `LayoutParams`, `TBScroller`, `TBLongClickTimer`, value connections, local listeners, and running widget animations. These become sparse tables keyed by `WidgetId`, and the mixin base classes (H-08) dissolve into them.

---

## 1. What "ECS" would mean here

In an ECS, an entity is just an ID, its data is split across component storages, and logic runs as systems that iterate over every entity with a given set of components. That model shines when there are thousands of similar entities processed in bulk, in any order, possibly in parallel.

TB's work is the opposite shape:

- **Almost everything is tree-ordered and recursive.** Paint goes depth-first, back to front. Hit testing goes front to back. Events bubble from the target to its ancestors. Layout recurses parent to child and back. Focus navigation walks `next deep` / `prev deep`. Skin conditions look at parent, ancestors and siblings.
- **Sibling order is semantic** (it is the z-order), and changing it with `SetZ` / `AddChildRelative` is common.
- **Node counts are small**: tens to a few hundred widgets per screen, not tens of thousands.
- **Behaviour is per-type and dispatched per node** (about 35 hooks), not per component set.

So TB gains little from what an ECS is good at (bulk, order-free iteration) and keeps needing what an ECS does not supply out of the box (ordered hierarchy, per-node polymorphic callbacks). bevy_ecs does provide `ChildOf` / `Children` relationships, but that is a hierarchy layered on top, and its dependency footprint works against the "small engine, few deps" principle.

**One real argument for a full ECS:** if the target host engine is itself ECS-based (Bevy, for example), UI entities could live in the game's world. That is an integration concern. It can be served by an adapter crate on top of option C, so it isn't a reason to make the core depend on an ECS.

---

## 2. The shape being assessed (illustrative only, not the design)

This sketch exists only so the hazard checks below have something concrete to point at. Names and details are placeholders.

```rust
// Generational handle: index + generation. A stale id simply fails lookup.
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct WidgetId { index: u32, generation: u32 }

struct Node {
    parent: Option<WidgetId>,
    children: Vec<WidgetId>,          // ordered: index 0 = bottom of z-order
    rect: Rect,
    id: TbId,                          // TB's hashed name id (TBID), not the slot handle
    group_id: TbId,
    skin_bg: TbId,
    state: WidgetState,                // bitflags
    flags: WidgetFlags,                // the old m_packed bits
    opacity: f32,
    gravity: Gravity,
    font: FontDescription,
    cached_ps: Option<(SizeConstraints, PreferredSize)>,
    behaviour: Box<dyn Widget>,        // per-type hooks + type-specific data
}

struct Ui {
    nodes: Arena<Node>,                                // Vec of slots + free list
    layout_params: SparseMap<WidgetId, LayoutParams>,  // "components"
    scrollers:     SparseMap<WidgetId, Scroller>,
    long_click:    SparseMap<WidgetId, LongClickTimer>,
    connections:   SparseMap<WidgetId, ValueConnection>,
    input: InputState { hovered, captured, focused: Option<WidgetId>, touches, .. },
    skin: Skin, fonts: FontManager, lang: Language, animations: Animations,
    messages: MessageQueue, factories: FactoryRegistry, values: ValueGroup,
}
```

Two different IDs exist and should stay distinct:

- **`WidgetId`** is the storage handle (generational, never reused while it can be confused).
- **`TbId`** is TB's FNV-1a name hash (`id: "ok"` in resources), used for lookup by name, skins, language and messages.

`GetWidgetByID` remains a subtree search by `TbId`. An optional `TbId → WidgetId` index could make it fast later.

---

## 3. Hazard-by-hazard check

| Hazard | What the ID store does to it | Result |
|---|---|---|
| **H-01** Intrusive lists | Children become `Vec<WidgetId>`. Every other list becomes a plain `Vec` or a sparse map keyed by ID. Being "in two lists at once" is no longer special: a message is an entry in one global queue that names its target `WidgetId`, and "the handler's pending messages" is a filter over that queue. | **Removed** |
| **H-02** Safe iterators | There are no list nodes to unlink from under an iterator. Iterating over a snapshot of child IDs (a cheap copy of a small `Vec`) and skipping IDs that no longer resolve reproduces TB's "removal during iteration is fine" behaviour. | **Removed in mechanism.** The policy is mutability question M-03 |
| **H-03** Raw parent and back pointers | `parent: Option<WidgetId>`. Value connections, animations, messages and popups hold a `WidgetId`. Bitmap fragments become `FragmentId`s into their atlas. `TBImage` becomes `Rc` or a refcounted handle in a table. Every reference is a lookup that can fail safely. | **Removed** |
| **H-04** Static input state | `hovered`, `captured`, `focused`, the touch table and `cancel_click` become fields on `Ui`. The destructor scrubbing in `~TBWidget` is no longer needed for safety, because stale IDs fail lookup. It is still wanted for correct semantics (focus moves on removal), so removal code clears them explicitly. | **Removed** |
| **H-05** Singletons and registries | Skin, fonts, language, animations, message queue, factories and value group become fields of `Ui` (or a few sub-contexts). The renderer is passed into `paint(&mut self, renderer: &mut impl Renderer)` and never stored globally. `no_std` has no `thread_local!`, which pushes in the same direction anyway. | **Removed**, at the cost of borrow-splitting (M-01, M-06) |
| **H-06** Static-init factories | An explicit registry on `Ui`. Built-ins are registered in `Ui::new()`, and apps call `ui.register_widget::<MyWidget>("MyWidget")`. | **Removed** (orthogonal to the ECS question, but it falls out naturally) |
| **H-07** Fat virtual base, deep inheritance | Splits in two. **Shared state** (rect, state, skin, flags, font, layout cache) moves into `Node`, so it is no longer inherited. **Behaviour** becomes a `Widget` trait with default methods, held as `Box<dyn Widget>`. Inheritance depth flattens: `TBCheckBox → TBRadioCheckBox → TBWidget` becomes one trait impl, and shared code becomes helper functions or composition. "Must call base `OnInflate`" disappears because the framework does the generic part itself, before and after calling the hook. | **Mostly removed.** Dynamic dispatch remains, which is fine and small |
| **H-08** Multiple-inheritance mixins | `TBMessageHandler` becomes "messages target a `WidgetId`", plus an `on_message` hook. `TBScroller` and `TBLongClickTimer` become sparse components. `TBSelectItemViewer` becomes the source keeping a list of viewer IDs. `TBWidgetListener` on a widget becomes a sparse listener table. | **Removed.** This is where the ECS idea pays off |
| **H-09** Custom RTTI and downcasting | `core::any::Any` (available in `no_std`) through `as_any()` / `as_any_mut()` on the trait. This gives `ui.get::<Button>(id) -> Option<&Button>`, with an optional typed `WidgetId<T>` wrapper on top. | **Removed** |
| **H-10** Self-deletion, `TBWidgetSafePointer` | `die(id)` / `remove(id)` operate on the arena, never on `self`. Because every outside reference is a generational `WidgetId`, **every ID is already a safe weak pointer**. `TBWidgetSafePointer` and the "was I deleted?" checks after callbacks become a failed `get`. The dying protocol (keep in the tree, block input, let an animation finish, then remove) becomes a flag plus a deferred removal. | **Removed in mechanism.** When removal happens during dispatch is M-02 |
| **H-11** Children embedded by value | Internal parts (window mover, resizer, title, close button, button text and layout, scroll container root and scrollbars) become **ordinary child nodes** created in the widget's construction hook. The parent stores their IDs as plain fields, for example `struct Window { mover: WidgetId, close: WidgetId, .. }`. Ownership becomes uniform: deleting a subtree deletes everything in it, and destructor ordering no longer matters. `GetContentRoot()` returns a `WidgetId`. | **Removed** (a clear win) |
| **H-12** Re-entrancy during traversal | Memory safety is now guaranteed by IDs. What remains is a pure borrow and semantics question: a hook running on node X holds `&mut` to X's behaviour while it wants `&mut Ui` to change the tree. | **Becomes the core mutability question** (M-01 to M-03) |
| **H-13** `TBValue` ownership flags | Unrelated to storage. It becomes a Rust enum: `Null`, `Int`, `Float`, `String`, `Array`, `Object(Box<dyn Any>)`. | Unaffected |
| **H-17** Renderer state machine | Unrelated. Paint recursion can carry the translation, opacity and clip as parameters, so the narrow `Renderer` trait stays as narrow as `TBRenderer`. | Unaffected, still easy |
| **H-18** Mutation hidden in getters (layout cache) | The cache sits in `Node`. Index-based access makes "parent measures children" easy to express, but measuring a child calls that child's hook while the parent's hook is also running. | **Becomes M-04** |
| **H-19** Destructor callbacks | Removal becomes an explicit `remove_subtree(id)` that fires `on_remove`, listener notifications, animation aborts and focus fix-up in a defined order *before* freeing the slots. No `Drop` logic is needed. | **Removed** |
| **H-22** Context loss, flush before free | Unrelated. Atlases keep renderer bitmap handles, and context loss clears them, the same as in TB. | Unaffected |

**Non-widget trees and lists get the same treatment:**

- The parsed resource tree (`TBNode`) can be a plain owned tree (`Vec<Node>` children). Its only parent use is include-cycle detection, which a traversal stack handles.
- Skin elements and font faces go in maps keyed by `TbId`.
- Animations become a `Vec` (or arena) of `{ target: WidgetId, .. }`. A stale target means the animation is dropped quietly, which replaces TB's abort-on-delete listener.
- The message queue becomes a binary heap of `(fire_time, seq, target, message)`, plus an "immediate" `Vec`.

---

## 4. Full ECS vs arena tree, in more detail

| Concern | Full ECS framework | Arena tree + sparse components |
|---|---|---|
| Ordered children and z-order changes | Relationship components; ordering has to be maintained by hand | `Vec<WidgetId>`: insert at a position, remove, rotate |
| Tree walks (paint, hit test, bubble, focus order) | Repeated lookups through `Children` components | Direct index into the arena |
| Per-type behaviour (about 35 hooks) | Still needs a `Box<dyn Widget>` component, or many marker components plus systems that each re-walk the tree | `Box<dyn Widget>` in the node |
| Optional data (layout params, scroller, timers) | Natural fit (components) | Sparse maps: the same idea, hand-rolled |
| State toggles such as hovered and pressed | Archetype ECS (bevy, hecs) moves entities between tables when components are added or removed, so these would have to be flags anyway | Flags in the node |
| Generational IDs | Yes | Yes |
| Dependencies | bevy_ecs about 20 direct; hecs 3 | 0 (hand-rolled) or `slotmap` (no deps of its own) |
| Embedding in a game that already uses an ECS | Native if it is the same ECS | Through an adapter that mirrors or hosts the `Ui` |
| Fits "small, low-deps engine" | Weakly | Strongly |

**Hand-rolling versus the `slotmap` crate.** A generational arena is about 150 to 250 lines (a `Vec<Slot<T>>` with a free list and generation counters), and a sparse "secondary map" keyed by the same ID is similar. `slotmap` 1.1 has no required dependencies and works `no_std` with its `std` feature turned off, so it is a reasonable shortcut. Owning the code keeps the dependency count at zero, though. Either choice is cheap, so it is not a decision that needs to be made now.

**`children: Vec<WidgetId>` versus ID-linked siblings.** Steve asked for Vecs, and they are the better default. Child lists are short, iteration is the common operation, index access helps the layout engine, and `GetChildFromIndex` becomes O(1). The cost is O(n) insert and remove in the middle of a list, which doesn't matter at UI sizes. (Storing `prev`/`next` IDs in each node keeps O(1) re-ordering but reintroduces the bookkeeping TB had.)

---

## 5. What `no_std` + `alloc` costs

The ID-store design itself needs only `alloc` (`Vec`, `Box`, `String`, `Rc`, `BTreeMap`) and `core` (`Any`, `Cell`, `RefCell`). The costs come from elsewhere in TB:

| Need | In std | With only core + alloc | Cost |
|---|---|---|---|
| Hash maps (skin elements, fonts, glyph cache, language, touch info) | `HashMap` | Not in `alloc`. Options: `BTreeMap` (in `alloc`, fine at these sizes), `hashbrown` (small, `no_std`), or a tiny map keyed by `u32`, since TB keys are already FNV hashes | Low |
| Float math: TB uses `roundf`, `floor`, `ceil`, `sqrt`, `pow`, `expf` and `fabs` (dimension conversion, animation curves, font blur, scroller physics) | `f32::round()` and friends | Most transcendental and rounding methods are not available on `f32` in `core` on stable Rust. Use the `libm` crate (pure Rust, no deps) or a few hand-written helpers | Low (one tiny dep or about 50 lines) |
| Time (`TBSystem::GetTimeMS`) | `std::time::Instant` | Already a porting hook in TB. It becomes a `now_ms()` supplied by the host | None |
| File access (`TBFile`) | `std::fs` | Already a hook. Load resources from `&[u8]`, with a host-supplied loader trait for includes (`@file`) | None |
| Image decoding (skin PNGs) | `png` / `image` crates | The `png` crate reads through `std::io`, so it needs `std`. Options: decoding behind a host trait (as `TBImageLoader` already is), a `std`-only feature, or a small `no_std` PNG decoder | Medium (a design choice, not a blocker) |
| TrueType fonts | `fontdue`, `ttf-parser` | Both work `no_std` | Low |
| Errors | `std::error::Error` | `core::error::Error` is stable | None |
| Formatting and debug output (`TBDebugPrint`) | `println!` | `alloc::format!` plus a host log hook | None |
| Global state | `thread_local!` | Not available. Push toward the explicit `Ui` context, which section 3 recommends anyway | None (it helps) |
| Tests and demo | std | Tests and the demo can use `std` freely while the library stays `no_std` | None |

**Bottom line:** `no_std + alloc` for the core crate is realistic and cheap. The only real decision is image decoding: either a host hook, or an optional `std` feature. A common layout is `#![no_std]` with `extern crate alloc`, plus a `std` feature that turns on convenience such as file loading, PNG decoding and `HashMap`.

---

## 6. Mutability questions to settle next (flagged only, not solved)

| ID | Question | Where it comes from |
|---|---|---|
| **M-01** | When `ui` calls a hook on node X, it is holding `&mut` to X's `behaviour` out of `ui.nodes`. The hook needs `&mut Ui` to read other nodes, change focus, add or remove children, post messages and start animations. How do we lend both at once? (Candidates for the next step include taking the behaviour out of its slot for the duration of the call, splitting `Ui` into separately borrowable parts, or passing a context that queues commands.) | H-07, H-12 |
| **M-02** | Should structural changes made during dispatch (remove, die, add child, re-order) apply **immediately**, as in TB (code can rely on the tree having changed right after the call), or be **deferred** to a safe point? Which TB behaviours depend on immediacy? | H-10, H-12 |
| **M-03** | While walking children (paint, process, bubbling, listener fan-out), do we snapshot the ID lists or re-validate indices? What is the expected behaviour for children added during the walk? | H-02 |
| **M-04** | Layout measurement: a parent's `on_calculate_preferred_size` asks each child for its size, which may run the child's hook and write the child's cache. This is recursive `&mut` access to different nodes. | H-18 |
| **M-05** | Global listeners (popups, animations, the value group) react to events on other widgets and change them. How are they registered and called without aliasing the tree? | H-04, H-05, H-08 |
| **M-06** | During paint, widgets need `&mut Renderer` and `&mut FontManager` (glyphs are rendered lazily into the atlas, which creates bitmaps through the renderer) and `&Skin`, while the paint walk borrows the nodes. | H-05, H-17, H-22 |
| **M-07** | The skin's `is_painting` / `is_getting` recursion guards and other "mutating reads". Can these use `Cell`, or should they be restructured? | H-18 |
| **M-08** | Value binding: a change on widget A pushes to widgets B and C through their hooks, while A's hook is possibly still on the stack. | H-12, H-13 |

All eight are the same underlying problem, shared mutable access to a tree during callbacks, seen from different call sites. The ID store makes all of them memory-safe by construction, so the remaining work is choosing the borrowing and ordering policy.

---

## 7. Recommendation for the next step

Adopt **option C** (generational arena of nodes, ordered `Vec<WidgetId>` children, `Box<dyn Widget>` behaviour, sparse component tables, one explicit `Ui` context, renderer passed in at paint) as the working assumption. Then take up M-01 to M-08 as the mutability design. A full ECS can be revisited later as an adapter if a specific ECS host engine becomes a target.
