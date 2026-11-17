//! Port of TB's src/tb/tests/test_tb_parser.cpp, with its three `.tb.txt` files, plus
//! tests for the parts of the format TB's tests do not cover.

use turbo_ferret::lang::Language;
use turbo_ferret::res::{self, Lookup, MAX_DEPTH, ParseCtx, ResNode};
use turbo_ferret::{MemLoader, Value};

fn loader() -> MemLoader {
    MemLoader::new()
        .with("data/test_tb_parser.tb.txt", include_bytes!("data/test_tb_parser.tb.txt"))
        .with("data/test_tb_parser_included.tb.txt", include_bytes!("data/test_tb_parser_included.tb.txt"))
        .with("data/test_tb_parser_definitions.tb.txt", include_bytes!("data/test_tb_parser_definitions.tb.txt"))
}

// TB: TB_TEST(Init)
fn node() -> ResNode {
    let mut loader = loader();
    let mut ctx = ParseCtx::new("data/test_tb_parser.tb.txt").with_loader(&mut loader);
    let node = res::parse_file(&mut ctx).expect("parse");
    assert!(ctx.warnings.is_empty(), "{:?}", ctx.warnings);
    node
}

fn s<'a>(node: &'a ResNode, path: &str) -> std::borrow::Cow<'a, str> {
    node.str(path, "")
}

fn assert_float(v: f32, expected: f32) {
    assert!((v - expected).abs() < 1.0e-5, "{v} != {expected}");
}

#[test]
fn strings() {
    let node = node();
    assert_eq!(s(&node, "strings>string1"), "A string");
    assert_eq!(s(&node, "strings>string2"), "\"A string\"");
    assert_eq!(s(&node, "strings>string3"), "'A string'");
    assert_eq!(s(&node, "strings>string4"), "\"'A string'\"");
    assert_eq!(s(&node, "strings>string5"), "Foo\nBar");
}

#[test]
fn strings_compact() {
    let node = node();
    assert_eq!(s(&node, "strings_compact>string1"), "");
    assert_eq!(s(&node, "strings_compact>string2"), "A string");
    assert_eq!(s(&node, "strings_compact>string3"), "A string");
    assert_eq!(s(&node, "strings_compact>string4"), "'A string'");
    assert_eq!(s(&node, "strings_compact>string5"), "\"A string\"");
    assert_eq!(s(&node, "strings_compact>string6"), "\"A string\"");
    assert_eq!(s(&node, "strings_compact>string7"), "\\");
    assert_eq!(s(&node, "strings_compact>string8"), "\"");
    assert_eq!(s(&node, "strings_compact>string9"), "\\\\\\\\");
    assert_eq!(s(&node, "strings_compact>string10"), "\\\\\"");
    assert_eq!(s(&node, "strings_compact>string11"), "\"\"''");
    // TB: GetValueStringRaw. ResNode::str never translates.
    assert_eq!(s(&node, "strings_compact>string12"), "@language_string_token");
    assert_eq!(s(&node, "strings_compact>string13"), "#ffdd00");
}

#[test]
fn strings_compact_language() {
    // TB: GetValueString looks "@key" up in the language.
    let node = node();
    let mut lang = Language::new();
    let lookup = Lookup::new(&node).with_lang(&lang);
    let missing = lang.get_key("language_string_token");
    assert_eq!(lookup.str(&node, "strings_compact>string12", ""), missing);
    #[cfg(not(feature = "debug"))]
    assert_eq!(missing, turbo_ferret::lang::MISSING);
    #[cfg(feature = "debug")]
    assert_eq!(missing, "<TRANSLATE:language_string_token>");
    lang.set("language_string_token", "Translated");
    let lookup = Lookup::new(&node).with_lang(&lang);
    assert_eq!(lookup.str(&node, "strings_compact>string12", ""), "Translated");
    assert_eq!(lookup.str_raw(&node, "strings_compact>string12", ""), "@language_string_token");
    assert_eq!(lookup.str(&node, "strings_compact>string13", ""), "#ffdd00");
}

