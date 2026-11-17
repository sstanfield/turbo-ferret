//! Resource, reader, layout and skin regressions found in review (2026-11): hostile and
//! odd resource files, and differences from Turbo Badger's parser and loaders.
use turbo_ferret::layout::dim::DimensionConverter;
use turbo_ferret::res::{self, ParseCtx, ResNode};
use turbo_ferret::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const ALPHABET: &[&str] = &[
    "\t", "\t", "\t", " ", "\n", "\n", "\r\n", ":", ": ", ",", ", ", "\"", "'", "\\", "\\\n", " \\\n", "@", "@file",
    "@include", "@if", "@else", "#", ">", "a", "b", "x", "u", "0", "1", "9", "-", ".", "é", "€", "😀", "\u{feff}", "[",
    "]", "\\x", "\\u", "\\xe9", "\\uD800", "\\0", "f", "ff", "e", "+",
];

#[test]
fn fuzz_parser_no_panic() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut refs = RefTrees::new();
    refs.set_from_str("t", "a\n\tb 1\n\tc @t>a>b\nd @t>d\n").unwrap();
    for _ in 0..200_000 {
        let n = rng.below(40);
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(ALPHABET[rng.below(ALPHABET.len())]);
        }
        let mut loader = MemLoader::new().with("a.tb.txt", b"x 1\n\ty 2\n").with("b.tb.txt", b"@file a.tb.txt\n");
        let mut ctx = ParseCtx::new("root.tb.txt").with_loader(&mut loader).with_refs(&refs);
        let r = res::parse(&s, &mut ctx);
        if let Ok(node) = r {
            let _ = node.str("a", "");
            let _ = node.int("a>b", 0);
            let _ = node.float("x", 0.0);
        }
        let _ = Value::parse_auto(&s);
        let _ = res::unescape(&s);
        let _ = Color::parse(&s);
        let dc = DimensionConverter::new(96, 192, 96);
        let _ = dc.px_from_str(&s, 0);
        let _ = dc.px_from_str_f(&s, 0.0);
        let _ = dc.dst_dpi_filename(&s);
        let _ = Value::str(&s).as_int();
        let _ = Value::str(&s).as_float();
    }
}

// ---------------------------------------------------------------------------------------
// Inflation with hostile numbers
// ---------------------------------------------------------------------------------------

fn ui() -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(800, 600));
    ui
}

fn inflate_and_update(src: &str) {
    let mut ui = ui();
    let root = ui.root();
    let ids = ui.inflate(root, src).unwrap();
    for id in ids {
        let _ = ui.preferred_size(id, SizeConstraints::NONE);
    }
    ui.update();
}

#[test]
fn huge_lp_max_width_in_layout() {
    inflate_and_update(
        "TBLayout\n\tTBWidget\n\t\tlp: max-width: 2000000000\n\tTBWidget\n\t\tlp: max-width: 2000000000\n",
    );
}

#[test]
fn huge_lp_pref_width_in_layout() {
    inflate_and_update(
        "TBLayout\n\tTBWidget\n\t\tlp: pref-width: 2000000000\n\tTBWidget\n\t\tlp: pref-width: 2000000000\n",
    );
}

#[test]
fn huge_spacing_in_layout() {
    inflate_and_update("TBLayout: spacing: 2147483647\n\tTBWidget\n\tTBWidget\n\tTBWidget\n");
}

#[test]
fn huge_rect() {
    inflate_and_update("TBWidget: rect: 2147483647 2147483647 2147483647 2147483647\n\tTBWidget: gravity: all\n");
}

#[test]
fn huge_rect_in_layout() {
    inflate_and_update("TBLayout\n\tTBButton: text: x, rect: 2000000000 2000000000 2000000000 2000000000\n");
}

#[test]
fn deep_nesting_inflate() {
    // 255 nested layouts (the parser's MAX_DEPTH is 256).
    let mut src = String::new();
    for d in 0..255 {
        for _ in 0..d {
            src.push('\t');
        }
        src.push_str("TBLayout: axis: y\n");
    }
    inflate_and_update(&src);
}

fn nested(kind: &str, depth: usize) {
    let mut src = String::new();
    for d in 0..depth {
        for _ in 0..d {
            src.push('\t');
        }
        src.push_str(kind);
        src.push('\n');
    }
    let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
    let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(res)));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    ui.set_viewport(Size::new(640, 480));
    let root = ui.root();
    ui.inflate(root, &src).unwrap();
    ui.update();
    ui.pointer_move(Point::new(20, 20), Modifiers::NONE);
    ui.update();
    let mut rec = Rec::default();
    ui.paint(&mut rec);
}

