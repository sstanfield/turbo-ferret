//! M2: layout (stack layout settings, measure cache) and inflation.

use std::cell::Cell;
use std::rc::Rc;

use turbo_ferret::layout::stack::*;
use turbo_ferret::widgets::Layout;
use turbo_ferret::*;

fn ui() -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(800, 600));
    ui
}

/// A leaf with a fixed preferred size; counts measure calls.
#[derive(Default)]
struct Fixed {
    ps: PreferredSize,
    measured: Rc<Cell<u32>>,
}

impl Widget for Fixed {
    fn type_name(&self) -> &'static str {
        "Fixed"
    }
    fn measure_content(&mut self, _cx: &mut Ctx, _sc: SizeConstraints) -> PreferredSize {
        self.measured.set(self.measured.get() + 1);
        self.ps
    }
}

fn ps(min: (i32, i32), pref: (i32, i32), max: (i32, i32)) -> PreferredSize {
    PreferredSize {
        min: Size::new(min.0, min.1),
        pref: Size::new(pref.0, pref.1),
        max: Size::new(max.0, max.1),
        dependency: Default::default(),
    }
}

fn std_ps() -> PreferredSize {
    ps((20, 10), (50, 20), (100, 40))
}

/// A layout of `w`x`h` with three standard children; returns (ui, layout, children).
fn setup(w: i32, h: i32, f: impl FnOnce(&mut Layout)) -> (Ui, WidgetId, Vec<WidgetId>) {
    let mut ui = ui();
    let root = ui.root();
    let mut l = Layout::new(Axis::X);
    l.spacing = Some(10);
    f(&mut l);
    let lay = ui.add(root, l);
    let kids: Vec<WidgetId> = (0..3).map(|_| ui.add(lay, Fixed { ps: std_ps(), ..Default::default() })).collect();
    ui.set_rect(lay, Rect::new(0, 0, w, h));
    ui.update();
    (ui, lay, kids)
}

fn rects(ui: &Ui, ids: &[WidgetId]) -> Vec<(i32, i32, i32, i32)> {
    ids.iter()
        .map(|&i| {
            let r = ui.rect(i);
            (r.x, r.y, r.w, r.h)
        })
        .collect()
}

#[test]
fn preferred_distribution_centered() {
    let (ui, _, kids) = setup(300, 50, |_| {});
    assert_eq!(rects(&ui, &kids), [(65, 15, 50, 20), (125, 15, 50, 20), (185, 15, 50, 20)]);
}

#[test]
fn distribution_available_grows_children() {
    let (ui, _, kids) = setup(300, 50, |l| l.distribution = LayoutDistribution::Available);
    assert_eq!(rects(&ui, &kids), [(0, 15, 93, 20), (103, 15, 93, 20), (206, 15, 94, 20)]);
}

#[test]
fn distribution_gravity_grows_only_stretchy_children() {
    let (mut ui, lay, kids) = setup(300, 50, |l| l.distribution = LayoutDistribution::Gravity);
    ui.set_gravity(kids[1], Gravity::LEFT_RIGHT | Gravity::TOP);
    ui.update();
    let _ = lay;
    // Only the middle child may grow: extra 130 capped by its max (100 - 50 = 50).
    // used = 170 + 50 = 220, offset (300 - 220) / 2 = 40.
    assert_eq!(rects(&ui, &kids), [(40, 15, 50, 20), (100, 15, 100, 20), (210, 15, 50, 20)]);
}

#[test]
fn missing_space_shrinks_towards_min() {
    let (ui, _, kids) = setup(100, 50, |_| {});
    assert_eq!(rects(&ui, &kids), [(0, 15, 27, 20), (37, 15, 27, 20), (74, 15, 26, 20)]);
}

#[test]
fn distribution_position_left_and_right() {
    let (ui, _, kids) = setup(300, 50, |l| l.distribution_position = LayoutDistributionPosition::LeftTop);
    assert_eq!(rects(&ui, &kids)[0].0, 0);
    let (ui, _, kids) = setup(300, 50, |l| l.distribution_position = LayoutDistributionPosition::RightBottom);
    assert_eq!(rects(&ui, &kids)[2].0, 250);
}