#[test]
fn numbers() {
    let node = node();
    assert_eq!(node.int("numbers>integer1", 0), 42);
    assert_float(node.float("numbers>float1", 0.0), 1.1);
    assert_eq!(node.int("numbers>integer2", 0), 42);
    assert_float(node.float("numbers>float2", 0.0), 1.1);
}

#[test]
fn numbers_compact() {
    let node = node();
    assert_eq!(node.int("numbers_compact>integer1", 0), -10);
    assert_eq!(node.int("numbers_compact>integer2", 0), -10);
    assert_float(node.float("numbers_compact>float1", 0.0), 1.0);
    assert_float(node.float("numbers_compact>float2", 0.0), -1.0);
    assert_float(node.float("numbers_compact>float3", 0.0), -0.2);
    assert_float(node.float("numbers_compact>float4", 0.0), -0.2);
    assert_float(node.float("numbers_compact>float5", 0.0), 1.1);
    assert_float(node.float("numbers_compact>float6", 0.0), 1.1);
}

#[test]
fn numbers_with_unit() {
    let node = node();
    assert_eq!(node.int("numbers_with_unit>number1", 0), 10);
    assert_eq!(s(&node, "numbers_with_unit>number1"), "10px");
    assert_eq!(node.int("numbers_with_unit>number2", 0), -10);
    assert_eq!(s(&node, "numbers_with_unit>number2"), "-10px");
    assert_eq!(node.int("numbers_with_unit>number3", 0), 10);
    assert_eq!(s(&node, "numbers_with_unit>number3"), "10px");
    assert_eq!(node.int("numbers_with_unit>number4", 0), -10);
    assert_eq!(s(&node, "numbers_with_unit>number4"), "-10px");
    assert_float(node.float("numbers_with_unit>number5", 0.0), -2.1);
    assert_eq!(s(&node, "numbers_with_unit>number5"), "-2.1px");
    assert_float(node.float("numbers_with_unit>number6", 0.0), -0.1);
    assert_eq!(s(&node, "numbers_with_unit>number6"), "-.1px");
    assert_float(node.float("numbers_with_unit>number7", 0.0), 1.0);
    assert_eq!(s(&node, "numbers_with_unit>number7"), "1px ");
}

#[test]
fn compact_with_children() {
    let node = node();
    assert_eq!(s(&node, "compact_with_children>string"), "A string");
    assert_eq!(s(&node, "compact_with_children>string>child1"), "Child 1");
    assert_eq!(s(&node, "compact_with_children>string>child2"), "Child 2");

    assert_eq!(node.int("compact_with_children>integer", 0), -10);
    assert_eq!(node.int("compact_with_children>integer>child1", 0), 1);
    assert_eq!(node.int("compact_with_children>integer>child2", 0), 2);

    assert_float(node.float("compact_with_children>float", 0.0), 1.0);
    assert_float(node.float("compact_with_children>float>child1", 0.0), 1.0);
    assert_float(node.float("compact_with_children>float>child2", 0.0), 2.0);
}

#[test]
#[allow(clippy::approx_constant)] // 3.14 is the fixture's value, not PI.
fn compact_no_value() {
    let node = node();
    assert_eq!(s(&node, "compact_no_value>string"), "A string");
    assert_eq!(node.int("compact_no_value>int", 0), 42);
    assert_float(node.float("compact_no_value>float", 0.0), 3.14);
    assert_eq!(s(&node, "compact_no_value>subgroup>string1"), "A string, with \"comma\"");
    assert_eq!(s(&node, "compact_no_value>subgroup>string2"), "'Another string'");
    assert_eq!(s(&node, "compact_no_value>subgroup>string3"), "And another string");
}

#[test]
fn arrays_numbers() {
    let node = node();
    let arr = node.get("arrays>numbers").expect("node").value.as_array().expect("array");
    assert_eq!(arr.len(), 5);
    assert_eq!(arr[0].as_int(), 1);
    assert_eq!(arr[1].as_int(), 2);
    assert_float(arr[2].as_float(), 0.5);
    assert_float(arr[3].as_float(), 1.0e-8);
    assert_eq!(arr[4].as_int(), 1_000_000_000);
}

