//! M5: tab container, toggle container and section behaviour (headless).

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::widgets::*;
use turbo_ferret::*;

fn ui() -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(800, 600));
    ui.set_time(1000);
    ui
}

fn click(ui: &mut Ui, id: WidgetId) {
    let r = ui.rect(id);
    let c = ui.to_root(id, Point::new(r.w / 2, r.h / 2));
    ui.pointer_down(c, 1, Modifiers::NONE);
    ui.pointer_up(c, Modifiers::NONE);
}

fn fixed(ui: &mut Ui, id: WidgetId, w: i32, h: i32) {
    ui.set_layout_params(id, Some(LayoutParams::fixed(w, h)));
}

const TABS: &str = r#"
TBTabContainer: id: tc
	tabs
		TBButton: text: "One"
			lp: width: 60, height: 20
		TBButton: text: "Two"
			lp: width: 60, height: 20
		TBButton: text: "Three"
			lp: width: 60, height: 20
	TBWidget: id: page1, gravity: all
	TBWidget: id: page2, gravity: all
	TBWidget: id: page3
		lp: width: 200, height: 100
"#;

struct Tabs {
    tc: WidgetId,
    tabs: Vec<WidgetId>,
    pages: Vec<WidgetId>,
    tab_layout: WidgetId,
    pages_root: WidgetId,
}

fn tabs(ui: &mut Ui, src: &str) -> Tabs {
    let root = ui.root();
    let tc = ui.inflate(root, src).unwrap()[0];
    ui.set_rect(tc, Rect::new(0, 0, 300, 200));
    ui.update();
    let w = ui.get::<TabContainer>(tc).unwrap();
    let (tab_layout, pages_root) = (w.tab_layout().unwrap(), w.pages_root().unwrap());
    Tabs { tc, tabs: ui.children(tab_layout).to_vec(), pages: ui.children(pages_root).to_vec(), tab_layout, pages_root }
}

fn shown(ui: &Ui, t: &Tabs) -> Vec<bool> {
    t.pages.iter().map(|&p| ui.visibility(p) == Visibility::Visible).collect()
}

fn pressed(ui: &Ui, t: &Tabs) -> Vec<i32> {
    t.tabs.iter().map(|&b| ui.value(b).as_int()).collect()
}

#[test]
fn tab_container_inflates_tabs_and_pages() {
    let mut ui = ui();
    let t = tabs(&mut ui, TABS);
    assert_eq!((t.tabs.len(), t.pages.len()), (3, 3));
    for &b in &t.tabs {
        assert!(ui.get::<Button>(b).is_some());
        let n = ui.node(b).unwrap();
        assert_eq!((n.id(), n.skin()), (tbid!("tab"), tbid!("TBTabContainer.tab")));
    }
    assert_eq!(ui.find(t.tc, tbid!("page2")), Some(t.pages[1]));
    assert_eq!(ui.content_root(t.tc), t.pages_root);
    assert_eq!(ui.node(t.pages_root).unwrap().skin(), tbid!("TBTabContainer.container"));
    // The first frame shows page 0 and presses its tab.
    assert_eq!(shown(&ui, &t), [true, false, false]);
    assert_eq!(pressed(&ui, &t), [1, 0, 0]);
    assert_eq!(ui.value(t.tc), Value::Int(0));
}

#[test]
fn tab_container_switches_pages() {
    let mut ui = ui();
    let t = tabs(&mut ui, TABS);
    // Top alignment: the tabs are above the pages, centered (as in TB).
    let (tl, pr) = (ui.rect(t.tab_layout), ui.rect(t.pages_root));
    assert!(tl.y < pr.y);
    assert_eq!(tl.x + tl.w / 2, pr.x + pr.w / 2);

    // Pressing a tab is enough (it does not wait for the release).
    let r = ui.rect(t.tabs[2]);
    let p = ui.to_root(t.tabs[2], Point::new(r.w / 2, r.h / 2));
    ui.pointer_down(p, 1, Modifiers::NONE);
    assert_eq!(shown(&ui, &t), [false, false, true]);
    assert_eq!(pressed(&ui, &t), [0, 0, 1]);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(ui.value(t.tc), Value::Int(2));
    // Hidden pages keep their place (invisible, not gone).
    assert_eq!(ui.visibility(t.pages[0]), Visibility::Invisible);

    ui.set_value(t.tc, 1);
    assert_eq!(shown(&ui, &t), [false, true, false]);
    assert_eq!(pressed(&ui, &t), [0, 1, 0]);
    let cur = ui.with_widget::<TabContainer, _>(t.tc, |w, cx| (w.current_page_widget(cx), w.num_pages(cx)));
    assert_eq!(cur, Some((Some(t.pages[1]), 3)));

    // Pages that fill (gravity all) are as large as the content area; others keep
    // their preferred size.
    ui.update();
    assert_eq!(ui.rect(t.pages_root).w, 300);
    assert_eq!(ui.rect(t.pages[1]), ui.padding_rect(t.pages_root));
    assert_eq!(ui.rect(t.pages[2]).size(), Size::new(200, 100));
}

