# TurboBadger C++ Architecture Breakdown

*Step one toward an idiomatic Rust port. This is analysis only. It contains no port design.*

**Source analysed:** `github.com/fruxo/turbobadger`, `master` at commit `389baa6` (24 July 2016, the latest commit). Author: Emil Segerås. License: zlib (see `src/tb/tb_core.h`). About 30k lines of the library's own C++ under `src/tb/`, plus vendored `stb_image.h` / `stb_truetype.h` and a small `utf8` helper. Demo apps and the GLFW/Android glue live in `Demo/` and `DemoAndroid/`.

**How to read this doc.** Each subsystem section says what it does, which files hold it, and how it connects to the rest. Spots that will be hard to carry over into Rust are tagged **`[H-nn]`**. The full hazard catalogue is in [section 15](#15-rust-translation-hazard-catalogue), so a later design pass (or Claude Code during the implementation) can grep for `[H-` and work through every one.

---

## Contents

1. [Big picture and frame loop](#1-big-picture-and-frame-loop)
2. [Core infrastructure: intrusive lists, hash tables, strings](#2-core-infrastructure-intrusive-lists-hash-tables-strings)
3. [IDs and hashing](#3-ids-and-hashing)
4. [Widget tree and lifecycle](#4-widget-tree-and-lifecycle)
5. [Events and input](#5-events-and-input)
6. [Layout](#6-layout)
7. [Skins](#7-skins)
8. [Renderer and backend abstraction (bitmaps, fragments, batching)](#8-renderer-and-backend-abstraction)
9. [Fonts](#9-fonts)
10. [The TB text resource format, parser and widget inflation](#10-the-tb-text-resource-format-parser-and-widget-inflation)
11. [Animation](#11-animation)
12. [Message system (timers and deferred work)](#12-message-system)
13. [Language and string system](#13-language-and-string-system)
14. [Platform backends](#14-platform-backends)
15. [Rust translation hazard catalogue](#15-rust-translation-hazard-catalogue)
16. [Memory and ownership summary](#16-memory-and-ownership-summary)
17. [Other widgets and pieces worth knowing about](#17-other-widgets-and-pieces-worth-knowing-about)
18. [Questions to settle in the design phase](#18-questions-to-settle-in-the-design-phase)

---

## 1. Big picture and frame loop

TurboBadger ("TB") is a retained-mode widget toolkit for games. The design goals are visible everywhere in the code: almost no dependencies, no STL, no exceptions, no RTTI, small allocations, and a renderer interface narrow enough to plug into any engine.

### Layering

```
 Application / game
   │  owns a root TBWidget, feeds input, calls Process + Paint each frame
   ▼
 ┌───────────────────────────────────────────────────────────────┐
 │ Widgets: TBWidget base + ~25 concrete widgets (tb_widgets_*,   │
 │ tb_layout, tb_window, tb_editfield, tb_select, ...)            │
 ├──────────────┬──────────────┬──────────────┬──────────────────┤
 │ Resources:   │ Skin         │ Fonts        │ Animation        │
 │ parser →     │ (TBSkin,     │ (TBFontMgr,  │ (TBAnimationMgr, │
 │ TBNode tree →│  elements,   │  faces,      │  widget anims)   │
 │ WidgetsReader│  conditions) │  glyph cache)│                  │
 ├──────────────┴──────┬───────┴──────────────┴──────────────────┤
 │ Bitmap fragments    │ Message queue (TBMessageHandler)        │
 │ (texture atlasing)  │ Values / widget-value binding           │
 ├─────────────────────┴─────────────────────────────────────────┤
 │ Porting interfaces: TBRenderer, TBBitmap, TBSystem, TBFile,    │
 │ TBClipboard, TBImageLoader, TBFontRenderer                     │
 ├───────────────────────────────────────────────────────────────┤
 │ Core: TBLinkList, TBHashTable, TBList, TBStr, TBTempBuffer,    │
 │ TBValue, TBID/hash, TBTypedObject, geometry, color, utf8       │
 └───────────────────────────────────────────────────────────────┘
```

### Global engine state

`tb_core.cpp` defines the engine singletons and `tb_core_init(TBRenderer*)` creates them **`[H-05]`**:

| Global | Type | Role |
|---|---|---|
| `g_renderer` | `TBRenderer*` | Supplied by the app; used directly by painting code everywhere |
| `g_tb_skin` | `TBSkin*` | The loaded skin |
| `g_font_manager` | `TBFontManager*` | Fonts and glyph cache |
| `g_tb_lng` | `TBLanguage*` | String table |
| `g_widgets_reader` | `TBWidgetsReader*` | Factory registry for resource inflation |
| `g_image_manager` | `TBImageManager*` | Optional (`TB_IMAGE`) image cache |
| `g_value_group` | `TBValueGroup` (static object) | Named values for widget data binding |

There are also many file-level and class-level statics: the message queues, the animating-object list, the widget-animation list, the list of `TBNodeRefTree`s, the global widget listeners, the registered factory chain, and the input state statics on `TBWidget` (hovered, captured and focused widget, and so on). See [H-04] and [H-05].

### The frame loop

TB never owns the loop. The demo's `App` (`Demo/platform/Application.cpp`) shows the expected order:

```cpp
// per frame
TBAnimationManager::Update();      // advance animations (may move/fade/kill widgets)
root.InvokeProcessStates();        // only if someone called InvalidateStates()
root.InvokeProcess();              // skin-change detection, then OnProcess/OnProcessAfterChildren (layout validates here)

g_renderer->BeginPaint(w, h);
root.InvokePaint(TBWidget::PaintProps());
g_renderer->EndPaint();
if (TBAnimationManager::HasAnimationsRunning()) root.Invalidate();
```

Input arrives out of band: the platform layer calls `root.InvokePointerDown/Up/Move`, `InvokeWheel`, `InvokeKey`, `InvokeTouch*` on the root widget. Timers arrive through `TBMessageHandler::ProcessMessages()`, which the platform calls when the timer it was asked to schedule (`TBSystem::RescheduleTimer`) fires.

Repaint is pull-based. `TBWidget::Invalidate()` walks up and calls `OnInvalid()` on ancestors, and the app's root widget turns that into a "please repaint" signal for the host.

---

## 2. Core infrastructure: intrusive lists, hash tables, strings

These containers are the foundation of almost every ownership decision in TB, so they come first.

### `TBLinkList` (`tb_linklist.h/.cpp`): the intrusive doubly linked list

- An object becomes listable by inheriting `TBLinkOf<T>`, which embeds `prev`, `next` and a `linklist` back-pointer **`[H-01]`**.
- `TBLinkListOf<T>` is the typed list. It does not own its elements; `TBLinkListAutoDeleteOf<T>` deletes them when it is destroyed.
- An object can only be in **one list per `TBLink` base**. To be in two lists at once, TB adds dummy bases: `TBWidgetListener : TBLinkOf<TBWidgetListener>, TBWidgetListenerGlobalLink`, `TBMessage : TBLinkOf<TBMessage>, TBMessageLink`, `TBWidgetAnimationObject : TBAnimationObject (itself TBLinkOf<TBAnimationObject>), TBLinkOf<TBWidgetAnimationObject>` **`[H-01]` `[H-08]`**.
- **Safe iterators.** `TBLinkListIterator` registers itself in a linked list of iterators kept on the list. When a link is removed, every live iterator that points at it steps past it automatically. This is how TB survives listeners and children being removed in the middle of iteration **`[H-02]` `[H-12]`**.
- `IsInList()` / `linklist != nullptr` is used as state. For example, `TBAnimationObject::IsAnimating()` just checks whether the object is linked into the manager's list.

Users of `TBLinkOf`: widgets (children list), widget listeners, value connections, value group listeners, messages, animation objects and listeners, skin conditions and state elements, node-tree nodes, ref trees, font renderers, font glyphs (LRU list), bitmap space allocator chunks, style-edit blocks and fragments, select item viewers, widget factories, and renderer listeners.

### Other containers

- **`TBListOf<T>` / `TBListAutoDeleteOf<T>`** (`tb_list.h`): a growable array of `T*` (a vector of raw pointers). The auto-delete variant owns its items.
- **`TBHashTable` / `TBHashTableOf<T>` / `TBHashTableAutoDeleteOf<T>`** (`tb_hashtable.h`): chained buckets keyed by `uint32` (in practice a `TBID`), with automatic rehash. It stores `void*` content internally. Used for skin elements, fonts, font infos, glyphs, language strings, touch info and widget values.
- **`TBStr`** (`tb_str.h`): a minimal heap `char*` string that is always valid (it falls back to a static `""`). It converts implicitly to `char*` / `const char*` and has no length field. `TBStrC` is a non-owning wrapper **`[H-15]`**.
- **`TBTempBuffer`** (`tb_tempbuffer.h`): a growable byte buffer used for string building, path assembly and parser scratch space.
- **`TBValue`** (`tb_value.h`): a tagged union of null, string, float, int, owned `TBTypedObject*`, or owned `TBValueArray*`. It carries ownership flags (`SET_NEW_COPY`, `SET_TAKE_OWNERSHIP`, `SET_AS_STATIC`), and **getters convert and mutate the stored representation** (`GetString()` is non-const and may turn an int into a string in place) **`[H-13]`**. `TakeOver()` is a manual move.
- **Geometry and color** (`tb_geometry.h`, `tb_color.h`): `TBRect`, `TBPoint`, `TBRegion`, `TBColor` (with `#rrggbb[aa]` parsing). These are plain value types and translate trivially.
- **`TBTypedObject`** (`tb_object.h`): homegrown RTTI. `GetTypeId<T>()` returns the address of a function-local `static char`. `TBOBJECT_SUBCLASS(clazz, base)` generates `IsOfTypeId` and `GetClassName`, and `TBSafeCast<T>(obj)` is the checked downcast **`[H-09]`**.
- **Enum flags** (`MAKE_ENUM_FLAG_COMBO`) and **packed bitfield unions** (`m_packed` / `m_packed_init`) appear on `TBWidget`, `TBLayout`, `TBValue`, `TBFontDescription` and `TBStyleEdit` **`[H-14]`**. These map easily to `bitflags`, so the tag is low severity.
- OOM handling: nearly every `new` is followed by a null check and a `false` return. This dates from builds without exceptions and can simply be dropped.

---

## 3. IDs and hashing

**Files:** `tb_id.h/.cpp`, `tb_hash.h/.cpp`.

- `TBID` wraps a `uint32`. It can be built from an integer or from a string, which is hashed with **32-bit FNV-1a**.
- `TBIDC("literal")` is a `constexpr` hash, so ID comparisons compile down to integer compares **`[H-21]`**. A Rust `const fn` covers this directly.
- In `TB_RUNTIME_DEBUG_INFO` builds, `TBID` also stores a `debug_string`. Setting an ID from a string records it, and a global table (in `tb_id.cpp`) warns about hash collisions. This **changes the size and layout of `TBID`** depending on a build flag **`[H-20]`**.
- IDs are used as the universal key for widget IDs (`id`), group IDs (radio groups), skin element names, font names and font-face IDs (`font id + packed size`), language string keys, message IDs, skin condition values, `ref_id` on events (for example `"copy"` and `"paste"` for `EVENT_TYPE_SHORTCUT`), and bitmap fragment keys (hash of the file name).
- `GetWidgetByID` is a **linear depth-first search** of the subtree each time it is called. There is no index.

The hash function is part of the de-facto data format: IDs given as integers in `.tb.txt` and IDs given as strings share one namespace. A port that wants identical behaviour must keep FNV-1a 32 with the same basis and prime (`2166136261`, `16777619`), and with the empty string hashing to 0.

---

## 4. Widget tree and lifecycle

**Files:** `tb_widgets.h/.cpp` (1,084 + 1,815 lines; the heart of the library), `tb_widgets_listener.h/.cpp`, `tb_window.h/.cpp`.

### Structure

```cpp
class TBWidget : public TBTypedObject, public TBLinkOf<TBWidget> {
    TBWidget *m_parent;                     // raw back pointer            [H-03]
    TBLinkListOf<TBWidget> m_children;      // intrusive child list        [H-01]
    TBLinkListOf<TBWidgetListener> m_listeners;
    TBWidgetValueConnection m_connection;   // data binding link
    TBRect m_rect;                          // relative to parent
    TBID m_id, m_group_id, m_skin_bg, m_skin_bg_expected;
    float m_opacity; WIDGET_STATE m_state; WIDGET_GRAVITY m_gravity;
    TBFontDescription m_font_desc;          // 0 = inherit from parent
    PreferredSize m_cached_ps; SizeConstraints m_cached_sc;  // layout cache [H-18]
    LayoutParams *m_layout_params;          // optional, heap
    TBScroller *m_scroller;                 // lazily created
    TBLongClickTimer *m_long_click_timer;   // lazily created
    union { struct { ...13 bit flags... } m_packed; uint16 m_packed_init; };  [H-14]
public:
    TBValue data;                           // free user slot
    static TBWidget *hovered_widget, *captured_widget, *focused_widget;  [H-04]
    static int pointer_down_widget_x/y, pointer_move_widget_x/y;
    static bool cancel_click, update_widget_states, update_skin_states, show_focus_state;
};
```

- **Siblings are ordered by z.** The first child is painted first (at the bottom) and the last child is painted on top. `AddChild(child, WIDGET_Z_TOP|BOTTOM)`, `AddChildRelative(child, BEFORE|AFTER, reference)` and `SetZ` change the order. `SetZInflate` controls where inflated children go; `TBButton` inflates its children at the bottom so that its own text field stays on top.
- **Traversal helpers:** `GetNextDeep`, `GetPrevDeep`, `GetLastLeaf`, `IsAncestorOf`, `GetParentRoot`, `GetParentWindow` (all of which walk the parent pointers), and `GetWidgetAt(x, y)` (hit testing that walks children back to front and calls the virtual `GetHitStatus`).
- **`GetContentRoot()`** (virtual) lets a composite widget redirect where children go. `TBScrollContainer` returns its inner `m_root`, and `TBTabContainer` returns its content area.

### "Fat base class" with about 35 virtual hooks [H-07]

`TBWidget` mixes three roles in one class:

1. **Lifecycle and tree callbacks:** `OnAdded`, `OnRemove`, `OnChildAdded`, `OnChildRemove`, `OnDie`, `OnInflate`, `OnInflateChild`, `OnSkinChanged`, `OnFontChanged`, `OnFocusChanged`, `OnVisibilityChanged`, `OnCaptureChanged`, `OnResized`, `OnScroll`, `OnInvalid`.
2. **Frame hooks:** `OnProcessStates`, `OnProcess`, `OnProcessAfterChildren`, `OnPaint`, `OnPaintChildren`, `OnEvent`.
3. **A generic property interface** that every widget may or may not implement: `SetValue/GetValue`, `SetValueDouble/GetValueDouble`, `SetText/GetText`, `SetAxis/GetAxis`, `GetChildTranslation`, `ScrollTo`, `GetScrollInfo`, `GetScrollRoot`, `GetPaddingRect`, `OnCalculatePreferredContentSize`, `OnCalculatePreferredSize`, `InvalidateLayout`, `GetHitStatus`, `GetCustomSkinCondition`, `GetContentRoot`, `GetEventDestination`.

Generic code (resource inflation, value binding, skin conditions) calls the property interface on any widget without knowing its type. For example, the reader does `widget->SetText(...)` and `widget->SetAxis(...)` for whatever was inflated.

### Ownership and lifecycle [H-10] [H-11]

- **A parent owns its heap-allocated children.** `~TBWidget` calls `DeleteAllChildren()`, which runs `RemoveChild` followed by `delete` on each child. The destructor asserts `!m_parent`, so a widget must be removed before it is deleted.
- **Some children are not heap-owned: they are by-value members of their parent** **`[H-11]`**. For example, `TBWindow` contains `TBMover m_mover; TBResizer m_resizer; TBTextField m_textfield; TBWidget m_close_button;` and links them into its own child list in its constructor. Its destructor has to `RemoveFromParent()` them first so that `DeleteAllChildren` doesn't `delete` member subobjects. `TBButton` (an `m_layout` plus `m_textfield`), `TBScrollContainer` (`m_root`, `m_scrollbar_x/y`), `TBEditField`, `TBSelectDropdown`, `TBInlineSelect` and others do the same. So the child list holds a mix of owned and borrowed nodes, and correctness depends on destructor order.
- **`Die()`** is the graceful delete. It sets `is_dying`, calls `OnDie()`, then asks the listeners (`TBWidgetListener::InvokeWidgetDying`). If any listener returns true, for example the animation manager starting a fade-out, the widget stays in the tree until that listener removes and deletes it. Otherwise it runs `RemoveFromParent(); delete this;` **`[H-10]`**. Dying widgets and their descendants get no input or focus (`GetIsDying()` walks the parents).
- **`TBWindow::Close()`** calls `Die()`, and the doc comment warns: "This window will be deleted after this call!"
- **The destructor scrubs global pointers.** `~TBWidget` nulls `hovered_widget`, `captured_widget` and `focused_widget`, scrubs every touch-info entry, and notifies `OnWidgetDelete` listeners. The listener header notes that this happens "in its destructor, so virtual functions are already gone" **`[H-19]`**.
- **`TBWidgetSafePointer`** is TB's weak pointer. It is itself a `TBWidgetListener` that adds itself as a local listener on the widget it points at, watches `OnWidgetDelete`, and nulls itself. It is used throughout event code because "this widget may be deleted after this call" (see `InvokeEvent`, `SetFocus`, and popups tracking their target) **`[H-10]`**.

### Widget listeners (`tb_widgets_listener.h`)

`TBWidgetListener` observes `OnWidgetDelete`, `OnWidgetDying`, `OnWidgetAdded`, `OnWidgetRemove`, `OnWidgetFocusChanged` and `OnWidgetInvokeEvent` (which can **intercept and block** events). A listener can be **local** (added to one widget's `m_listeners` and invoked only for that widget, before the global ones) or **global** (a static list, invoked for every widget) **`[H-05]`**. Because a listener object sits in both kinds of list, it needs the dual-link trick from section 2.

Global listeners in the library: `TBWidgetsAnimationManager`, `TBPopupWindow` (it closes itself when focus moves outside it, on a pointer-down outside it, or when its target dies), and `TBMessageWindow`. Every `TBWidgetSafePointer` is a local listener.

### State, visibility and focus

- `WIDGET_STATE` flags (disabled, focused, pressed, selected, hovered) mirror `SKIN_STATE` bit for bit (a comment says "must exactly match"). `GetAutoState()` adds automatic states, such as hovered when the widget is the hovered widget and pressed while it is captured.
- `WIDGET_VISIBILITY`: visible, invisible (still laid out) or gone (no layout).
- **Focus** is a single global `focused_widget`. `TBWindow` remembers `m_last_focus` (a safe pointer), so activating a window restores its focus. `MoveFocus` handles Tab navigation through `GetNextDeep` / `GetPrevDeep` within the window. `SetFocus` is defensive about re-entrancy: it re-checks `focused_widget == this` after each callback because `OnFocusChanged` can change focus again **`[H-12]`**.
- **Windows:** `TBWindow` is a widget with a title bar, a resizer and a close button, and it is a group root. Activation brings it to the top z, deactivates the other windows, and restores focus.

### Data binding (`tb_widget_value.h/.cpp`)

`TBWidgetValue` (a named `TBValue`) keeps a list of `TBWidgetValueConnection`s. Each widget embeds one connection with a raw `m_widget` back-pointer **`[H-03]`**. When a widget fires `EVENT_TYPE_CHANGED`, `InvokeEvent` calls `m_connection.SyncFromWidget(this)`, which pushes the value to the other connected widgets through the generic `SetValue/SetText` virtuals, with an `m_syncing` guard against loops. The resource attribute `connection: name` binds to the global `g_value_group` **`[H-05]`**.

Radio and toggle groups are a separate mechanism. `group-id` plus the nearest `is-group-root` ancestor makes `TBRadioButton` untoggle its siblings by walking the subtree.

---

## 5. Events and input

**Files:** `tb_widgets.h/.cpp` (the `Invoke*` methods), `tb_scroller.h/.cpp`.

### The event object

`TBWidgetEvent : TBTypedObject` carries `target`, `type`, `target_x/y`, `delta_x/y`, `count` (click count, with `GetCountCycle(n)` to fold it), `key` (a UCS-4 code point), `special_key`, `modifierkeys`, `ref_id` and `touch`. `TBWidgetEventFileDrop` is a subclass with a file list, picked out with `TBSafeCast` **`[H-09]`**.

Types: `CLICK`, `LONG_CLICK`, `POINTER_DOWN/UP/MOVE`, `TOUCH_DOWN/UP/MOVE/CANCEL`, `WHEEL`, `CHANGED`, `KEY_DOWN/UP`, `SHORTCUT` (with a `ref_id` such as "copy", "paste" or "undo"), `CONTEXT_MENU`, `FILE_DROP`, `CUSTOM`.

### Dispatch (`TBWidget::InvokeEvent`)

1. Set `ev.target = this`. Take a `TBWidgetSafePointer` to self.
2. Ask the listeners (`OnWidgetInvokeEvent`). Any of them can swallow the event. If self was deleted in the process, return "handled".
3. For `CHANGED`, invalidate skin states and sync the value connection. Check again whether self was deleted.
4. For `CLICK`, `LONG_CLICK`, `CHANGED` and `KEY_*`, call `InvalidateStates()`, which sets a global flag so `OnProcessStates` runs on the whole tree next frame.
5. **Bubble:** call `OnEvent(ev)` on this widget, then on `GetEventDestination()` (by default the parent), and so on until one returns true. `TBPopupWindow` overrides `GetEventDestination()` to return the widget that opened it, so menu clicks bubble to the opener rather than to the root.

There is no capture or tunnelling phase. Event handlers commonly delete widgets, close windows or open new ones from inside `OnEvent` **`[H-12]`**.

### Pointer pipeline (called on the root)

- `InvokePointerDown(x, y, click_count, mods, touch)`: if nothing is captured, hit-test with `GetWidgetAt` and set **`captured_widget`** and **`hovered_widget`** (globals **`[H-04]`**), activate the parent window, start a long-click timer for touch, stop any running scroller up the chain (and set `cancel_click`), and focus the nearest focusable ancestor. Then convert the coordinates to widget space and send `POINTER_DOWN`.
- `InvokePointerMove`: update the hovered widget and send `POINTER_MOVE` to the captured widget, or to the hovered one if nothing is captured. If no widget handles it, **panning** kicks in: once the drag passes `TBSystem::GetPanThreshold()`, the nearest scrollable ancestor (`GetScrollInfo`) is scrolled with `ScrollByRecursive`, `is_panning` is set, and the release will not produce a click.
- `InvokePointerUp`: send `POINTER_UP`, then `CLICK` if `!cancel_click` and the pointer is still inside the captured widget (`GetHitStatus`), then release capture.
- **Long click:** `TBLongClickTimer` is a private `TBMessageHandler` that posts a delayed message. When it fires, it sends `LONG_CLICK`. If that is not handled it sends `CONTEXT_MENU`, and if either is handled the coming click is suppressed.
- **Multi-touch:** `InvokeTouchDown/Up/Move/Cancel(id)`. Touch id 0 is mapped onto the normal pointer path. Other ids get `TOUCH_*` events, with per-id `TOUCH_INFO` (hovered and captured widget, coordinates) in a global hash table **`[H-04]`**.
- **Wheel:** `InvokeWheel` updates hover and sends `WHEEL` to the captured or hovered widget. Scrollable widgets (`TBLayout`, `TBScrollContainer` and others) handle it in their own `OnEvent`, usually with `ScrollBySmooth`.

### Keyboard

`InvokeKey(key, special_key, mods, down)` goes to **`focused_widget`**. If the widget has `click_by_key` and the key is Enter or Space, it emulates press and click, using **function-local statics** (`check_pressed_state`, `had_pressed_state`) to track key-repeat **`[H-04]`**. Otherwise it sends `KEY_DOWN/UP`, which bubbles. An unhandled Tab moves focus. The platform layer is expected to send `SHORTCUT` events before the key-down (the demo does this for Ctrl+C and similar).

### Scrolling

- A widget that scrolls its children implements `GetChildTranslation`, `ScrollTo` and `GetScrollInfo`, and can redirect to another widget with `GetScrollRoot`. `TBLayout` (overflow scroll), `TBScrollContainer`, `TBEditField` and `TBSelectList` do this.
- `TBScroller` (`tb_scroller.h`) provides kinetic or smooth scrolling. It is a private `TBMessageHandler` that re-posts itself every frame while moving, and it includes snap listeners **`[H-08]`**.

---

## 6. Layout

**Files:** `tb_layout.h/.cpp`, the `PreferredSize` / `LayoutParams` / `SizeConstraints` classes in `tb_widgets.h`, and `TBWidget::OnCalculatePreferredSize` and `GetPreferredSize`.

### Measurement model

- `PreferredSize { min_w/h, max_w/h, pref_w/h, size_dependency }`. `size_dependency` flags whether width depends on height or the reverse (wrapping text, for example).
- `SizeConstraints { available_w, available_h }`, where `NO_RESTRICTION` is 10,000.
- `LayoutParams` (optional and heap-allocated) holds per-widget overrides of min, max and preferred. It comes from `lp: width: ...` in resources.
- `GetPreferredSize(constraints)` applies the layout params to the constraints, **returns the cached value if the constraints are equal or `size_dependency == NONE`**, and otherwise calls the virtual `OnCalculatePreferredSize`, caches the result and applies the layout-param overrides **`[H-18]`**. This getter mutates the widget.
- The default `OnCalculatePreferredSize` = `OnCalculatePreferredContentSize` (virtual; by default the union of children's preferred sizes) + skin padding, with skin-specified `width`, `height`, `min-*`, `max-*` and `pref-*` taking priority and intrinsic bitmap size as a fallback.

### Invalidation

`InvalidateLayout(INVALIDATE_LAYOUT_RECURSIVE | TARGET_ONLY)` clears the cache bit and walks up parents (raw pointers **`[H-03]`**). `TBLayout` overrides it to also set `layout_is_invalid`. Layout is **lazy**: it happens in `TBLayout::OnProcess()` (part of `InvokeProcess`) and in `OnResized`, and also during measurement when a parent asks for a preferred size.

### `TBLayout`: the one real layout engine

A flexbox-like single-axis stack. Settings:

| Setting | Values |
|---|---|
| `axis` | x, y |
| `spacing` | px, or from the skin (`SPACING_FROM_SKIN`) |
| `size` (cross axis) | `gravity`, `preferred`, `available` |
| `position` (cross axis) | `center`, `left/top`, `right/bottom`, `gravity` |
| `distribution` (main axis) | `preferred`, `available`, `gravity` |
| `distribution-position` | `center`, `left/top`, `right/bottom` |
| `overflow` | `clip`, `scroll` (plus an optional fade-out painted at the edges) |
| `layout order` | bottom-to-top (creation order) or reversed |

The algorithm in `ValidateLayout` is written **only for the X axis**. For Y it rotates the inputs and outputs with `RotRect`, `RotPreferredSize`, `RotGravity` and `RotSizeConstraints`. It sums preferred sizes, then shrinks toward the minimums or grows toward the maximums of the children that `QualifyForExpansion` (gravity on both sides), then positions the children. The same function computes the layout's own `PreferredSize` when called in "calculate" mode. Children with `WIDGET_VISIBILITY_GONE` are skipped.

### Non-layout parents

A plain `TBWidget` parent does gravity-based anchoring in its default `OnResized`: a child with left and right gravity stretches, a child with only right gravity follows the right edge, and so on. `OnInflateChild` sizes inflated children to their preferred size. `TBWindow::ResizeToFitContent` uses `GetPreferredSize` on the whole window.

### Dimension units (`tb_dimension.h`)

`TBDimensionConverter` converts `dp` (the default unit, relative to the skin's `base-dpi`), `px` and `mm` into pixels. The skin chooses the destination DPI from its `supported-dpi` list, which also selects `@192`/`@288`/`@384` bitmap variants. Resource values such as `lp>width: 40dp` go through it.

Rust notes: the arithmetic translates cleanly. The hard parts are the **cache living inside the widget** (measurement needs `&mut` while parents iterate children), **recursive invalidation through parent pointers**, and **layout running inside a "process" pass that also runs user `OnProcess` code**.

---

## 7. Skins

**Files:** `tb_skin.h/.cpp`, `tb_skin_util.h/.cpp`, `tb_widget_skin_condition_context.h/.cpp`; data in `resources/default_skin/skin.tb.txt`.

### Data model

- `TBSkin` holds a `TBHashTableAutoDeleteOf<TBSkinElement>` keyed by `TBID(name)`, a `TBBitmapFragmentManager` (atlas), a `TBDimensionConverter`, and defaults (text color, disabled and placeholder opacity, spacing). It is a private `TBRendererListener`, so it can drop and reload bitmaps when the graphics context is lost **`[H-08]` `[H-22]`**.
- `TBSkinElement` has a name, an optional bitmap fragment, a type (`StretchBox` nine-slice with `cut`, `StretchBorder`, `StretchImage`, `Tile`, `Image`), `expand` (paint outside the rect, for shadows), padding, intrinsic, min, max and preferred sizes, `spacing`, `content-ofs`, image position and offset, `flip-x/y`, `opacity`, `text-color`, `background-color`, the bitmap DPI, and a free `TBValue tag`.
- Each element has four **state lists**, each a list of `TBSkinElementState { element_id, state mask, list<TBSkinCondition> }`:
  - **overrides:** painted *instead of* the base element when the state matches (exact match first, then partial).
  - **strong-overrides:** like overrides, but they also change layout properties such as padding. `TBWidget::InvokeSkinUpdatesInternal` detects when the effective element changes because a condition changed, calls `OnSkinChanged` and invalidates layout.
  - **children:** every matching child element is painted on top, in order.
  - **overlays:** painted after all siblings, from `OnPaintChildren`.
- **`TBSkinCondition`** = target (this, parent, ancestors, previous sibling, next sibling) × property (skin, window-active, axis, align, id, state, value, hover, capture, focus, custom) × value × test (`==` or `!=`). `TBWidgetSkinConditionContext` evaluates conditions by walking the widget tree (parent pointers, siblings). `PROPERTY_CUSTOM` calls back into the widget's virtual `GetCustomSkinCondition` **`[H-07]`**.
- Skin files support `clone <element>` (copy another element's properties), an override skin file loaded on top (`Load(skin, override_skin)`), and a `TBSkinListener` hook for custom properties.
- Recursion guards: `is_painting` and `is_getting` flags on each element prevent infinite loops in override chains. These are mutable state inside "read" operations **`[H-18]`**.

### Painting

`TBSkin::PaintSkin(rect, element_or_id, state, context)` resolves overrides, paints the element (nine-slice and so on through `g_renderer->DrawBitmap*`), paints matching children, and returns the element actually used, which the widget then uses for `text_color` and `content_ofs`. There is a generic `"generic_focus"` element that is painted around the focused child when its skin has no focus state.

### Widget paint pass (`TBWidget::InvokePaint`)

It skips the widget if opacity is 0, the rect is empty, or the widget is not visible. Then it computes the auto state, multiplies opacity (widget × skin × disabled factor), sets `g_renderer` opacity, translates by `m_rect.x/y`, paints the background skin, inherits `PaintProps.text_color`, calls `OnPaint`, translates by `content_ofs`, calls `OnPaintChildren` (which translates by the child translation, culls children against the clip rect, paints overlays, and paints the generic focus), and then undoes every translation and the opacity by hand **`[H-17]`**. A comment admits that per-widget opacity is done by multiplying vertex alpha, not by rendering to an offscreen target.

---

## 8. Renderer and backend abstraction

**Files:** `tb_renderer.h/.cpp`, `renderers/tb_renderer_batcher.*`, `renderers/tb_renderer_gl.*`, `tb_bitmap_fragment.*`, `image/tb_image_manager.*`, `tb_image_loader_stb.cpp`.

### The interface the host implements

```cpp
class TBBitmap  { virtual int Width(); virtual int Height(); virtual void SetData(uint32 *bgra); };
class TBRenderer {
  BeginPaint(w,h); EndPaint();
  Translate(dx,dy);                         // stateful, manual push/pop by callers   [H-17]
  SetOpacity(f); GetOpacity();
  SetClipRect(rect, add_to_current) -> old; GetClipRect();
  DrawBitmap(dst, src, TBBitmapFragment*|TBBitmap*);
  DrawBitmapColored(dst, src, color, frag|bitmap);   // bitmap as alpha mask (glyphs)
  DrawBitmapTile(dst, bitmap);
  FlushBitmapFragment(frag);
  CreateBitmap(w, h, uint32 *data) -> TBBitmap*;     // power-of-two sizes
  BeginBatchHint/EndBatchHint;
  listeners: OnContextLost / OnContextRestored        // intrusive list of TBRendererListener
};
```

That is the entire GPU surface: textured, colored quads, a translation, a scissor rect, and a global alpha. No paths, no gradients, no shaders. This narrowness is a big part of the "small game-engine" feel, and it is easy to keep in a port.

### `TBRendererBatcher`

A reusable base that turns draw calls into **one** vertex batch (`Vertex { x, y, u, v, rgba }`, fixed `VERTEX_BATCH_SIZE`). It flushes when the bitmap changes, when the batch is full, or when the clip rect changes. Subclasses implement `RenderBatch`, `SetClipRect` and `CreateBitmap`. `TBRendererGL` (fixed-function GL / GLES1) is the shipped backend. Deleting or updating a bitmap must call `FlushBitmap` first so that a pending batch never references a freed texture **`[H-22]`**.

### Bitmap fragments (atlasing)

- `TBBitmapFragmentManager` packs many small images into a few power-of-two `TBBitmap`s (`TBBitmapFragmentMap`). Packing is row/shelf based (`TBFragmentSpaceAllocator` rows, each with a `TBSpaceAllocator` of free and used spans, as intrusive lists).
- `TBBitmapFragment` = map + rect + row + space + id. It holds raw back-pointers into its map and allocator **`[H-03]`**. Its lifetime is owned by the manager; everyone else holds raw `TBBitmapFragment*` and must not outlive the manager.
- The skin, the glyph cache and the image manager each own a separate fragment manager.
- `TBImageLoader::CreateFromFile` is a porting hook (the stb implementation is shipped). The skin loads bitmaps through it, choosing the DPI-suffixed file.

### `TBImageManager` (optional, `TB_IMAGE`)

`TBImage` is a manually reference-counted handle to a `TBImageRep` (`IncRef`/`DecRef`). The rep holds a raw `image_manager` back-pointer, and the manager nulls it in every live rep when it is destroyed **`[H-03]`**. This is `Rc`-style sharing done by hand. It is used by `TBImageWidget`.

---

## 9. Fonts

**Files:** `tb_font_renderer.h/.cpp`, `tb_font_desc.h`, `tb_font_renderer_tbbf.cpp` (bitmap fonts), `tb_font_renderer_stb.cpp`, `tb_font_renderer_freetype.cpp`.

- **`TBFontDescription`**: font `TBID` + packed `{ size:15, italic:1, bold:1 }` (bold and italic are unused). `GetFontFaceID() = id + packed`. An all-zero description means "inherit from the parent widget".
- **`TBFontManager`** (global `g_font_manager` **`[H-05]`**) owns:
  - a list of `TBFontRenderer` backends (intrusive list, takes ownership) **`[H-01]`**,
  - `TBFontInfo` (name → file) registered with `AddFontInfo`,
  - created `TBFontFace`s keyed by face ID,
  - one shared `TBFontGlyphCache`,
  - the default and test descriptions. **Font ID 0 is a built-in test font that draws boxes**, so `GetFontFace` never returns null.
- **`TBFontRenderer`** (virtual, one instance per opened face; it is also the factory through `Create(manager, filename, desc)`). The manager tries each registered renderer until one accepts the file. Backends register through free functions such as `register_tbbf_font_renderer()`, chosen at compile time by `TB_FONT_RENDERER_*` defines **`[H-20]`**.
- **TBBF bitmap fonts:** a `.tb.txt` file lists `glyph_str` (the characters in order) and, per size, a PNG strip, ascent, descent, `advance_delta` and `space_advance`. Glyph boundaries are found by scanning the PNG for fully transparent columns between glyphs. Examples are in `resources/default_font/` and `Demo/fonts/`.
- **`TBFontFace`** measures (`GetStringWidth`) and draws (`DrawString`) UTF-8 strings. It lazily renders the glyphs it needs. It supports a background "shadow" font (`SetBackgroundFont`, a raw pointer to another face) and a `TBFontEffect` (blur).
- **`TBFontGlyphCache`:** a hash table of `TBFontGlyph` keyed by `hash(face id, codepoint)`, an LRU-ish intrusive list of rendered glyphs, and its own fragment manager sized `TB_GLYPH_CACHE_WIDTH×HEIGHT`. When the atlas is full it drops old glyph fragments. It is a `TBRendererListener` for context loss.
- Glyph drawing goes through `DrawBitmapColored` (the glyph is an alpha mask), with `has_rgb` for color glyphs.
- There is no shaping, kerning (it is commented out), bidi or fallback chain. Text is simply UTF-8 decoded with `utf8::decode_next`, one code point equals one glyph.

Text editing (`TBEditField` → `TBStyleEdit`, about 2,100 lines in `tb_style_edit.cpp`) builds on the font face. It is a block and fragment document model (intrusive lists of `TBBlock` → `TBTextFragment`), with caret and selection, word wrap, undo and redo, password mode, a simple inline styling markup (`<color #rrggbb>`, `<u>`, `<hr>`), pluggable content factories (`TBTextFragmentContentFactory`) and a syntax-highlighter hook. `TBEditField` is a widget, a `TBStyleEditListener` and a `TBMessageHandler` (caret blink) at once **`[H-08]`**.

---

## 10. The TB text resource format, parser and widget inflation

**Files:** `parser/tb_parser.*`, `tb_node_tree.*`, `tb_node_ref_tree.*`, `tb_value.*`, `tb_widgets_reader.*`; examples in `Demo/demo01/ui_resources/*.tb.txt` and `resources/default_skin/skin.tb.txt`. The tests in `tests/test_tb_parser*.tb.txt` are the best format spec.

### Syntax

A **tab-indented** tree of `name value` lines. The same format is used for UI layouts, skins, fonts, language files and arbitrary data.

```
# comment
WindowInfo
	title Animations                 # "name value": value parsed by SetFromStringAuto
TBLayout: axis: y, distribution: available   # compact form: "Name:" then "key: value, key: value" as children
	TBButton: text: "Animate!", id: "Animate!", skin: "SpecialButton"
	TBSelectList: id: "curve", value: 1
		items
			item: text: "Linear"
	TBEditField
		text Hello world\nHello world        # backslash escapes
		lp: width: 200dp                       # nested compact
@if @settings>advanced                         # conditional branch from a ref tree
	TBButton: text: @advanced_option            # "@id" = language string lookup
@else
	...
@include @defaults>button_props                # clone nodes from a ref tree
@include local>path                            # clone nodes from earlier in this file
@file other.tb.txt                             # include another file
```

- **Indentation** is tabs only, and one level deeper per line at most (more is an error).
- **Values** (`TBValue::SetFromStringAuto`): an int, a float, a string (quoted or not), or an **array** when the value is numbers separated by spaces or commas (`padding 6 8`, `rect: 0 0 100 50`). Colors are `#rrggbb[aa]`. Multi-line values continue when a quoted string or `[` bracket isn't closed.
- **Compact lines:** a token ending in `:` makes the rest of the line `key: value` pairs, which become children (`Enter`/`Leave`). Its own value comes first if it looks like a value.
- **References:** `@treename>path>to>node` reads a value from a named `TBNodeRefTree` (a global registry of named data trees, `s_ref_trees` **`[H-05]`**). `@>path` refers to the same tree. A plain `@word` in a string is a language lookup (resolved by `GetValueString`, not by the parser).
- **Conditionals and includes:** `@if` and `@else` are resolved by `TBNodeRefTree::ResolveConditions` after parsing. `@include` and `@file` are handled in `TBNodeTarget::OnToken` during parsing, with cycle detection by walking parent pointers.
- **Node lookup:** `GetNode("a>b>c")` walks children by name, linearly.

### Pipeline

```
file/buffer → TBParser (line-based, mutates buffer in place)   [H-15]
           → TBParserTarget callbacks (OnToken/Enter/Leave/OnError/OnComment)  (virtual interface)
           → TBNodeTarget builds TBNode tree (intrusive child list + raw m_parent)  [H-01][H-03]
           → TBNodeRefTree::ResolveConditions (@if/@else)
           → consumer: TBWidgetsReader / TBSkin::Load / TBFontRenderer / TBLanguage::Load
```

- `TBParser` uses `TBParserStream` (pull data in chunks) and `TBParserTarget` (push events). It is small and self-contained.
- `TBNode` = `char *m_name` + `TBValue` + intrusive children + parent pointer + a cycle ID for reference loops.
- `TBNodeRefTree` holds a `TBNode` and a name. `SetValue` notifies `TBNodeRefTreeListener`s. This is the hook for "live" data such as settings that UI conditions depend on.

### Widget inflation (`TBWidgetsReader`)

- **Factories self-register at static-initialisation time.** `TB_WIDGET_FACTORY(TBButton, TBValue::TYPE_NULL, WIDGET_Z_BOTTOM) { /* custom props */ }` expands to a factory class **plus a `static` instance** whose constructor pushes itself onto the global singly linked `g_registered_factories` **`[H-06]`**. `TBWidgetsReader::Init` copies that chain into its own list. There are 24 built-in factories.
- `CreateWidget(target, node)`: find the factory by **string compare of the node name** with each factory's name (a linear scan), `new` the widget, call the macro body `ReadCustomProps`, then call `widget->OnInflate(info)` (virtual; subclasses must call the base, and the **base `OnInflate` is what adds the widget to its parent**, which is checked by an assert), then recurse into the child nodes, then handle `autofocus`.
- Generic properties read by `TBWidget::OnInflate`: `id`, `group-id`, `value`, `data`, `is-group-root`, `is-focusable`, `want-long-click`, `ignore-input`, `opacity`, `text`, `connection`, `axis`, `gravity`, `visibility`, `state`, `skin`, `rect`, `lp>*`, `autofocus`, and `font>name|size`.
- `sync_type` on each factory says which `TBValue` type binds through `connection:` (int for checkboxes, float for sliders, string for text fields).
- `resources/metadata/tb_widgets_attr.tb.txt` documents the attributes for each widget. It is useful for tooling and for a port's test fixtures.

---

## 11. Animation

**Files:** `animation/tb_animation.*`, `animation/tb_animation_utils.h`, `animation/tb_widget_animation.*`.

- **`TBAnimationObject`** (virtual `OnAnimationStart`, `OnAnimationUpdate(progress)` and `OnAnimationStop(aborted)`, plus a listener list) is `TBTypedObject` + `TBLinkOf<TBAnimationObject>`. **Being in the manager's list *is* the animating state** **`[H-01]`**.
- **`TBAnimationManager`** is all static **`[H-05]`**: `animating_objects`, `Update()` (computes progress from `TBSystem::GetTimeMS()`, applies the curve, calls the callbacks, and on completion removes and **deletes** the object), `StartAnimation(obj, curve, duration, ANIMATION_TIME_IMMEDIATELY | FIRST_UPDATE)`, `AbortAnimation(obj, delete)`, `AbortAllAnimations`, and a block counter (`TBAnimationBlocker` is an RAII guard that makes new animations finish immediately).
- **Curves:** linear, slow-down, speed-up, bezier and smooth.
- **Ownership:** the manager owns running animations (`new` at the call site, `delete` by the manager). The object may also be deleted from outside, which removes it from the list.
- **Widget animations** (`TBWidgetAnimationOpacity`, `TBWidgetAnimationRect` with modes src→dst, delta-in and delta-out) hold a raw `TBWidget *m_widget` **`[H-03]`** and are also linked into a file-static `widget_animations` list (a second link base) **`[H-01]` `[H-05]`**.
- **`TBWidgetsAnimationManager`** is a global `TBWidgetListener` (a static instance registered in `Init`):
  - `OnWidgetDelete`: abort every animation that targets the widget, so no dangling `m_widget` is left.
  - `OnWidgetDying`: for `TBWindow`, `TBMessageWindow` and `TBDimmer`, start a fade or slide and return true to keep the widget alive. The opacity animation constructed with `die = true` finally removes and deletes the widget in `OnAnimationStop`. It uses `TBSafeCast` to decide **`[H-09]`**.
  - `OnWidgetAdded`: fade in newly added windows and dimmers.
- `TBAnimatedFloat` / `TBFloatAnimator` are small utilities for animating a float value.

So animation is coupled to widget lifetime through the global listener mechanism, and the "dying" protocol is the bridge between them.

---

## 12. Message system

**Files:** `tb_msg.h/.cpp`; used by `TBButton` (auto-repeat), `TBProgressSpinner`, `TBEditField` (caret blink), `TBScroller`, `TBLongClickTimer`, `TBSelectList`/`TBSelectDropdown`, and app code.

- **`TBMessageHandler`** is a mixin base class. A class inherits from it (often privately, through multiple inheritance **`[H-08]`**) and overrides `OnMessageReceived(TBMessage*)` **`[H-07]`**.
- **`TBMessage`** = `TBID message` + owned `TBMessageData *data` (a `TBTypedObject` with `v1`, `v2`, `id1` and `id2`, which can be subclassed) + `fire_time_ms` + **raw back-pointer `mh` to its handler** **`[H-03]`**. Each message is in **two intrusive lists**: its handler's `m_messages` and one of the two **file-global queues** `g_all_delayed_messages` (sorted by fire time) or `g_all_normal_messages` **`[H-01]` `[H-05]`**.
- API: `PostMessage(id, data)`, `PostMessageDelayed(id, data, ms)`, `PostMessageOnTime(id, data, time)`, `GetMessageByID`, `DeleteMessage` and `DeleteAllMessages`. The handler's destructor deletes its pending messages, which is what keeps the global queues free of dangling handlers.
- **`static ProcessMessages()`** delivers the due delayed messages first, then the normal ones. Each is unlinked, `mh->OnMessageReceived(msg)` is called, and the message is deleted. Handlers can post new messages or delete themselves during delivery; the safe iterators from section 2 make that survivable **`[H-02]` `[H-12]`**.
- **`static GetNextMessageFireTime()`** returns 0 (as soon as possible), the earliest delayed time, or `TB_NOT_SOON`.
- **`TBSystem::RescheduleTimer(fire_time)`** is the platform callback that runs whenever the head of the queue changes. The platform arms a one-shot timer and calls `ProcessMessages` when it fires. In the GLFW demo this lives in `port_glfw.cpp` and is folded into the event wait.

This is a single-threaded, timer-driven event loop. Nothing in TB is thread-safe.

---

## 13. Language and string system

**Files:** `tb_language.*`, `tb_str.*`, `tb_tempbuffer.*`, `utf8/utf8.*`.

- `TBLanguage` (global `g_tb_lng` **`[H-05]`**) is a flat `TBHashTableOf<TBStr>` keyed by `TBID(name)`. `Load(file)` parses a TB-format file of `key value` lines and **adds** to the table, so several files can be layered. `GetString(id)` returns `const char*`, or `"<TRANSLATE!>"` (in debug builds, `"<TRANSLATE:%s>"` written into a **function-local static `TBStr`**, which is neither re-entrant nor thread-safe) **`[H-04]`**.
- Resource strings that start with `@` are looked up when a value is read (`TBNode::GetValueString`). Code can also call `g_tb_lng->GetString(TBIDC("ok"))`, as `TBMessageWindow` does for its OK, Cancel, Yes and No buttons.
- There is no plural handling, no formatting, and no runtime language-switch notification: switching languages means reloading the UI.
- All text is **UTF-8 `char*`** with manual decoding through `utf8::decode_next` / `encode`. Key events carry UCS-4. String lengths are often passed as `TB_ALL_TO_TERMINATION` (`INT_MAX`), meaning "up to the NUL" **`[H-15]`**.

---

## 14. Platform backends

TB defines porting interfaces as **static classes or free functions that the integrator links in**, selected by `tb_config.h` defines **`[H-20]`**:

| Interface | Kind | Shipped implementations |
|---|---|---|
| `TBSystem` (`GetTimeMS`, `RescheduleTimer`, `GetLongClickDelayMS`, `GetPanThreshold`, `GetPixelsPerLine`, `GetDPI`) | static methods, link-time | `tb_system_linux.cpp`, `tb_system_win.cpp`, `tb_system_android.cpp` (`RescheduleTimer` is in the demo port) |
| `TBClipboard` (`Empty`, `HasText`, `SetText`, `GetText`) | static methods | dummy, GLFW, Windows |
| `TBFile::Open` + virtual `Size`/`Read` | static factory + vtable | POSIX (`tb_file_posix.cpp`), Android asset manager |
| `TBImageLoader::CreateFromFile` | static factory + vtable | stb_image |
| `TBRenderer` / `TBBitmap` | vtable, instance passed to `tb_core_init` | `TBRendererBatcher` → `TBRendererGL` (GL 1.x / GLES 1) |
| `TBFontRenderer` | vtable, registered at runtime | TBBF, stb_truetype, FreeType |
| Input | host calls `root->Invoke*` | GLFW callbacks (`Demo/platform/port_glfw.cpp`), Android JNI (`DemoAndroid`) |

`tb_config.h` toggles: `TB_RUNTIME_DEBUG_INFO`, `TB_UNIT_TESTING`, `TB_ALWAYS_SHOW_EDIT_FOCUS`, `TB_PREMULTIPLIED_ALPHA`, the font backends, `TB_IMAGE_LOADER_STB`, `TB_RENDERER_BATCHER`, `TB_RENDERER_GL` / `GLES_1`, the glyph cache size, `TB_IMAGE`, the clipboard, file and system choices, and the target OS.

The demo's `App` / `AppBackend` / `AppRootWidget` split shows the intended integration. The app owns the root widget **by value**, `AppRootWidget::OnInvalid` requests a repaint from the backend, and the backend owns the GL window, translates input and drives `Process` and `RenderFrame`.

There is a built-in unit test framework (`tests/tb_test.h`, macros `TB_TEST_GROUP`, `TB_TEST`, `TB_PASS`, `TB_VERIFY`) with tests for the parser, values, link lists, hash tables, node ref trees, the space allocator, style edit, widget values and geometry. These are good candidates to port first as behavioural specs.

---

## 15. Rust translation hazard catalogue

Severity is a rough guide to how much design work each item needs: **High** means it shapes the core architecture, **Med** means it needs a deliberate pattern, **Low** means it is mechanical.

| ID | Hazard | Where it appears | Severity |
|---|---|---|---|
| **H-01** | **Intrusive linked lists.** Objects embed `prev/next/linklist` via `TBLinkOf<T>`; membership in two lists uses dummy extra base classes; "is in list" is used as state. | Widget children, listeners, messages (2 lists), animations (2 lists), node tree, skin states/conditions, glyph LRU, fragment allocator, style-edit blocks/fragments, factories, ref trees, renderer listeners | High |
| **H-02** | **Safe iterators that auto-advance on removal.** Iteration tolerates arbitrary removal (and deletion) of the current element by callbacks. | `TBLinkListIterator`; message processing, listener invocation, animation update | High |
| **H-03** | **Raw parent and back pointers.** Children point at parents; helpers point at owners; nothing enforces lifetime. | `TBWidget::m_parent`, `TBNode::m_parent`, `TBWidgetValueConnection::m_widget`, `TBMessage::mh`, `TBWidgetAnimationObject::m_widget`, `TBBitmapFragment::m_map/m_row/m_space`, `TBImageRep::image_manager`, `TBFontFace::m_bgFont`, `TBSelectItemViewer::m_source` | High |
| **H-04** | **Static mutable widget/input state.** Hovered, captured and focused widget; pointer coordinates; `cancel_click`; update flags; touch-info table; function-local statics in `InvokeKey` and `TBLanguage::GetString`. | `tb_widgets.cpp`, `tb_language.cpp` | High |
| **H-05** | **Engine singletons and global registries.** `g_renderer`, `g_tb_skin`, `g_font_manager`, `g_tb_lng`, `g_widgets_reader`, `g_image_manager`, `g_value_group`, the global message queues, `TBAnimationManager` statics, `widget_animations`, `s_ref_trees`, global widget listeners. Used implicitly from paint, layout and inflation code. | `tb_core.cpp` and most `.cpp` files | High |
| **H-06** | **Registration by static initialisers.** `TB_WIDGET_FACTORY` creates static objects whose constructors link into `g_registered_factories` before `main`. | `tb_widgets_reader.*`, app-defined widgets | Med |
| **H-07** | **Deep inheritance and a fat virtual base.** About 35 virtual hooks on `TBWidget`, many acting as an untyped "property interface" (`SetValue`, `SetText`, `SetAxis`, scroll API); subclasses override selectively and must call the base (for example `OnInflate`, which adds the widget to its parent). Also `TBMessageHandler::OnMessageReceived`, the parser target, skin condition context, select item source, font renderer. | Whole widget hierarchy (3 to 4 levels: `TBWidget` → `TBButton` → `TBSectionHeader`; `TBWidget` → `TBWindow` → `TBPopupWindow` → `TBMenuWindow`) | High |
| **H-08** | **Multiple inheritance mixins.** Widgets also *are* message handlers, listeners or viewers. | `TBButton : TBWidget, TBMessageHandler`; `TBEditField : TBWidget, TBStyleEditListener, TBMessageHandler`; `TBSelectList : TBWidget, TBSelectItemViewer`; `TBPopupWindow : TBWindow, TBWidgetListener`; `TBSkin : TBRendererListener`; `TBScroller : TBMessageHandler` | Med |
| **H-09** | **Custom RTTI and downcasting.** `TBTypedObject`, `TBOBJECT_SUBCLASS`, `TBSafeCast<T>`, `GetWidgetByIDAndType<T>`; type IDs are addresses of statics. | Event subclasses, animation manager decisions, app code fetching typed widgets by ID | Med |
| **H-10** | **Self-deletion and use-after-free avoidance by convention.** `delete this` in `Die()`; widgets deleted inside their own event handlers; `TBWidgetSafePointer` as an ad-hoc weak reference implemented as a widget listener. | `TBWidget::Die`, `TBWindow::Close`, `InvokeEvent`, `SetFocus`, popups, message windows | High |
| **H-11** | **Children embedded by value in their parent.** The child list mixes heap-owned and member subobjects; destructors must unlink members before the base destructor deletes the remaining children. | `TBWindow`, `TBButton`, `TBScrollContainer`, `TBEditField`, `TBSelectDropdown`, `TBInlineSelect`, `TBTabContainer`, demo `App::m_root` | High |
| **H-12** | **Re-entrancy during traversal.** Callbacks (`OnEvent`, `OnFocusChanged`, listeners, `OnMessageReceived`, animation stop) freely add, remove or delete widgets, change focus, post messages. The code re-checks state after each callback. | Event dispatch, focus, message loop, animation update | High |
| **H-13** | **Flag-based ownership and mutating getters in `TBValue`.** `SET_NEW_COPY / TAKE_OWNERSHIP / AS_STATIC`; owns `TBTypedObject*` and arrays; `GetString()` converts in place; `TakeOver` is a manual move; `Copy` silently drops objects. | `tb_value.*`, parser, node tree, message data, `TBWidget::data`, skin `tag` | Med |
| **H-14** | **Packed bitfield unions.** `m_packed` / `m_packed_init` read as one integer and as bit fields; `MAKE_ENUM_FLAG_COMBO`. | `TBWidget`, `TBLayout`, `TBValue`, `TBFontDescription`, `TBStyleEdit` | Low |
| **H-15** | **C strings and in-place buffer mutation.** NUL-terminated `char*` everywhere; implicit `TBStr` → `char*`; parser writes NULs into the input; `TB_ALL_TO_TERMINATION` lengths; manual UTF-8. | `tb_str.*`, `tb_parser.cpp`, `TBNode::m_name`, font and edit code | Low/Med |
| **H-16** | **Out-parameters and bool error returns.** `GetChildTranslation(int &x, int &y)`, `GetText(TBStr &)`, `ScrollByRecursive(int &dx, int &dy)`; `false` for OOM. | Throughout | Low |
| **H-17** | **Renderer as a global mutable state machine.** Callers manually pair `Translate(+)`/`Translate(-)`, save and restore opacity and clip rect; any early return would corrupt state. | `TBWidget::InvokePaint`, `OnPaintChildren`, `TBLayout::OnPaintChildren`, skin painting | Med |
| **H-18** | **Hidden mutation inside "read" operations.** `GetPreferredSize` fills a cache; layout validates lazily during measurement; skin `is_painting/is_getting` recursion flags; `TBValue` getters; glyphs rendered on first measure. | Layout, skin, value, font | Med |
| **H-19** | **Callbacks from destructors.** `OnWidgetDelete` listeners run during `~TBWidget` (virtuals already gone); `~TBMessageHandler` deletes messages; destructors unlink from global lists. | Widget, message, animation, value connection | Med |
| **H-20** | **Preprocessor configuration and build-flag-dependent layout.** `tb_config.h` selects backends; `TB_RUNTIME_DEBUG_INFO` adds fields (`TBID::debug_string`, widget timing) and debug painting. | Whole library | Low (maps to Cargo features) |
| **H-21** | **Compile-time string hashing.** `constexpr` FNV-1a in `TBIDC`. | `tb_hash.h` | Low |
| **H-22** | **Graphics-context loss and the flush-before-free protocol.** Listeners drop and recreate textures; bitmaps must be flushed from the batch before deletion; raw `TBBitmap*` is shared between fragment maps and the batcher. | Renderer, skin, glyph cache, image manager | Med |

### Hazard clusters

- **The tree itself (H-01, H-03, H-10, H-11, H-12, H-19)** is the single biggest decision: how widgets are stored, referenced and mutated during dispatch. Everything else in TB assumes it can hold a `TBWidget*` and walk `m_parent` at any time.
- **Implicit context (H-04, H-05, H-06, H-17)**: painting, layout, inflation and input all reach for globals. A port has to decide whether those become an explicit context value passed in, or remain thread-local or singleton state.
- **Polymorphism (H-07, H-08, H-09)**: the widget class hierarchy, the generic property interface, mixin roles and typed lookup.
- **Data plumbing (H-13, H-15, H-18)**: `TBValue`, strings, and caches that need interior mutability.

---

## 16. Memory and ownership summary

| Object | Owner | How others refer to it |
|---|---|---|
| Widget (heap) | Parent widget's child list (deleted in `~TBWidget`) or, for roots, the app | Raw `TBWidget*`; `TBWidgetSafePointer` where deletion is likely; global hovered/captured/focused statics |
| Widget (member) | Enclosing widget object | Linked into the parent's children anyway |
| `LayoutParams`, `TBScroller`, `TBLongClickTimer` | The widget (lazy `new`, `delete` in destructor) | Private |
| `TBWidgetListener` | Whoever created it (often a member or static) | Intrusive lists on widgets and a global list; must be removed before the widget dies (asserted) |
| `TBMessage` + `TBMessageData` | `TBMessageHandler` (deleted after delivery or with the handler) | Global queues hold links |
| Animation object | `TBAnimationManager` while running (deleted on finish or abort) | Raw pointer at the call site; widget animations also in a global list |
| `TBNode` tree | Root node (often a stack object during load) | Raw child/parent pointers; `TBNodeRefTree` owns its node |
| `TBSkinElement` | `TBSkin` hash table | Raw `TBSkinElement*` returned from lookups and painting |
| `TBBitmapFragment` | Its `TBBitmapFragmentManager` | Raw pointers from skin elements, glyphs and images |
| `TBBitmap` | Fragment map (or the user) | Raw pointer in batch state |
| `TBFontFace`, `TBFontInfo`, `TBFontRenderer` | `TBFontManager` | Raw `TBFontFace*` from `GetFont()` (not stored long-term by widgets) |
| `TBFontGlyph` | `TBFontGlyphCache` hash table | Also in the LRU intrusive list |
| `TBImageRep` | Ref-counted by `TBImage` handles | Manager keeps a table; nulls back-pointers when it is destroyed |
| `TBWidgetValue` | `TBValueGroup` (global) or the user | Connections hold raw pointers both ways |
| Engine singletons | `tb_core_init` / `tb_core_shutdown` | Globals |

Shutdown order matters: the widgets must be gone before `tb_core_shutdown()`, which aborts animations and then deletes the image manager, reader, skin, font manager and language, in that order.

---

## 17. Other widgets and pieces worth knowing about

These are in scope for a port but are built on the subsystems above rather than being subsystems themselves.

- **Common widgets** (`tb_widgets_common.*`): `TBTextField`, `TBButton` (an internal `TBLayout` + `TBTextField`, toggle mode, auto-repeat through messages), `TBClickLabel`, `TBSkinImage`, `TBSeparator`, `TBProgressSpinner`, `TBRadioCheckBox` → `TBCheckBox` / `TBRadioButton`, `TBScrollBar`, `TBSlider`, `TBContainer`, `TBMover` (drags the parent window), `TBResizer`, `TBDimmer` (a modal backdrop).
- **Containers:** `TBScrollContainer` (with an inner `TBScrollContainerRoot` + 2 scrollbars), `TBTabContainer` (with a `TBTabLayout`), `TBToggleContainer` (toggles enabled, opacity or expanded state from its value), `TBSection` / `TBSectionHeader` (collapsible).
- **Lists and selection** (`tb_select.*`, `tb_select_item.*`): `TBSelectList`, `TBSelectDropdown` and `TBInlineSelect` use an **observer-style model and view**. `TBSelectItemSource` (abstract: `GetNumItems`, `GetItemString`, `CreateItemWidget`, sub-sources for menus, filter, sort) notifies `TBSelectItemViewer`s (intrusive list) about changes. `TBGenericStringItemSource` is the default. The viewer widgets build item widgets through `CreateItemWidget` and rebuild them when the source reports changes.
- **Popups:** `TBPopupWindow` (with alignment helpers, closes when the target loses focus or on an outside click, and its events bubble to the target), `TBMenuWindow` (a popup with a select list), `TBMessageWindow` (a modal OK/Cancel/Yes/No dialog with a dimmer; on a button click it invokes a `CLICK` with `ref_id` set to the button's ID, which bubbles to its target, then closes itself).
- **`TBImageWidget`** (`image/`): shows a `TBImage`.
- **Debug tooling** (`tb_debug.*`, `TB_RUNTIME_DEBUG_INFO`): a settings window that toggles layout bounds, layout and measure flashes, the fragment atlas view and glyph cache view. `TB_IF_DEBUG_SETTING` is sprinkled through the core.
- **Demo `ResourceEditWindow`**: a live editor that re-inflates TB text as you type. It is a good end-to-end test of the format and the reader.

---

## 18. Questions to settle in the design phase

These are listed only so the next step has a starting agenda. None of them is answered here.

1. How the widget tree is stored and how widgets refer to each other (H-01, H-03, H-10, H-11, H-12).
2. How dispatch can mutate the tree safely during events, listeners, messages and animations (H-02, H-12, H-19).
3. Whether the engine services (renderer, skin, fonts, language, reader, animations, message queue, input state) become one explicit context object, a set of contexts, or thread-local singletons (H-04, H-05).
4. How widget kinds and the generic property interface are expressed (H-07, H-08, H-09), and how app code fetches typed widgets by ID.
5. How factories are registered without static initialisers (H-06).
6. Whether the `.tb.txt` format, the FNV-1a ID hashing and the existing skins and fonts must stay byte-compatible, so that existing resources load unchanged.
7. Which renderer backend(s) ship first, and whether the `TBRenderer` quad-only surface is kept as is (H-17, H-22).
8. What `TBValue` becomes (H-13), and whether text stays UTF-8 `str` with simple per-code-point glyphs.
9. Which parts are first-milestone scope: core plus layout plus skin plus TBBF fonts plus basic widgets is a natural cut, with `TBStyleEdit`, select lists and the image manager later.