#[test]
fn arrays_dimensions() {
    let node = node();
    let arr = node.get("arrays>dimensions").expect("node").value.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_int(), 1);
    assert_eq!(arr[1].as_int(), 2);
}

// TB: arrays_strings and arrays_mixed are commented out there ("Not supported yet").

#[test]
fn strings_multiline() {
    let node = node();
    assert_eq!(s(&node, "strings_multiline>string1"), "Line 1\nLine 2\nLine 3");
    assert_eq!(s(&node, "strings_multiline>string2"), "abc");
    assert_eq!(s(&node, "strings_multiline>string3"), "AB");
    assert_eq!(s(&node, "strings_multiline>string4"), "Line 1\nLine 2\nLine 3\n");
    assert_eq!(s(&node, "strings_multiline>subgroup>first"), "Foo");
    assert_eq!(s(&node, "strings_multiline>subgroup>second"), "AB");
    assert_eq!(s(&node, "strings_multiline>string5"), "The last string");
}

#[test]
fn comments_and_space() {
    let node = node();
    assert_eq!(node.int("comments_and_space>one", 0), 1);
    assert_eq!(node.int("comments_and_space>two", 0), 2);
    assert_eq!(node.int("comments_and_space>three", 0), 3);
}

#[test]
fn include_file() {
    let node = node();
    assert_eq!(s(&node, "include_file>file1>something1"), "Chocolate");
    assert_eq!(s(&node, "include_file>file1>something2"), "Cake");
    assert_eq!(s(&node, "include_file>file2>something1"), "Chocolate");
    assert_eq!(s(&node, "include_file>file2>something2"), "Cake");
}

#[test]
fn include_locally() {
    let node = node();
    assert_eq!(s(&node, "include_branch>test1>skin"), "DarkSkin");
    assert_eq!(s(&node, "include_branch>test2>skin"), "LightSkin");
}

#[test]
fn conditions() {
    let node = node();
    assert_eq!(node.int("conditions>value", 0), 42);
    // The @if/@else nodes are gone and only the true branch is left.
    let conditions = node.get("conditions").unwrap();
    assert_eq!(conditions.children().len(), 1);
}

#[test]
fn local_ref() {
    let node = node();
    assert_eq!(s(&node, "defines_test>test1"), "#ffdd00");
    assert_eq!(s(&node, "defines_test>test2"), "#ffdd00");
    assert_eq!(s(&node, "defines_test>broken"), "@>defines>colors>broken");
    assert_eq!(s(&node, "defines_test>cycle"), "@>defines_test>cycle");
}

#[test]
fn fs_loader_reads_the_same() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");
    let mut fs = turbo_ferret::FsLoader::new(dir);
    let from_fs = ResNode::load(&mut fs, "data/test_tb_parser.tb.txt", None).expect("parse");
    assert_eq!(from_fs, node());
}

// ---- Beyond TB's tests ----

fn parse_warn(src: &str) -> (ResNode, Vec<res::ParseError>) {
    let mut ctx = ParseCtx::new("test.tb.txt");
    let node = res::parse(src, &mut ctx).expect("parse");
    (node, ctx.warnings)
}

#[test]
fn value_kinds() {
    let node = ResNode::parse_str("a 1\nb 1.5\nc 1 2\nd text\ne\nf \ng: \"7\"\nh: 7\n").unwrap();
    assert_eq!(node.get("a").unwrap().value, Value::Int(1));
    assert_eq!(node.get("b").unwrap().value, Value::Float(1.5));
    assert_eq!(node.get("c").unwrap().value.as_array().unwrap().len(), 2);
    assert_eq!(node.get("d").unwrap().value, Value::str("text"));
    assert_eq!(node.get("e").unwrap().value, Value::Null);
    // A token followed by white space has an empty string value (TB).
    assert_eq!(node.get("f").unwrap().value, Value::str(""));
    // Quoted values are always strings.
    assert_eq!(node.get("g").unwrap().value, Value::str("7"));
    assert_eq!(node.get("h").unwrap().value, Value::Int(7));
    assert_eq!(&*node.name, "");
    assert!(node.value.is_null());
}