#[test]
fn tab_container_alignment() {
    for (align, name) in
        [(Align::Left, "left"), (Align::Right, "right"), (Align::Bottom, "bottom"), (Align::Top, "top")]
    {
        let mut ui = ui();
        let src = TABS.replace("id: tc", &format!("id: tc, align: {name}"));
        let t = tabs(&mut ui, &src);
        let w = ui.get::<TabContainer>(t.tc).unwrap();
        assert_eq!(w.alignment(), align);
        let (tl, pr) = (ui.rect(t.tab_layout), ui.rect(t.pages_root));
        match align {
            Align::Left => assert!(tl.right() <= pr.x + 1 && tl.y > pr.y, "{tl:?} {pr:?}"),
            Align::Right => assert!(tl.x >= pr.right() - 1, "{tl:?} {pr:?}"),
            Align::Bottom => assert!(tl.y >= pr.bottom() - 1, "{tl:?} {pr:?}"),
            _ => assert!(tl.bottom() <= pr.y + 1, "{tl:?} {pr:?}"),
        }
        // Side tabs are stacked vertically.
        let (a, b) = (ui.rect(t.tabs[0]), ui.rect(t.tabs[1]));
        if matches!(align, Align::Left | Align::Right) {
            assert!(b.y > a.y && b.x == a.x);
            assert_eq!(ui.get::<TabContainer>(t.tc).unwrap().axis(), Axis::X);
        } else {
            assert!(b.x > a.x && b.y == a.y);
        }
    }
}

#[test]
fn tab_container_parts_take_properties() {
    let mut ui = ui();
    let src = r#"
TBTabContainer
	tabs: id: mytabs, spacing: 7
		TBButton: text: "A"
			lp: width: 40, height: 20
		TBButton: text: "B"
			lp: width: 40, height: 20
	content: id: mycontent
	root: id: myroot
	TBWidget
	TBWidget
"#;
    let t = tabs(&mut ui, src);
    assert_eq!(ui.node(t.tab_layout).unwrap().id(), tbid!("mytabs"));
    assert_eq!(ui.node(t.pages_root).unwrap().id(), tbid!("mycontent"));
    let root_layout = ui.get::<TabContainer>(t.tc).unwrap().root_layout().unwrap();
    assert_eq!(ui.node(root_layout).unwrap().id(), tbid!("myroot"));
    assert_eq!(ui.get::<Layout>(t.tab_layout).unwrap().spacing, Some(7));
    assert_eq!(ui.rect(t.tabs[1]).x - ui.rect(t.tabs[0]).right(), 7);
}

#[test]
fn many_tabs_do_not_widen_the_container() {
    let mut ui = ui();
    let mut src = String::from("TBTabContainer\n\ttabs\n");
    for i in 0..20 {
        src += &format!("\t\tTBButton: text: \"Tab {i}\"\n\t\t\tlp: width: 60, height: 20\n");
    }
    src += "\tTBWidget\n\t\tlp: width: 100, height: 50\n";
    let root = ui.root();
    let tc = ui.inflate(root, &src).unwrap()[0];
    let ps = ui.preferred_size_free(tc);
    assert!(ps.min.w <= 100, "min width {}", ps.min.w);
    // In a narrow container the tabs overflow and can be scrolled.
    ui.set_rect(tc, Rect::new(0, 0, 200, 100));
    ui.update();
    let tl = ui.get::<TabContainer>(tc).unwrap().tab_layout().unwrap();
    assert!(ui.get::<Layout>(tl).unwrap().overflow() > 0);
    assert!(ui.scroll_info(tl).unwrap().can_scroll_x());
}

