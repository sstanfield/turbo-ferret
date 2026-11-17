//! Language tables: TB's resources/language files, look-ups and the missing-key fallback.

use turbo_ferret::lang::Language;
use turbo_ferret::res::{Lookup, ResNode};
use turbo_ferret::{FsLoader, TbId, tbid};

fn resources() -> FsLoader {
    FsLoader::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources"))
}

fn english() -> Language {
    let mut lang = Language::new();
    lang.load(&mut resources(), "language/lng_en.tb.txt").expect("load");
    lang
}

#[test]
fn load_english() {
    let lang = english();
    assert_eq!(lang.len(), 16);
    assert_eq!(lang.get(tbid!("cut")), "Cut");
    assert_eq!(lang.get_key("selectall"), "Select all");
    assert_eq!(lang.get(TbId::new("TBMessageWindow.ok")), "Ok");
    assert_eq!(lang.get_key("TBList.header"), "Showing %d of %d");
    assert_eq!(lang.try_get(tbid!("search")), Some("Search"));
}

#[test]
fn missing_key_fallback() {
    let lang = english();
    assert_eq!(lang.try_get(tbid!("no-such-key")), None);
    #[cfg(not(feature = "debug"))]
    {
        assert_eq!(lang.get(tbid!("no-such-key")), "<TRANSLATE!>");
        assert_eq!(lang.get_key("no-such-key"), "<TRANSLATE!>");
        assert_eq!(lang.resolve("@no-such-key"), "<TRANSLATE!>");
    }
    #[cfg(feature = "debug")]
    {
        assert_eq!(lang.get_key("no-such-key"), "<TRANSLATE:no-such-key>");
        assert_eq!(lang.resolve("@no-such-key"), "<TRANSLATE:no-such-key>");
        assert!(lang.get(tbid!("no-such-key")).starts_with("<TRANSLATE:0x"));
    }
}

#[test]
fn load_is_additive() {
    let mut lang = english();
    lang.set("only-here", "Kept");
    // The Swedish file starts with a byte order mark.
    lang.load(&mut resources(), "language/lng_sv.tb.txt").expect("load");
    assert_eq!(lang.len(), 17);
    assert_eq!(lang.get_key("cut"), "Klipp ut");
    assert_eq!(lang.get_key("close"), "Stäng");
    assert_eq!(lang.get_key("only-here"), "Kept");
    lang.load_str("cut Cut again\n").unwrap();
    assert_eq!(lang.get_key("cut"), "Cut again");
    lang.clear();
    assert!(lang.is_empty());
}

#[test]
fn resolve_references() {
    let lang = english();
    assert_eq!(lang.resolve("@close"), "Close");
    // Not language strings: plain text, and node references (they contain '>').
    assert_eq!(lang.resolve("close"), "close");
    assert_eq!(lang.resolve("@>defs>close"), "@>defs>close");
    assert_eq!(lang.resolve("@tree>close"), "@tree>close");
}

#[test]
fn resource_strings_are_translated_through_a_lookup() {
    let lang = english();
    let root = ResNode::load(&mut resources(), "demo01/ui_resources/resource_edit_window.tb.txt", None).unwrap();
    let lookup = Lookup::new(&root).with_lang(&lang);
    let mut found = 0;
    walk(&root, &mut |n| {
        if &*n.name == "text" && n.value.get_str() == Some("@new") {
            found += 1;
        }
    });
    assert_eq!(found, 1);
    let new_button = find(&root, &|n| n.child("text").is_some_and(|t| t.value.get_str() == Some("@new"))).unwrap();
    assert_eq!(lookup.str(new_button, "text", ""), "New");
    assert_eq!(new_button.str("text", ""), "@new");
}

fn walk(node: &ResNode, f: &mut dyn FnMut(&ResNode)) {
    for c in node {
        f(c);
        walk(c, f);
    }
}

fn find<'a>(node: &'a ResNode, pred: &dyn Fn(&ResNode) -> bool) -> Option<&'a ResNode> {
    for c in node {
        if pred(c) {
            return Some(c);
        }
        if let Some(n) = find(c, pred) {
            return Some(n);
        }
    }
    None
}