#[test]
fn bom_crlf_and_last_line() {
    let node = ResNode::parse_str("\u{feff}a 1\r\nb\r\n\tc 2\r\nd 3").unwrap();
    assert_eq!(node.int("a", 0), 1);
    assert_eq!(node.int("b>c", 0), 2);
    assert_eq!(node.int("d", 0), 3);
    assert_eq!(node.children().len(), 3);
}

#[test]
fn indentation_errors_skip_the_line() {
    let (node, warnings) = parse_warn("a\n\t\t\tbad 1\n\tok 2\n");
    assert_eq!(node.int("a>ok", 0), 2);
    assert!(node.get("a>bad").is_none());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].line, 2);
    assert_eq!(warnings[0].file, "test.tb.txt");
}

#[test]
fn indented_first_line_does_not_drop_the_rest() {
    // TB lost its target node here and dropped every later line.
    let (node, _) = parse_warn("\tlost 1\nkept 2\n");
    assert!(node.get("lost").is_none());
    assert_eq!(node.int("kept", 0), 2);
}

#[test]
fn compact_line_with_no_children_and_children_after() {
    let node = ResNode::parse_str("a:\n\tb 1\nc: 5, d: 6\n\te 7\n").unwrap();
    assert_eq!(node.int("a>b", 0), 1);
    assert_eq!(node.int("c", 0), 5);
    assert_eq!(node.int("c>d", 0), 6);
    // An indented line after a compact line goes into the compact line's node.
    assert_eq!(node.int("c>e", 0), 7);
}

#[test]
fn include_errors_are_warnings() {
    let (node, warnings) = parse_warn(
        "a\n\tx 1\n\
         b\n\t@include missing\n\
         c\n\t@include c\n\
         d\n\t@include @tree>x\n\
         e\n\t@file other.tb.txt\n\
         f\n\t@include a\n",
    );
    assert_eq!(warnings.len(), 4, "{warnings:?}");
    assert_eq!(warnings.iter().map(|w| w.line).collect::<Vec<_>>(), [4, 6, 8, 10]);
    assert_eq!(node.int("f>x", 0), 1);
}

#[test]
fn include_copies_in_order_after_existing_nodes() {
    let node = ResNode::parse_str("defs\n\tp 1\n\tq 2\nw\n\tfirst 0\n\t@include defs\n\tlast 3\n").unwrap();
    let names: Vec<&str> = node.get("w").unwrap().iter().map(|n| &*n.name).collect();
    assert_eq!(names, ["first", "p", "q", "last"]);
}

#[test]
fn include_into_a_child_of_itself_is_a_cycle() {
    let (node, warnings) = parse_warn("a\n\tb\n\t\t@include a\n");
    assert_eq!(warnings.len(), 1);
    assert_eq!(node.count_children_recursive(), 2);
}

#[test]
fn file_cycles_are_caught() {
    let mut loader = MemLoader::new()
        .with("dir/a.tb.txt", b"a 1\n@file b.tb.txt\n")
        .with("dir/b.tb.txt", b"b 2\n@file a.tb.txt\n@file sub/c.tb.txt\n")
        .with("dir/sub/c.tb.txt", b"c 3\n");
    let mut ctx = ParseCtx::new("dir/a.tb.txt").with_loader(&mut loader);
    let node = res::parse_file(&mut ctx).unwrap();
    assert_eq!(node.int("a", 0), 1);
    assert_eq!(node.int("b", 0), 2);
    assert_eq!(node.int("c", 0), 3);
    assert_eq!(ctx.warnings.len(), 1, "{:?}", ctx.warnings);
    assert_eq!(ctx.warnings[0].file, "dir/b.tb.txt");
    assert_eq!(ctx.warnings[0].line, 2);
}

#[test]
fn file_errors() {
    let mut loader = MemLoader::new().with("bad.tb.txt", b"a \xff\n");
    let err = ResNode::load(&mut loader, "bad.tb.txt", None).unwrap_err();
    assert_eq!(err.file, "bad.tb.txt");
    let err = ResNode::load(&mut loader, "missing.tb.txt", None).unwrap_err();
    assert_eq!(err.line, 0);
    assert!(err.to_string().starts_with("missing.tb.txt: "), "{err}");
}