#[test]
fn toggle_container_modes() {
    let mut ui = ui();
    let root = ui.root();
    let t = ui.add(root, ToggleContainer::new(Toggle::Expanded));
    // Off: collapsed and disabled (so Tab focus skips the children).
    assert_eq!(ui.visibility(t), Visibility::Gone);
    assert!(ui.has_state(t, WidgetState::DISABLED));
    ui.set_value(t, 1);
    assert_eq!(ui.visibility(t), Visibility::Visible);
    assert!(!ui.has_state(t, WidgetState::DISABLED));
    assert_eq!(ui.value(t), Value::Int(1));

    ui.with_widget::<ToggleContainer, _>(t, |w, cx| w.set_invert(cx, true));
    assert_eq!(ui.visibility(t), Visibility::Gone);
    assert!(!ui.get::<ToggleContainer>(t).unwrap().is_on());
    ui.set_value(t, 0);
    assert_eq!(ui.visibility(t), Visibility::Visible);

    let o = ui.add(root, ToggleContainer::new(Toggle::Opacity));
    assert_eq!(ui.opacity(o), 0.0);
    ui.set_value(o, 1);
    assert_eq!(ui.opacity(o), 1.0);

    let e = ui.add(root, ToggleContainer::new(Toggle::Enabled));
    assert!(ui.has_state(e, WidgetState::DISABLED));
    ui.set_value(e, 1);
    assert!(!ui.has_state(e, WidgetState::DISABLED));

    let n = ui.add(root, ToggleContainer::default());
    ui.set_value(n, 1);
    assert_eq!((ui.visibility(n), ui.opacity(n)), (Visibility::Visible, 1.0));
    assert_eq!(Toggle::parse("Expanded"), Some(Toggle::Expanded));
    assert_eq!(Toggle::parse("x"), None);
}

#[test]
fn toggle_container_follows_a_connected_check_box() {
    let mut ui = ui();
    let root = ui.root();
    let src = r#"
TBLayout: axis: y
	TBCheckBox: id: check, connection: show-more
	TBToggleContainer: id: more, toggle: expanded, connection: show-more
		TBButton: text: "More"
	TBToggleContainer: id: less, toggle: opacity, invert: 1, connection: show-more
		TBButton: text: "Less"
"#;
    ui.inflate(root, src).unwrap();
    ui.update();
    let (check, more, less) = (
        ui.find(root, tbid!("check")).unwrap(),
        ui.find(root, tbid!("more")).unwrap(),
        ui.find(root, tbid!("less")).unwrap(),
    );
    assert_eq!(ui.visibility(more), Visibility::Gone);
    assert_eq!(ui.opacity(less), 1.0);
    ui.set_value(check, 1);
    ui.update();
    assert_eq!(ui.visibility(more), Visibility::Visible);
    assert_eq!(ui.opacity(less), 0.0);
    assert!(!ui.rect(more).is_empty());
    ui.set_value(check, 0);
    assert_eq!(ui.visibility(more), Visibility::Gone);
}

