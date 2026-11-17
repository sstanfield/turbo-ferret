# turbo-ferret-ttf

TrueType fonts for [Turbo Ferret](../../README.md), a Rust port of Turbo Badger by
Emil Segerås (<https://github.com/fruxo/turbobadger>). It is not the original
software, and it is not endorsed by its author.

A small parser and an anti-aliased rasterizer, written for this crate: `no_std`,
no `unsafe`, and no dependencies beyond `turbo-ferret`.

```rust
ui.fonts_mut().add_backend(Box::new(turbo_ferret_ttf::TtfBackend::new()));
ui.add_font("DejaVu", "DejaVuSans.ttf");
ui.create_font_face(FontDesc::new(tbid!("DejaVu"), 14));
```

Supported: TrueType outlines (`glyf`, simple and composite glyphs), Unicode
character maps of formats 4 and 12, the first font of a `.ttc`.

Not supported: PostScript outlines (`.otf` with `CFF `), hinting, kerning, shaping,
variable-font axes, color glyphs. Turbo Badger's TrueType backends did not kern or
shape either.

An app that should not carry a rasterizer can turn its fonts into bitmaps ahead of
time with `turbo-ferret-fontgen` and ship those.

The tests compare the output with `fontdue` (a dev-dependency only) and with
16x supersampling, and check that damaged files never panic.
