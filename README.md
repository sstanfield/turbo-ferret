# Turbo Ferret

Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås (https://github.com/fruxo/turbobadger). It is not the original software, and it is not endorsed by its author.

A small, host-driven UI toolkit for games. The host owns the loop, feeds input and time, and calls `update` and `paint`. The core crate is `#![no_std]` + `alloc` with zero dependencies.

| Crate | What |
|---|---|
| `crates/turbo-ferret` | The core: widget tree, layout, skin, fonts (TB bitmap fonts), resources, PNG decoder, widgets (basic ones, scrolling, containers, lists, menus and message windows). `no_std`. |
| `crates/turbo-ferret-soft` | Software renderer into an RGBA buffer (`no_std`, zero deps); used for golden-image tests. |
| `crates/turbo-ferret-miniquad` | OpenGL/GLES/WebGL renderer and input adapter on miniquad. |
| `demo` | Turbo Badger's demo01, ported (not published). |

```sh
cargo test --workspace                 # unit, behaviour and golden-image tests
cargo run -p turbo-ferret-demo         # the windowed demo
BLESS=1 cargo test -p turbo-ferret-soft  # re-bless golden images in tests/golden/
```

Design docs are in `design/` (start with `port-design.md`; status in `decisions-and-status.md`).

## Licence

zlib, like Turbo Badger. See `LICENSE` (Turbo Ferret) and `NOTICE` (Turbo Badger's copyright and licence). The default skin in `resources/` is Turbo Badger's (public domain). The default font is rasterized from a copyrighted font and is used for the demo and tests only; it is not part of any published crate.