#[test]
fn section_expands_and_collapses() {
    let mut ui = ui();
    let root = ui.root();
    let src = r#"
TBLayout: axis: y, id: col
	TBSection: id: sec, text: "Details"
		TBWidget: id: body
			lp: width: 120, height: 80
	TBWidget: id: below
		lp: width: 120, height: 10
"#;
    let col = ui.inflate(root, src).unwrap()[0];
    ui.resize_to_preferred(col);
    ui.update();
    let sec = ui.find(root, tbid!("sec")).unwrap();
    let body = ui.find(root, tbid!("body")).unwrap();
    let below = ui.find(root, tbid!("below")).unwrap();
    let s = ui.get::<Section>(sec).unwrap();
    let (header, container) = (s.header().unwrap(), s.container().unwrap());
    assert_eq!(ui.parent(body), Some(container));
    assert_eq!(ui.text(sec), Some("Details"));
    assert_eq!(ui.text(header), Some("Details"));
    assert_eq!(ui.node(header).unwrap().skin(), tbid!("TBSectionHeader"));
    assert!(ui.get::<SectionHeader>(header).is_some());

    // Collapsed by default: only the header takes space.
    assert_eq!(ui.value(sec), Value::Int(0));
    assert_eq!(ui.visibility(container), Visibility::Gone);
    let collapsed = ui.preferred_size_free(sec);
    assert_eq!(collapsed.max.h, collapsed.pref.h, "a section does not grow");
    let below_y = ui.rect(below).y;

    // Clicking the header expands it; what is below moves down.
    let changed = Rc::new(RefCell::new(Vec::new()));
    let c = changed.clone();
    ui.on_event(sec, move |_, ev| {
        if ev.kind == EventKind::Changed {
            c.borrow_mut().push(ev.value.clone());
        }
        false
    });
    click(&mut ui, header);
    assert_eq!(ui.value(sec), Value::Int(1));
    assert_eq!(ui.value(header), Value::Int(1));
    assert_eq!(ui.visibility(container), Visibility::Visible);
    assert!(ui.get::<Section>(sec).unwrap().is_expanded());
    ui.resize_to_preferred(col);
    ui.update();
    let expanded = ui.preferred_size_free(sec);
    assert!(expanded.pref.h >= collapsed.pref.h + 80, "{} vs {}", expanded.pref.h, collapsed.pref.h);
    assert!(ui.rect(below).y >= below_y + 80);
    assert!(!ui.rect(body).is_empty());

    click(&mut ui, header);
    assert_eq!(ui.value(sec), Value::Int(0));
    assert_eq!(ui.visibility(container), Visibility::Gone);

    // By value, as resources do (`value: 1`).
    ui.set_value(sec, 1);
    assert_eq!(ui.value(header), Value::Int(1));
    assert_eq!(ui.visibility(container), Visibility::Visible);
    ui.set_text(sec, "Renamed");
    assert_eq!(ui.text(header), Some("Renamed"));
}

#[test]
fn expanding_a_section_scrolls_it_into_view() {
    let mut ui = ui();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Off));
    ui.with_widget::<ScrollContainer, _>(sc, |w, cx| w.set_adapt_content_size(cx, true));
    let col = ui.add(ui.content_root(sc), Layout::new(Axis::Y));
    let spacer = ui.add(col, Plain);
    fixed(&mut ui, spacer, 100, 150);
    let sec = ui.add(col, Section::new("Far down"));
    let body = ui.add(ui.content_root(sec), Plain);
    fixed(&mut ui, body, 100, 120);
    let header = ui.get::<Section>(sec).unwrap().header().unwrap();
    fixed(&mut ui, header, 100, 20);
    ui.set_rect(sc, Rect::new(0, 0, 100, 200));
    ui.update();
    assert_eq!(ui.scroll_info(sc).unwrap().pos, Point::ZERO);
    assert!(ui.scroll_info(sc).unwrap().max.y == 0, "collapsed, everything fits");

    click(&mut ui, header);
    ui.update();
    // The content grew past the container; the section was scrolled into view.
    let info = ui.scroll_info(sc).unwrap();
    assert!(info.max.y > 0);
    let top = ui.to_root(sec, Point::ZERO).y;
    let bottom = top + ui.rect(sec).h;
    assert!(info.pos.y > 0 && top >= 0 && bottom <= 200, "pos {:?}, section {top}..{bottom}", info.pos);
}

#[test]
fn inflated_section_with_value() {
    let mut ui = ui();
    let root = ui.root();
    let sec = ui.inflate(root, "TBSection: value: 1, text: \"Open\"\n\tTBButton: text: \"Inside\"\n").unwrap()[0];
    let s = ui.get::<Section>(sec).unwrap();
    assert!(s.is_expanded());
    assert_eq!(ui.visibility(s.container().unwrap()), Visibility::Visible);
    assert_eq!(ui.value(s.header().unwrap()), Value::Int(1));
    // A lone header is a toggle button.
    let h = ui.inflate(root, "TBSectionHeader: text: \"Lone\"\n").unwrap()[0];
    ui.set_rect(h, Rect::new(0, 300, 100, 20));
    click(&mut ui, h);
    assert_eq!(ui.value(h), Value::Int(1));
}