#[test]
fn cross_axis_position_and_size() {
    let (ui, _, kids) = setup(300, 50, |l| l.position = LayoutPosition::LeftTop);
    assert_eq!(rects(&ui, &kids)[0], (65, 0, 50, 20));
    let (ui, _, kids) = setup(300, 50, |l| l.position = LayoutPosition::RightBottom);
    assert_eq!(rects(&ui, &kids)[0], (65, 30, 50, 20));
    let (ui, _, kids) = setup(300, 50, |l| l.size = LayoutSize::Available);
    assert_eq!(rects(&ui, &kids)[0], (65, 5, 50, 40), "up to max height");
    // Gravity size: a child with top+bottom gravity fills the height (capped by max).
    let (mut ui, _, kids) = setup(300, 30, |_| {});
    ui.set_gravity(kids[0], Gravity::LEFT | Gravity::TOP_BOTTOM);
    ui.update();
    assert_eq!(rects(&ui, &kids)[0], (65, 0, 50, 30));
    // Gravity position: bottom gravity sits at the bottom.
    let (mut ui, _, kids) = setup(300, 50, |l| l.position = LayoutPosition::Gravity);
    ui.set_gravity(kids[0], Gravity::LEFT | Gravity::BOTTOM);
    ui.update();
    assert_eq!(rects(&ui, &kids)[0], (65, 30, 50, 20));
    assert_eq!(rects(&ui, &kids)[1], (125, 0, 50, 20), "top gravity stays at the top");
}

#[test]
fn vertical_axis_and_reverse() {
    let (ui, _, kids) = setup(50, 300, |l| l.axis = Axis::Y);
    // Rotated: pref heights 20 along y, spacing 10: total 80, offset (300 - 80) / 2 = 110.
    // Cross axis: width = min(50, pref 50) = 50 (left gravity), centered at x 0.
    assert_eq!(rects(&ui, &kids), [(0, 110, 50, 20), (0, 140, 50, 20), (0, 170, 50, 20)]);
    let (ui, _, kids) = setup(300, 50, |l| l.reverse = true);
    assert_eq!(rects(&ui, &kids), [(185, 15, 50, 20), (125, 15, 50, 20), (65, 15, 50, 20)]);
}

#[test]
fn gone_children_take_no_space() {
    let (mut ui, _, kids) = setup(300, 50, |_| {});
    ui.set_visibility(kids[1], Visibility::Gone);
    ui.update();
    // total = 50 + 10 + 50 = 110, offset 95
    assert_eq!(rects(&ui, &[kids[0], kids[2]]), [(95, 15, 50, 20), (155, 15, 50, 20)]);
}

#[test]
fn overflow_and_scrolling() {
    let mut ui = ui();
    let root = ui.root();
    let mut l = Layout::new(Axis::X);
    l.spacing = Some(0);
    let lay = ui.add(root, l);
    let fixed = PreferredSize::fixed(100, 20);
    for _ in 0..3 {
        ui.add(lay, Fixed { ps: fixed, ..Default::default() });
    }
    ui.set_rect(lay, Rect::new(0, 0, 200, 20));
    ui.update();
    assert_eq!(ui.get::<Layout>(lay).unwrap().overflow(), 100);
    let info = ui.scroll_info(lay).unwrap();
    assert!(info.can_scroll_x() && info.max.x == 100);
    ui.scroll_to(lay, Point::new(70, 0));
    assert_eq!(ui.scroll_info(lay).unwrap().pos.x, 70);
    ui.scroll_to(lay, Point::new(500, 0));
    assert_eq!(ui.scroll_info(lay).unwrap().pos.x, 100, "clamped");
    // Child translation applies to hit testing.
    let kids = ui.children(lay).to_vec();
    assert_eq!(ui.widget_at(root, Point::new(150, 10)), Some(kids[2]));
    // Wheel scrolls back (positive y = down = forward).
    ui.wheel(Point::new(10, 10), Point::new(0, -1), Modifiers::NONE);
    assert_eq!(ui.scroll_info(lay).unwrap().pos.x, 100 - ui.config().pixels_per_line);
}

