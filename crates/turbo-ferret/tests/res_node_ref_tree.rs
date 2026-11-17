//! Port of TB's src/tb/tests/test_tb_node_ref_tree.cpp.
//!
//! TB's tests inflate widgets and check their properties. Widgets arrive at M2, so these
//! check the values the widget reader would read (with the same lookups: references
//! followed, `@if` resolved, `@include` copied). The change listener half of
//! `change_on_set` is deferred with `TBNodeRefTreeListener` (port-design.md section 16).

use turbo_ferret::res::{self, Lookup, ParseCtx, RefTrees, ResNode};
use turbo_ferret::{TbId, Value, tbid};

fn parse_with(src: &str, refs: &RefTrees) -> ResNode {
    let mut ctx = ParseCtx::new("{data}").with_refs(refs);
    let node = res::parse(src, &mut ctx).expect("parse");
    assert!(ctx.warnings.is_empty(), "{:?}", ctx.warnings);
    node
}

/// The first top-level node whose `id` is `id` (TB: TBWidget::GetWidgetByID).
fn by_id<'a>(root: &'a ResNode, id: &str) -> &'a ResNode {
    root.iter().find(|n| n.str("id", "") == id).expect("widget id")
}

#[test]
fn change_on_set() {
    let mut dt = RefTrees::new();
    dt.set_value("r", "mainbar>size", Value::str("10mm"));
    dt.set_value("r", "mainbar>axis", Value::str("x"));
    dt.set_value("r", "mainbar>visible", Value::Int(42));

    assert_eq!(dt.value("r", "mainbar>size").unwrap().as_str(), "10mm");
    assert_eq!(dt.value("r", "mainbar>axis").unwrap().as_str(), "x");
    assert_eq!(dt.value("r", "mainbar>visible").unwrap().as_int(), 42);

    // Setting again replaces the value without adding nodes.
    dt.set_value("r", "mainbar>visible", Value::Int(7));
    assert_eq!(dt.value("r", "mainbar>visible").unwrap().as_int(), 7);
    assert_eq!(dt.get("r").unwrap().get("mainbar").unwrap().children().len(), 3);
    assert!(dt.value("r", "mainbar>missing").is_none());
    assert!(dt.value("nope", "mainbar>size").is_none());
}

#[test]
fn reference_value() {
    let mut refs = RefTrees::new();
    refs.set_from_str("test_styles", "FireButton\n\tskin: 'FireButtonSkin'\n").unwrap();

    let root = parse_with("TBButton: id: 'fire', skin: '@test_styles>FireButton>skin'", &refs);
    let button = by_id(&root, "fire");
    let lookup = Lookup::new(&root).with_refs(&refs);
    assert_eq!(TbId::new(&lookup.str(button, "skin", "")), tbid!("FireButtonSkin"));
    // Without the ref trees the reference stays as written.
    assert_eq!(button.str("skin", ""), "@test_styles>FireButton>skin");
}

#[test]
fn reference_value_recurse() {
    let mut refs = RefTrees::new();
    refs.set_from_str("test_foo", "foo_value: 42\nfoo_circular: '@test_bar>bar_circular'\n").unwrap();
    // The missing "\n" after bar_broken_node is in TB's test too.
    refs.set_from_str(
        "test_bar",
        "bar_value: '@test_foo>foo_value'\n\
         bar_circular: '@test_foo>foo_circular'\n\
         bar_circular2: '@test_bar>bar_circular'\n\
         bar_broken_node: '@test_foo>foo_broken_node'\
         bar_broken_tree: '@test_foo>foo_broken_tree'",
    )
    .unwrap();

    let root = parse_with(
        "TBInlineSelect: id: 'select', value: '@test_bar>bar_value'\n\
         TBButton: id: 'button_circular', text: '@test_bar>bar_circular'\n\
         TBButton: id: 'button_broken_node', text: '@test_bar>bar_broken_node'\n\
         TBButton: id: 'button_broken_tree', text: '@test_bad_tree>foo'\n",
        &refs,
    );
    let lookup = Lookup::new(&root).with_refs(&refs);

    // Reference from resource to one tree to another tree.
    assert_eq!(lookup.int(by_id(&root, "select"), "value", 0), 42);

    // Reference in a circular loop. Should not freeze.
    assert_eq!(lookup.str(by_id(&root, "button_circular"), "text", ""), "@test_bar>bar_circular");

    // Reference in a circular loop. Should not freeze.
    assert!(refs.value_from_tree("@test_bar>bar_circular2").is_none());

    // Referenced tree is wrong.
    assert!(refs.value_from_tree("@test_bad_tree>does_not_exist").is_none());

    // Reference that is broken (has no matching node).
    assert_eq!(lookup.str(by_id(&root, "button_broken_node"), "text", ""), "@test_foo>foo_broken_node");

    // Reference that is broken (has no matching tree).
    assert_eq!(lookup.str(by_id(&root, "button_broken_tree"), "text", ""), "@test_bad_tree>foo");
}

#[test]
fn reference_include() {
    let mut refs = RefTrees::new();
    refs.set_from_str("test_styles", "VeryNice\n\tskin: 'SpecialSkin'\n\ttext: 'hello'\n").unwrap();

    let root = parse_with("TBEditField: id: 'edit'\n\t@include @test_styles>VeryNice", &refs);
    let edit = by_id(&root, "edit");
    assert_eq!(&*edit.name, "TBEditField");
    assert_eq!(TbId::new(&edit.str("skin", "")), tbid!("SpecialSkin"));
    assert_eq!(edit.str("text", ""), "hello");
}

