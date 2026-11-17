//! M0's "done when": every TB resource file under the repo's `resources/` parses.

use std::path::Path;

use turbo_ferret::FsLoader;
use turbo_ferret::res::{self, ParseCtx, ResNode};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources");

/// Every `*.tb.txt` under `dir`, as paths relative to [`ROOT`], sorted.
fn tb_files(dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            tb_files(&path, out);
        } else if path.to_string_lossy().ends_with(".tb.txt") {
            let rel = path.strip_prefix(ROOT).expect("under root");
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    out.sort();
}

fn load(path: &str) -> ResNode {
    let mut loader = FsLoader::new(ROOT);
    let mut ctx = ParseCtx::new(path).with_loader(&mut loader);
    let node = res::parse_file(&mut ctx).unwrap_or_else(|e| panic!("{e}"));
    assert!(ctx.warnings.is_empty(), "{path}: {:?}", ctx.warnings);
    node
}

#[test]
fn every_resource_file_parses() {
    let mut files = Vec::new();
    tb_files(Path::new(ROOT), &mut files);
    for dir in ["default_skin/", "default_font/", "language/", "demo01/ui_resources/", "demo01/skin/", "demo_fonts/"] {
        assert!(files.iter().any(|f| f.starts_with(dir)), "no files under {dir}");
    }
    for path in &files {
        let node = load(path);
        assert!(!node.children().is_empty(), "{path} is empty");
    }
}

#[test]
fn spot_checks() {
    let skin = load("default_skin/skin.tb.txt");
    assert_eq!(skin.int("description>base-dpi", 0), 96);
    assert_eq!(skin.get("description>supported-dpi").unwrap().value.as_array().unwrap().len(), 4);
    assert_eq!(skin.str("elements>TBButton>bitmap", ""), "button.png");
    assert_eq!(skin.str("elements>TBButton>overrides>element", ""), "TBButtonInGroup");
    let condition = skin.get("elements>TBButton>overrides>element>condition").unwrap();
    assert_eq!(condition.str("target", ""), "parent");
    assert_eq!(condition.str("value", ""), "button_group");

    let font = load("default_font/dejavu_white_with_shadow.tb.txt");
    let glyphs = font.iter().next().unwrap().str("glyph_str", "");
    assert!(glyphs.starts_with("!\"#$%&'()*+,-./0123456789:;<=>?@ABC"), "{glyphs}");
    assert!(glyphs.contains("[\\]^_"), "unknown escapes are kept: {glyphs}");

    // @include of a branch declared earlier in the same file.
    let snap = load("demo01/ui_resources/test_scroller_snap.tb.txt");
    assert_eq!(snap.int("TBScrollContainer>lp>width", 0), 200);
    assert_eq!(snap.int("TBScrollContainer>TBLayout>TBContainer>lp>height", 0), 300);

    // @include followed by children that go into the last included node.
    let layout = load("demo01/ui_resources/test_layout01.tb.txt");
    let containers: Vec<&ResNode> = layout
        .iter()
        .filter(|n| &*n.name == "TBLayout")
        .flat_map(|n| n.iter())
        .filter(|n| n.str("id", "") == "switch_layout")
        .flat_map(|n| n.iter_named("TBContainer"))
        .collect();
    assert_eq!(containers.len(), 3);
    for (i, c) in containers.iter().enumerate() {
        let inner = c.child("TBLayout").expect("included layout");
        assert_eq!(inner.int("id", 0), i as i32 + 1);
        assert_eq!(inner.iter_named("TBButton").count(), 5);
    }

    // @if 1 is resolved away.
    let edit = load("demo01/ui_resources/resource_edit_test.tb.txt");
    assert!(edit.get("TBLayout>@if").is_none());
    assert!(edit.get("TBLayout>TBContainer>TBLayout>TBSlider").is_some());
}