#[test]
fn measure_layout() {
    let mut ui = ui();
    let root = ui.root();
    let mut l = Layout::new(Axis::X);
    l.spacing = Some(10);
    let lay = ui.add(root, l);
    for _ in 0..3 {
        ui.add(lay, Fixed { ps: std_ps(), ..Default::default() });
    }
    let p = ui.preferred_size_free(lay);
    assert_eq!(p.pref, Size::new(170, 20));
    assert_eq!(p.min, Size::new(80, 10));
    assert_eq!(p.max, Size::new(320, 20), "TB: wanted height of non-filling children is their pref");
}

#[test]
fn measure_cache_hits_and_invalidation_up_the_chain() {
    let mut ui = ui();
    let root = ui.root();
    let lay = ui.add(root, Layout::new(Axis::X));
    let inner = ui.add(lay, Layout::new(Axis::Y));
    let count = Rc::new(Cell::new(0));
    let leaf = ui.add(inner, Fixed { ps: std_ps(), measured: count.clone() });
    ui.preferred_size_free(lay);
    assert_eq!(count.get(), 1);
    ui.preferred_size_free(lay);
    ui.preferred_size_free(inner);
    assert_eq!(count.get(), 1, "cached");
    ui.invalidate_layout(leaf);
    ui.preferred_size_free(lay);
    assert_eq!(count.get(), 2, "invalidation cleared the whole chain");
    // Layout params override the measured size.
    ui.set_layout_params(leaf, Some(LayoutParams::fixed(33, 44)));
    let p = ui.preferred_size_free(leaf);
    assert_eq!(p.pref, Size::new(33, 44));
    assert_eq!(ui.preferred_size_free(lay).pref, Size::new(33, 44));
}

#[test]
fn inflate_inline_layout_and_properties() {
    let mut ui = ui();
    let root = ui.root();
    ui.register::<Fixed>("Fixed");
    let src = "TBLayout\n\
               \tid main\n\
               \taxis y\n\
               \tspacing 4\n\
               \tposition left top\n\
               \tFixed\n\
               \t\tid a\n\
               \t\tlp: width: 30, height: 12\n\
               \tFixed\n\
               \t\tid b\n\
               \t\tlp: width: 40, height: 20\n\
               \t\tstate disabled\n\
               \t\topacity 0.5\n\
               \t\tgravity all\n";
    let made = ui.inflate(root, src).unwrap();
    assert_eq!(made.len(), 1);
    let main = ui.find(root, tbid!("main")).unwrap();
    assert_eq!(made[0], main);
    assert_eq!(ui.get::<Layout>(main).unwrap().axis, Axis::Y);
    assert_eq!(ui.gravity(main), Gravity::ALL, "layouts fill by default");
    let a = ui.find(root, tbid!("a")).unwrap();
    let b = ui.find(root, tbid!("b")).unwrap();
    assert!(ui.has_state(b, WidgetState::DISABLED));
    assert_eq!(ui.opacity(b), 0.5);
    // The layout was sized to its preferred size inside the root, then arranged.
    ui.update();
    assert_eq!(ui.rect(main), Rect::new(0, 0, 800, 600), "gravity all fills the parent");
    let ra = ui.rect(a);
    let rb = ui.rect(b);
    assert_eq!((ra.w, ra.h), (30, 12));
    assert_eq!((rb.w, rb.h), (40, 20));
    assert_eq!(ra.x, 0, "position left top");
    // spacing 4 along y; distribution position centered: total 12 + 4 + 20 = 36
    assert_eq!(ra.y, (600 - 36) / 2);
    assert_eq!(rb.y, ra.y + 12 + 4);
}