#[test]
fn reference_local_include() {
    let root = parse_with(
        "SomeDeclarations\n\
         \tskin: 'SpecialSkin'\n\
         \ttext: 'hello'\n\
         TBEditField: id: 'edit'\n\
         \t@include SomeDeclarations",
        &RefTrees::new(),
    );
    let edit = by_id(&root, "edit");
    assert_eq!(&*edit.name, "TBEditField");
    assert_eq!(TbId::new(&edit.str("skin", "")), tbid!("SpecialSkin"));
    assert_eq!(edit.str("text", ""), "hello");
}

#[test]
fn reference_condition() {
    let mut refs = RefTrees::new();
    refs.set_from_str("test_settings", "layout\n\tlandscape: 1\n").unwrap();

    let layout_str = "TBLayout: id: 'layout'\n\
                      \tdistribution: 'available'\n\
                      \t@if @test_settings>layout>landscape\n\
                      \t\taxis: 'x'\n\
                      \t\tspacing: 100px\n\
                      \t@else\n\
                      \t\taxis: 'y'\n\
                      \t\tspacing: 200px\n\
                      \tgravity: 'all'\n";

    // Inflate & check
    let root1 = parse_with(layout_str, &refs);
    let layout1 = by_id(&root1, "layout");
    assert_eq!(layout1.str("axis", ""), "x");
    assert_eq!(layout1.int("spacing", 0), 100);
    assert_eq!(layout1.str("gravity", ""), "all");

    // Change data for condition
    refs.set_value("test_settings", "layout>landscape", Value::Int(0));

    // Inflate & check
    let root2 = parse_with(layout_str, &refs);
    let layout2 = by_id(&root2, "layout");
    assert_eq!(layout2.str("axis", ""), "y");
    assert_eq!(layout2.int("spacing", 0), 200);
    assert_eq!(layout2.str("gravity", ""), "all");

    // Without the tree, the condition's value is the unresolved string, which is false.
    let root3 = res::parse(layout_str, &mut ParseCtx::new("{data}")).unwrap();
    assert_eq!(by_id(&root3, "layout").str("axis", ""), "y");
}

fn child(parent: &ResNode, index: usize) -> &ResNode {
    &parent.children()[index]
}

#[test]
fn reference_condition_branch_insert_order() {
    let src = "A\n\
               \tB1\n\
               \t\tC1\n\
               \t@if 1\n\
               \t\t@if 1\n\
               \t\t\tB2\n\
               \t\tB3\n\
               \t\tB4\n\
               \tB5\n";
    let node = ResNode::parse_str(src).unwrap();

    let a = child(&node, 0);
    assert_eq!(&*a.name, "A");
    assert_eq!(&*child(a, 0).name, "B1");
    assert_eq!(&*child(child(a, 0), 0).name, "C1");
    assert_eq!(&*child(a, 1).name, "B2");
    assert_eq!(&*child(a, 2).name, "B3");
    assert_eq!(&*child(a, 3).name, "B4");
    assert_eq!(&*child(a, 4).name, "B5");
    assert_eq!(a.children().len(), 5);
}

// ---- Beyond TB's tests ----

#[test]
fn refs_resolve_and_follow() {
    let mut refs = RefTrees::new();
    refs.set_from_str("t", "a: '@t>b'\nb: '@>c'\nc 5\nd: '@>missing'\n").unwrap();

    // "@>" inside a ref tree looks up from that tree's root.
    assert_eq!(refs.value_from_tree("@t>a"), Some(&Value::Int(5)));
    assert_eq!(refs.value("t", "a"), Some(&Value::Int(5)));
    // A chain that breaks part way stops at the last node reached.
    assert_eq!(refs.value_from_tree("@t>d"), Some(&Value::str("@>missing")));

    let r = Value::str("@t>a");
    assert_eq!(refs.resolve(&r), &Value::Int(5));
    let plain = Value::str("plain");
    assert_eq!(refs.resolve(&plain), &plain);
    let lang = Value::str("@key");
    assert_eq!(refs.resolve(&lang), &lang);
    assert!(refs.value_from_tree("@key").is_none());

    // Trees are matched by whole name (TB matched a prefix).
    assert!(refs.value_from_tree("@tt>c").is_none());
    assert!(refs.get("t").is_some());
    assert_eq!(refs.len(), 1);
    assert!(refs.remove("t").is_some());
    assert!(refs.is_empty());
}

#[test]
fn local_refs_need_the_document_root() {
    let root = ResNode::parse_str("defs\n\tc 7\nw\n\tv @>defs>c\n").unwrap();
    let w = root.get("w").unwrap();
    // Read from the root, the reference resolves.
    assert_eq!(root.int("w>v", 0), 7);
    // Read from a subtree, a Lookup supplies the root (TB used parent pointers).
    assert_eq!(Lookup::new(&root).int(w, "v", 0), 7);
    // With the subtree as its own root, the reference is broken.
    assert_eq!(w.str("v", ""), "@>defs>c");
}

#[test]
fn long_reference_chains() {
    let mut src = String::new();
    for i in 0..res::MAX_REF_HOPS {
        src.push_str(&format!("n{i} @>n{}\n", i + 1));
    }
    src.push_str(&format!("n{} 9\n", res::MAX_REF_HOPS));
    let root = ResNode::parse_str(&src).unwrap();
    assert_eq!(root.int("n0", 0), 9);
    // One hop more than the limit is treated like a loop.
    let src = format!("x @>n0\n{src}");
    let root = ResNode::parse_str(&src).unwrap();
    assert_eq!(root.str("x", ""), "@>n0");
    assert_eq!(root.int("n1", 0), 9);
}
