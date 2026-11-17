# Turbo Ferret

Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås (https://github.com/fruxo/turbobadger). It is not the original software, and it is not endorsed by its author.

A small, host-driven UI toolkit for games. The host owns the loop, feeds input and time, and calls `update` and `paint`. The core crate is `#![no_std]` + `alloc` with zero dependencies.

| Crate | What |
|---|---|
| `crates/turbo-ferret` | The core: widget tree, layout, skin, fonts (bitmap fonts), images, resources, PNG decoder, widgets (basic ones, scrolling, containers, lists, menus, message windows, edit fields, images). `no_std`. Feature `debug`: overlays and a debug window. |
| `crates/turbo-ferret-soft` | Software renderer into an RGBA buffer (`no_std`, zero deps); used for golden-image tests. |
| `crates/turbo-ferret-miniquad` | OpenGL/GLES/WebGL renderer and input adapter on miniquad. |
| `crates/turbo-ferret-ttf` | TrueType fonts: its own parser and rasterizer (`no_std`, no deps beyond the core). Optional. |
| `tools/fontgen` | `turbo-ferret-fontgen`: turns a TrueType font into a bitmap font, for apps that should not carry a rasterizer (not published). |
| `demo` | Turbo Badger's demo01, ported (not published). |

```sh
cargo test --workspace                 # unit, behaviour and golden-image tests
cargo run -p turbo-ferret-demo         # the windowed demo
BLESS=1 cargo test -p turbo-ferret-soft  # re-bless golden images in tests/golden/
cargo run -p turbo-ferret-fontgen -- font.ttf --sizes 14,28 --out fonts/name   # TrueType to bitmap font
```

Design docs are in `design/` (start with `port-design.md`; status in `decisions-and-status.md`).

## Licence

zlib, like Turbo Badger. See `LICENSE` (Turbo Ferret) and `NOTICE` (Turbo Badger's copyright and licence). The default skin in `resources/` is Turbo Badger's (public domain). The default font is rasterized from a copyrighted font and is used for the demo and tests only; it is not part of any published crate. `resources/vera.ttf` is the font Turbo Badger ships under that name (DejaVu Sans), under its own open licence (`resources/vera_license.txt`); `resources/vera_font` is that font as bitmaps.