#[test]
fn inflate_autofocus_and_connection() {
    #[derive(Default)]
    struct Val(i32);
    impl Widget for Val {
        fn type_name(&self) -> &'static str {
            "Val"
        }
        fn value(&self) -> Value {
            Value::Int(self.0)
        }
        fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
            if self.0 != v.as_int() {
                self.0 = v.as_int();
                cx.emit_changed(Value::Int(self.0));
            }
        }
    }
    let mut ui = ui();
    let root = ui.root();
    ui.register_factory(Factory::of::<Val>("Val").sync(ValueKind::Int));
    ui.inflate(
        root,
        "Val\n\tid a\n\tvalue 5\n\tconnection v\nVal\n\tid b\n\tconnection v\n\tis-focusable 1\n\tautofocus 1\n",
    )
    .unwrap();
    let a = ui.find(root, tbid!("a")).unwrap();
    let b = ui.find(root, tbid!("b")).unwrap();
    assert_eq!(ui.bound_value(tbid!("v")), Some(&Value::Int(5)), "new value takes the first widget's value");
    assert_eq!(ui.value(b), Value::Int(5), "second widget synced on connect");
    assert_eq!(ui.focused(), Some(b));
    ui.set_value(b, 9);
    assert_eq!(ui.value(a), Value::Int(9));
}

#[test]
fn inflate_skips_unknown_nodes() {
    let mut ui = ui();
    let root = ui.root();
    let made = ui.inflate(root, "WindowInfo\n\ttitle Hello\nTBContainer\n\tid c\n").unwrap();
    assert_eq!(made.len(), 1);
    assert!(ui.find(root, tbid!("c")).is_some());
}

/// Stand-in for widgets that arrive at M4+: a text holder with a fixed size.
#[derive(Default)]
struct Stub {
    text: String,
}
impl Widget for Stub {
    fn type_name(&self) -> &'static str {
        "Stub"
    }
    fn measure_content(&mut self, _cx: &mut Ctx, _sc: SizeConstraints) -> PreferredSize {
        let w = 8 * self.text.chars().count() as i32 + 10;
        PreferredSize { min: Size::new(w, 20), pref: Size::new(w, 20), ..Default::default() }
    }
    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }
    fn set_text(&mut self, cx: &mut Ctx, t: &str) {
        self.text = t.to_string();
        let me = cx.this();
        cx.invalidate_layout(me);
    }
}

#[test]
fn inflate_every_demo_layout() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/demo01/ui_resources");
    let names = [
        "TBButton",
        "TBTextField",
        "TBEditField",
        "TBRadioButton",
        "TBClickLabel",
        "TBCheckBox",
        "TBSkinImage",
        "TBImageWidget",
        "TBSlider",
        "TBToggleContainer",
        "TBSelectList",
        "TBSection",
        "TBScrollContainer",
        "TBSelectDropdown",
        "TBScrollBar",
        "TBInlineSelect",
        "TBTabContainer",
        "TBProgressSpinner",
        "TBSeparator",
    ];
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".tb.txt") {
            continue;
        }
        let mut ui = Ui::new(Config::default(), Services::with_loader(FsLoader::new(dir)));
        ui.services_mut().log = None;
        for n in names {
            if ui.factory(n).is_none() {
                ui.register::<Stub>(n);
            }
        }
        ui.set_viewport(Size::new(800, 600));
        let root = ui.root();
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let made = ui.inflate_file(root, &file).unwrap_or_else(|e| panic!("{file}: {e}"));
        ui.update();
        assert!(!made.is_empty(), "{file}: nothing inflated");
        // Every widget got a sane rect; layout children do not overlap along the axis.
        check_tree(&ui, root, &file);
        count += 1;
    }
    assert!(count >= 20, "only {count} files");
}

fn check_tree(ui: &Ui, id: WidgetId, file: &str) {
    let r = ui.rect(id);
    assert!(r.w >= 0 && r.h >= 0, "{file}: negative size {r:?}");
    let kids: Vec<WidgetId> =
        ui.children(id).iter().copied().filter(|&c| ui.visibility(c) != Visibility::Gone).collect();
    if let Some(l) = ui.get::<Layout>(id) {
        for pair in kids.windows(2) {
            let (a, b) = (ui.rect(pair[0]), ui.rect(pair[1]));
            let ok = match (l.axis, l.reverse) {
                (Axis::X, false) => a.right() <= b.x,
                (Axis::Y, false) => a.bottom() <= b.y,
                (Axis::X, true) => b.right() <= a.x,
                (Axis::Y, true) => b.bottom() <= a.y,
            };
            assert!(ok, "{file}: overlapping layout children {a:?} {b:?}");
        }
    }
    for c in kids {
        check_tree(ui, c, file);
    }
}
