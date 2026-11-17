# Turbo Ferret

A Rust port of Turbo Badger (TB), a small host-driven C++ UI toolkit for games, by
Emil Segerås (<https://github.com/fruxo/turbobadger>). The host owns the loop: it
feeds input and time to a `Ui`, calls `update()` and `paint(&mut renderer)`.

All milestones of the port (M0 to M8) are done and the code was reviewed. What
is left is in `design/decisions-and-status.md` ("Still open", "Review").

**Read before changing anything substantial:** `design/codebase-guide.md` (how
the code works, recipes, the traps). Then, as needed, `design/port-design.md`
(the design) and `design/decisions-and-status.md` (decisions, numbered
deviations from TB, status).

## Layout

| Path | What |
|---|---|
| `crates/turbo-ferret` | The core: tree, events, layout, skin, fonts, images, resources, widgets, text editing. `no_std` + `alloc`, no dependencies |
| `crates/turbo-ferret-soft` | Software renderer (`no_std`); the golden-image tests live here |
| `crates/turbo-ferret-miniquad` | GL renderer and input adapter on miniquad; system clipboard |
| `crates/turbo-ferret-ttf` | Optional TrueType backend: own parser and rasterizer (`no_std`) |
| `tools/fontgen` | `turbo-ferret-fontgen`: TrueType to bitmap font (not published) |
| `demo` | TB's demo01, ported; `demo/tests/screenshot.rs` drives it headlessly |
| `resources` | TB's default skin, language, demo layouts and images; fonts made from DejaVu Sans (never TB's own, see Rules) |
| `tests/golden` | Golden images (written by the soft renderer tests) |
| `design` | Design documents |
| `turbobadger/` | The C++ original, for reference. Gitignored: if it is missing, `git clone https://github.com/fruxo/turbobadger turbobadger` |

## Commands

Every change ends with all of these passing:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                     # core with the `debug` feature (the demo turns it on)
cargo test -p turbo-ferret                 # core with default features only
cargo test -p turbo-ferret --no-default-features --features std,png   # without the built-in font
cargo build -p turbo-ferret -p turbo-ferret-soft -p turbo-ferret-ttf --no-default-features --features turbo-ferret/png --target thumbv7em-none-eabihf
cargo build -p turbo-ferret --no-default-features --features default-font,debug --target thumbv7em-none-eabihf
cargo +1.86 check --workspace --all-targets          # MSRV
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Other useful ones:

```sh
cargo run -p turbo-ferret-demo                      # the windowed demo
./target/debug/turbo-ferret-demo --frames 90        # scripted run: draws 90 frames and quits
BLESS=1 cargo test -p turbo-ferret-soft             # rewrite golden images (then LOOK at them)
cargo test -p turbo-ferret-demo                     # headless demo flows; writes PNGs to target/
FUZZ_FULL=1 cargo test -p turbo-ferret --test review_edit_fuzz
cargo test -p turbo-ferret --test review_core_fuzz -- --ignored
cargo run -p turbo-ferret-fontgen -- resources/DejaVuSans.ttf --sizes 14,21,28 --out crates/turbo-ferret/src/font/default/dejavu_sans   # regenerate the built-in font
```

## Rules

- **Core, soft renderer and TrueType crate:** `#![no_std]` + `alloc`,
  `#![forbid(unsafe_code)]`, zero dependencies (the TrueType crate depends on the
  core only). Float math comes from `turbo_ferret::math`, not `std` or `libm`.
- **Dependencies:** the owner wants as few as possible. Prefer a small own
  implementation; if a dependency is really needed, make it optional and say so.
  Dev-dependencies and the tools are less strict (`png`, `fontdue =0.9.3`).
- **Rust:** edition 2024, MSRV 1.86 (no let-chains). `rustfmt.toml`: width 120.
- **Credit and licence:** ported code cites its origin in a comment,
  `// TB: file Function`. Keep the notices (`NOTICE`, zlib). Turbo Ferret is not
  the original software and is not endorsed by its author; say so where a crate
  introduces itself. The built-in font has its own licence
  (`crates/turbo-ferret/src/font/default/LICENSE-DejaVu.txt`); never name a
  derived font "Vera", "Bitstream" or "Arev".
- **Assets:** nothing of doubtful licence goes into the repository. TB's own
  bitmap fonts were rasterized from fonts their author could not vouch for and
  were removed, history included; every font here is made from DejaVu Sans by
  `turbo-ferret-fontgen` (the commands are in `resources/default_font/license.txt`,
  and tests fail if the files and the tool disagree). A new font needs an open
  licence and its notice next to it.
- **Deviations from TB:** when behaviour differs from TB on purpose, add a
  numbered entry to "Deviations" in `design/decisions-and-status.md`.
- **Tests:** a fix comes with a test that fails without it. Anything a user sees
  gets a golden image or a headless demo flow, and you look at the picture.
  Tests must not write the system clipboard (one `#[ignore]`d test does, and puts
  the text back).
- **Commits:** on `main`, small and described by what changed and why. End the
  message with the `Co-Authored-By` line the session gives you.
- **Keep the documents current:** this file, `design/codebase-guide.md` and
  `design/decisions-and-status.md` are how the next agent learns the code. Update
  them in the same change when you alter what they describe.

## The five things that bite

1. **A widget is "busy" while one of its own hooks runs.** Its behaviour
   (`Box<dyn Widget>`) is taken out of its node for the call, so
   `ui.get::<T>(id)` and `ui.with_widget::<T>(id, ..)` on it return `None` and
   hooks on it are skipped. Code that handles an event *from* widget A must not
   read A: the value comes with the event (`ev.value` for `Changed`; index in
   `ev.value` and item id in `ev.ref_id` for list and menu clicks). A few answers
   are cached for busy widgets in `Tree::busy` (scroll info, child translation,
   event destination, content root); `Ui::is_busy` tells.
2. **Removal detaches at once and frees later** (`flush`, at the top level).
   Ids are generational: a stale `WidgetId` is safe and every lookup on it gives
   `None`. Release things held outside the widget's subtree (global listeners,
   helper widgets) in `Widget::on_free`, which always runs; `on_removed` is
   skipped for a widget removed from inside its own hook.
3. **Paint is read-only.** `paint(&self, &mut PaintCtx)` cannot change the tree;
   use `Cell` for counters, `p.request_repaint()` to ask for another frame.
4. **Time and services are data.** The host calls `ui.set_time(ms)`; nothing
   reads a clock. Timers are messages (`post_message_delayed`), stepped by
   `update()`. `next_wake_time()` tells the host when to call `update` again.
5. **Resources are TB's text format** (`*.tb.txt`): tab indentation, `name value`,
   compact `TBButton: text: "Hi", id: ok`. Widget type names work with and
   without the `TB` prefix. `lp: width: 60` goes on its own line under the
   widget. `@key` is a language string, `@tree>path` and `@>path` are references.