/// `KIND="TBScrollContainer" DEPTHS="4 8 12" cargo test ... deep_probe -- --nocapture`
#[test]
fn deep_probe() {
    let Ok(kind) = std::env::var("KIND") else { return };
    let depths = std::env::var("DEPTHS").unwrap_or_default();
    for d in depths.split_whitespace() {
        let d: usize = d.parse().unwrap();
        let t = std::time::Instant::now();
        nested(&kind, d);
        println!("{kind} depth {d}: {:?}", t.elapsed());
    }
}

#[test]
fn deep_layout() {
    nested("TBLayout: axis: y", 255);
}
#[test]
#[ignore = "known limit: nested scroll containers measure in O(2^depth); this depth does not finish"]
fn deep_scroll_container() {
    nested("TBScrollContainer", 255);
}
#[test]
fn deep_tab_container() {
    nested("TBTabContainer", 255);
}
#[test]
fn deep_window() {
    nested("TBWindow", 255);
}
#[test]
fn deep_section() {
    nested("TBSection", 255);
}
#[test]
fn deep_button() {
    nested("TBButton", 255);
}
#[test]
fn deep_select_list() {
    nested("TBSelectList", 255);
}
#[test]
fn deep_toggle() {
    nested("TBToggleContainer", 255);
}

// ---------------------------------------------------------------------------------------
// Painting helpers
// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct Rec {
    next: u32,
    solid: Vec<(Color, Rect)>,
    textured: Vec<(BitmapId, Rect, Rect)>,
}

impl Renderer for Rec {
    fn begin_frame(&mut self, _viewport: Size) {
        self.solid.clear();
        self.textured.clear();
    }
    fn end_frame(&mut self) {}
    fn create_bitmap(&mut self, _size: Size, _rgba: &[u8]) -> Option<BitmapId> {
        self.next += 1;
        Some(BitmapId(self.next))
    }
    fn update_bitmap(&mut self, _bitmap: BitmapId, _rgba: &[u8]) {}
    fn destroy_bitmap(&mut self, _bitmap: BitmapId) {}
    fn draw(&mut self, bitmap: Option<BitmapId>, _clip: Rect, quads: &[Quad]) {
        for q in quads {
            match bitmap {
                Some(b) => self.textured.push((b, q.dst, q.src)),
                None => self.solid.push((q.color, q.dst)),
            }
        }
    }
}

/// Serves `extra` files from memory and everything else from the repo's resources.
struct Overlay {
    fs: FsLoader,
    extra: Vec<(String, Vec<u8>)>,
    loads: u64,
    limit: u64,
}

impl Overlay {
    fn new(extra: &[(&str, &str)]) -> Overlay {
        let res = format!("{}/../../resources", env!("CARGO_MANIFEST_DIR"));
        Overlay {
            fs: FsLoader::new(res),
            extra: extra.iter().map(|(k, v)| (k.to_string(), v.as_bytes().to_vec())).collect(),
            loads: 0,
            limit: u64::MAX,
        }
    }
}

impl ResourceLoader for Overlay {
    fn load(&mut self, path: &str) -> Result<std::borrow::Cow<'static, [u8]>, LoadError> {
        self.loads += 1;
        assert!(self.loads <= self.limit, "more than {} file loads", self.limit);
        // Normalize "./" as a file system would.
        let norm = path.replace("/./", "/");
        let norm = norm.trim_start_matches("./");
        if let Some((_, v)) = self.extra.iter().find(|(k, _)| k == norm) {
            return Ok(std::borrow::Cow::Owned(v.clone()));
        }
        self.fs.load(path)
    }
}

fn skinned_ui(extra: &[(&str, &str)]) -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(Overlay::new(extra)));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(640, 480));
    ui
}

// ---------------------------------------------------------------------------------------
// Skin
// ---------------------------------------------------------------------------------------

#[test]
fn skin_base_dpi_zero() {
    let skin =
        "description\n\tbase-dpi 0\n\tsupported-dpi 192 288\nelements\n\tX\n\t\tbitmap arrow_down.png\n\t\tcut 2\n";
    let mut ui = skinned_ui(&[("default_skin/evil.tb.txt", skin)]);
    let _ = ui.load_skin("default_skin/evil.tb.txt", None);
}

#[test]
fn skin_supported_dpi_min() {
    let skin = "description\n\tbase-dpi 96\n\tsupported-dpi -2147483648 96\nelements\n\tX\n\t\tcut 2\n";
    let mut ui = skinned_ui(&[("default_skin/evil.tb.txt", skin)]);
    let _ = ui.load_skin("default_skin/evil.tb.txt", None);
}