#[test]
fn conditions_in_included_files() {
    let mut loader = MemLoader::new()
        .with("main.tb.txt", b"x\n\t@file inc.tb.txt\n")
        .with("inc.tb.txt", b"@if 0\n\tv 1\n@else\n\tv 2\n");
    let node = ResNode::load(&mut loader, "main.tb.txt", None).unwrap();
    assert_eq!(node.int("x>v", 0), 2);
    assert_eq!(node.get("x").unwrap().children().len(), 1);
}

#[test]
fn conditions_follow_local_refs() {
    let node = ResNode::parse_str("defs\n\ton 1\n\toff 0\n@if @>defs>off\n\ta 1\n@else\n\ta 2\n@if @>defs>on\n\tb 1\n")
        .unwrap();
    assert_eq!(node.int("a", 0), 2);
    assert_eq!(node.int("b", 0), 1);
}

#[test]
fn unterminated_multiline_is_a_warning() {
    let (node, warnings) = parse_warn("a\n\tb: \"x\" \\\n");
    assert!(node.get("a>b").is_none());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn deep_nesting_is_bounded() {
    let mut src = String::new();
    for i in 0..MAX_DEPTH + 10 {
        src.push_str(&"\t".repeat(i));
        src.push_str("n\n");
    }
    src.push_str("after 1\n");
    let (node, warnings) = parse_warn(&src);
    assert_eq!(node.height(), MAX_DEPTH + 1);
    // One warning: the lines below the skipped one have no node to go into.
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(node.int("after", 0), 1);
}

#[test]
fn exponential_includes_are_bounded() {
    let mut src = String::from("n0\n\tx 1\n\tx 1\n");
    for i in 1..40 {
        src.push_str(&format!("n{i}\n\t@include n{}\n\t@include n{}\n", i - 1, i - 1));
    }
    let err = res::parse(&src, &mut ParseCtx::new("big")).unwrap_err();
    assert!(err.msg.contains("nodes"), "{err}");
}

#[test]
fn hostile_input_does_not_panic() {
    let inputs = [
        "",
        "\n\n",
        ":",
        "::",
        ": : :",
        "a:",
        "a: \"",
        "a: '",
        "a: \"\\",
        "\\",
        "a \\x",
        "a \\u12",
        "@include",
        "@file",
        "@if",
        "@else",
        "@include @",
        "@include @>",
        "@include >",
        "a: @",
        "a: #fff, b",
        "a: [1, 2]",
        "\t\t",
        "a\n\t@include a\n",
        "x: \"a\" \\\n\n# c\n\t\"b\"",
        "a: b: \"c\" \\",
        "\u{feff}",
        "é: \"ü\" \\\n'ß'",
    ];
    for src in inputs {
        let _ = ResNode::parse_str(src);
    }
}

#[test]
fn get_create_and_mutate() {
    let mut node = ResNode::parse_str("a\n\tb 1\n").unwrap();
    node.get_mut("a>b").unwrap().value = Value::Int(5);
    assert_eq!(node.int("a>b", 0), 5);
    node.get_or_create("a>c>d").value = Value::str("new");
    assert_eq!(node.str("a>c>d", ""), "new");
    assert_eq!(node.int("a>missing", -1), -1);
    assert_eq!(node.str("a>missing", "def"), "def");
    // A node with a null value is found, so the default is not used (TB).
    assert_eq!(node.int("a>c", -1), 0);
    assert_eq!(node.iter_named("b").count(), 0);
    assert_eq!(node.get("a").unwrap().iter_named("b").count(), 1);
}

trait CountExt {
    fn count_children_recursive(&self) -> usize;
    fn height(&self) -> usize;
}

impl CountExt for ResNode {
    fn count_children_recursive(&self) -> usize {
        self.iter().map(|c| 1 + c.count_children_recursive()).sum()
    }
    fn height(&self) -> usize {
        1 + self.iter().map(CountExt::height).max().unwrap_or(0)
    }
}