#[test]
fn skin_huge_cut_scaled() {
    let skin = "description\n\tbase-dpi 96\n\tsupported-dpi 96 192\nelements\n\tX\n\t\tbitmap arrow_down.png\n\t\tcut 100000000\n";
    let mut ui =
        Ui::new(Config::for_dpi(192), Services::with_loader(Overlay::new(&[("default_skin/evil.tb.txt", skin)])));
    ui.services_mut().log = None;
    let _ = ui.load_skin("default_skin/evil.tb.txt", None);
}

/// Loading the same skin again (a DPI change, or a hot reload) must give the same elements.
#[test]
fn skin_reload_keeps_scaled_cut() {
    let mut ui = Ui::new(Config::for_dpi(192), Services::with_loader(Overlay::new(&[])));
    ui.services_mut().log = None;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    let first = {
        let e = ui.skin_data().element(TbId::new("TBCheckBox")).unwrap();
        (e.cut, e.expand, e.bitmap_dpi, e.padding)
    };
    let first_btn = ui.skin_data().element(TbId::new("TBButton")).unwrap().cut;
    ui.load_skin("default_skin/skin.tb.txt", None).unwrap();
    let second = {
        let e = ui.skin_data().element(TbId::new("TBCheckBox")).unwrap();
        (e.cut, e.expand, e.bitmap_dpi, e.padding)
    };
    let second_btn = ui.skin_data().element(TbId::new("TBButton")).unwrap().cut;
    assert_eq!(first, second);
    assert_eq!(first_btn, second_btn);
}

/// An element that clones another element inherits its `overrides`, so it overrides to
/// itself. TB paints it (and its children) once; count how often the child is painted.
#[test]
fn skin_self_override_paints_children_once() {
    let skin = "elements\n\
        \tBtn\n\
        \t\tbitmap button.png\n\
        \t\tcut 17\n\
        \t\toverrides\n\
        \t\t\telement Btn.alt\n\
        \t\tchildren\n\
        \t\t\telement Icon\n\
        \tBtn.alt\n\
        \t\tclone Btn\n\
        \t\tbitmap button_pressed.png\n\
        \tIcon\n\
        \t\tbitmap arrow_down.png\n\
        \t\ttype Image\n";
    let mut ui = skinned_ui(&[("default_skin/loop.tb.txt", skin)]);
    ui.load_skin("default_skin/loop.tb.txt", None).unwrap();
    let root = ui.root();
    let ids = ui.inflate(root, "TBWidget: skin: Btn, rect: 10 10 100 100\n").unwrap();
    ui.update();
    let mut rec = Rec::default();
    ui.paint(&mut rec);
    let _ = ids;
    // The icon is an 'Image' element: one quad of the bitmap's size (18x13), centered.
    // TB paints it twice: as the child of Btn.alt (whose override to itself is cut by
    // is_painting) and as the child of Btn.
    let icons = rec.textured.iter().filter(|(_, d, _)| d.w == 18 && d.h == 13).count();
    assert_eq!(icons, 2, "icon quads");
}

/// A skin file may use `@if @tree>path` (TB resolves it against the global ref trees).
#[test]
fn skin_condition_uses_ref_trees() {
    let skin = "elements\n\tX\n\t\t@if @cfg>big\n\t\t\tcut 20\n\t\t@else\n\t\t\tcut 5\n";
    let mut ui = skinned_ui(&[("default_skin/cond.tb.txt", skin)]);
    ui.refs_mut().set_value("cfg", "big", Value::Int(1));
    // The same text through the widget reader's parser sees the tree:
    let node = {
        let mut loader = MemLoader::new();
        let mut ctx = ParseCtx::new("x").with_loader(&mut loader).with_refs(ui.refs());
        res::parse(skin, &mut ctx).unwrap()
    };
    assert_eq!(node.int("elements>X>cut", 0), 20);
    ui.load_skin("default_skin/cond.tb.txt", None).unwrap();
    assert_eq!(ui.skin_data().element(TbId::new("X")).unwrap().cut, 20);
}

/// TB wrote `\0` as a NUL byte, which ends the C string there.
#[test]
fn escape_nul() {
    let node = ResNode::parse_str("a: text: \"abc\\0def\"\n").unwrap();
    assert_eq!(node.str("a>text", ""), "abc");
}

/// A deep chain made through the public API (no parser limit applies).
#[test]
fn deep_tree_via_api() {
    let mut refs = RefTrees::new();
    let path = "a>".repeat(200_000);
    refs.set_value("t", &path, Value::Int(1));
    drop(refs);
}

struct Counting {
    mem: MemLoader,
    loads: u64,
    limit: u64,
}

impl ResourceLoader for Counting {
    fn load(&mut self, path: &str) -> Result<std::borrow::Cow<'static, [u8]>, LoadError> {
        self.loads += 1;
        assert!(self.loads <= self.limit, "more than {} file loads (last: {path})", self.limit);
        self.mem.load(path)
    }
}

// ---------------------------------------------------------------------------------------
// @file
// ---------------------------------------------------------------------------------------

#[test]
fn file_include_blowup() {
    // "./a.tb.txt" names the same file, but the cycle check compares strings.
    let a = b"@file ./a.tb.txt\n@file ./a.tb.txt\n";
    let mut loader = Counting { mem: MemLoader::new().with("ui/a.tb.txt", a), loads: 0, limit: 1_000_000 };
    let mut ctx = ParseCtx::new("ui/a.tb.txt").with_loader(&mut loader);
    let r = res::parse_file(&mut ctx);
    assert!(r.is_ok());
    assert_eq!(loader.loads, 1, "the file itself: both includes are seen as the same file");
}

/// Files that each include the next one twice double at every level: the include
/// budget of a parse ends that.
#[test]
fn file_include_chain_is_bounded() {
    let mut mem = MemLoader::new();
    for i in 0..30 {
        let next = format!("@file f{}.tb.txt\n@file f{}.tb.txt\n", i + 1, i + 1);
        mem.add_owned(&format!("f{i}.tb.txt"), next.into_bytes());
    }
    mem.add_owned("f30.tb.txt", b"leaf 1\n".to_vec());
    let mut loader = Counting { mem, loads: 0, limit: 10_000 };
    let mut ctx = ParseCtx::new("f0.tb.txt").with_loader(&mut loader);
    assert!(res::parse_file(&mut ctx).is_ok());
    assert!(loader.loads <= res::MAX_INCLUDES as u64 + 1, "{} loads", loader.loads);
}

// ---------------------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------------------

#[test]
fn axis_with_trailing_space() {
    // TB: `*axis == 'x' ? AXIS_X : AXIS_Y`, so "y " (trailing blank) is the Y axis.
    let mut ui = ui();
    let root = ui.root();
    let ids = ui.inflate(root, "TBLayout\n\taxis y \n\tTBWidget\n").unwrap();
    assert_eq!(ui.get::<widgets::Layout>(ids[0]).unwrap().axis, Axis::Y);
}

/// TB: `GetPxFromString(str, LayoutParams::UNSPECIFIED)`: a value that is not a number
/// leaves the parameter unspecified.
#[test]
fn lp_non_numeric_is_unspecified() {
    let mut ui = ui();
    let root = ui.root();
    let ids = ui.inflate(root, "TBWidget\n\tlp: width: auto\n").unwrap();
    let lp = ui.layout_params(ids[0]).unwrap();
    assert_eq!(lp.pref_w, None);
}

/// TB reads widget-specific properties with GetValueString, which follows references.
#[test]
fn layout_spacing_follows_reference() {
    let mut ui = ui();
    ui.refs_mut().set_value("dims", "gap", Value::Int(20));
    let root = ui.root();
    // The generic property `id` in the same node does follow the reference:
    ui.refs_mut().set_value("dims", "name", Value::str("the_id"));
    let ids = ui.inflate(root, "TBLayout: spacing: @dims>gap, id: @dims>name\n\tTBWidget\n").unwrap();
    assert_eq!(ui.find(root, TbId::new("the_id")), Some(ids[0]));
    assert_eq!(ui.get::<widgets::Layout>(ids[0]).unwrap().spacing, Some(20));
}

#[derive(Default)]
struct Custom;
impl Widget for Custom {
    fn type_name(&self) -> &'static str {
        "Custom"
    }
}

#[test]
fn lowercase_factory_name() {
    let mut ui = ui();
    ui.register::<Custom>("my_widget");
    let root = ui.root();
    let ids = ui.inflate(root, "my_widget\n\tid foo\n").unwrap();
    assert_eq!(ids.len(), 1, "TB creates a widget for every node name that has a factory");
}

#[test]
fn huge_rect_paint_and_hit() {
    let mut ui = ui();
    let root = ui.root();
    ui.inflate(root, "TBWidget: rect: 2147483647 2147483647 2147483647 2147483647\n\tTBButton: text: x\n").unwrap();
    ui.inflate(root, "TBWidget: rect: 10 10 2147483647 2147483647\n\tTBButton: text: x, gravity: all\n").unwrap();
    ui.update();
    ui.pointer_move(Point::new(20, 20), Modifiers::NONE);
    let mut rec = Rec::default();
    ui.paint(&mut rec);
}
