//! M5: scrolling (headless): the scroll container and its scroll bar modes, panning,
//! kinetic and smooth scrolling driven by `update`, snapping, nested scrolling and
//! layout boundaries.

use std::cell::{Cell, RefCell};
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

fn fixed(ui: &mut Ui, id: WidgetId, w: i32, h: i32) {
    ui.set_layout_params(id, Some(LayoutParams::fixed(w, h)));
}

/// A 100x100 scroll container at (10, 10) holding one plain `content` widget.
fn container(ui: &mut Ui, mode: ScrollMode, content: Size) -> (WidgetId, WidgetId) {
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(mode));
    let c = ui.add(ui.content_root(sc), Plain);
    fixed(ui, c, content.w, content.h);
    ui.set_rect(sc, Rect::new(10, 10, 100, 100));
    ui.update();
    (sc, c)
}

fn pos(ui: &Ui, id: WidgetId) -> Point {
    ui.scroll_info(id).unwrap().pos
}

/// Advance time in 16 ms steps, updating, until nothing scrolls (or `max_ms` passed).
fn settle(ui: &mut Ui, max_ms: u64) -> u64 {
    let start = ui.now();
    while ui.has_running_scrollers() && ui.now() - start < max_ms {
        ui.set_time(ui.now() + 16);
        ui.update();
    }
    ui.now() - start
}

fn events(ui: &mut Ui, id: WidgetId) -> Rc<RefCell<Vec<EventKind>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    ui.on_event(id, move |_, ev| {
        l.borrow_mut().push(ev.kind);
        false
    });
    log
}

/// Press at `from` and drag by `step` every 10 ms, `n` times. Returns the last position.
fn drag(ui: &mut Ui, from: Point, step: Point, n: i32) -> Point {
    ui.pointer_down(from, 1, Modifiers::NONE);
    let mut p = from;
    for _ in 0..n {
        ui.set_time(ui.now() + 10);
        p += step;
        ui.pointer_move(p, Modifiers::NONE);
        ui.update();
    }
    p
}

#[test]
fn scroll_bar_visibility_solver() {
    let avail = Size::new(100, 100);
    let solve = |mode, w, h| ScrollBarVisibility::solve(mode, Size::new(w, h), avail, 10, 12);
    let v = solve(ScrollMode::XY, 10, 10);
    assert_eq!((v.x_on, v.y_on, v.visible), (true, true, Size::new(88, 90)));
    let v = solve(ScrollMode::Off, 500, 500);
    assert_eq!((v.x_on, v.y_on, v.visible), (false, false, avail));
    let v = solve(ScrollMode::Y, 500, 10);
    assert_eq!((v.x_on, v.y_on, v.visible), (false, true, Size::new(88, 100)));
    let v = solve(ScrollMode::YAuto, 500, 100);
    assert_eq!((v.x_on, v.y_on, v.visible), (false, false, avail));
    let v = solve(ScrollMode::YAuto, 50, 101);
    assert_eq!((v.x_on, v.y_on, v.visible), (false, true, Size::new(88, 100)));
    let v = solve(ScrollMode::Auto, 100, 100);
    assert_eq!((v.x_on, v.y_on), (false, false));
    // Too high: the Y bar takes width, which then makes X needed too.
    let v = solve(ScrollMode::Auto, 95, 200);
    assert_eq!((v.x_on, v.y_on, v.visible), (true, true, Size::new(88, 90)));
    // Too wide only: the X bar takes height, but the content still fits.
    let v = solve(ScrollMode::Auto, 200, 80);
    assert_eq!((v.x_on, v.y_on, v.visible), (true, false, Size::new(100, 90)));
    assert_eq!(ScrollMode::parse("y-auto"), Some(ScrollMode::YAuto));
    assert_eq!(ScrollMode::parse("nope"), None);
}

#[test]
fn scroll_container_lays_out_content_and_clamps() {
    let mut ui = ui();
    let (sc, c) = container(&mut ui, ScrollMode::Off, Size::new(300, 400));
    let w = ui.get::<ScrollContainer>(sc).unwrap();
    let (root, sbx, sby) = (w.root().unwrap(), w.scrollbar_x().unwrap(), w.scrollbar_y().unwrap());
    assert_eq!(ui.rect(root), Rect::new(0, 0, 100, 100));
    assert_eq!(ui.rect(c), Rect::new(0, 0, 300, 400));
    assert_eq!(ui.opacity(sbx), 0.0);
    assert_eq!(ui.opacity(sby), 0.0);
    let info = ui.scroll_info(sc).unwrap();
    assert_eq!((info.min, info.max, info.pos), (Point::ZERO, Point::new(200, 300), Point::ZERO));
    assert_eq!(ui.scroll_root(sc), root);

    ui.scroll_to(sc, Point::new(50, 1000));
    assert_eq!(pos(&ui, sc), Point::new(50, 300));
    ui.scroll_by(sc, Point::new(-500, -100));
    assert_eq!(pos(&ui, sc), Point::new(0, 200));
    // The content moved under the root: coordinates and hit testing follow.
    assert_eq!(ui.to_root(c, Point::ZERO), Point::new(10, 10 - 200));
    assert_eq!(ui.from_root(c, Point::new(10, 10)), Point::new(0, 200));
    assert_eq!(ui.widget_at(ui.root(), Point::new(20, 20)), Some(c));

    // Shrinking the content shrinks the limits and clamps the position.
    fixed(&mut ui, c, 300, 150);
    ui.update();
    let info = ui.scroll_info(sc).unwrap();
    assert_eq!((info.max, info.pos), (Point::new(200, 50), Point::new(0, 50)));
}

#[test]
fn scroll_bars_take_space_by_mode() {
    let mut ui = ui();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Auto));
    let w = ui.get::<ScrollContainer>(sc).unwrap();
    let (r, sbx, sby) = (w.root().unwrap(), w.scrollbar_x().unwrap(), w.scrollbar_y().unwrap());
    fixed(&mut ui, sbx, 20, 10);
    fixed(&mut ui, sby, 12, 20);
    let c = ui.add(r, Plain);
    fixed(&mut ui, c, 80, 300);
    ui.set_rect(sc, Rect::new(0, 0, 100, 100));
    ui.update();
    // Only Y is needed.
    assert_eq!((ui.opacity(sbx), ui.opacity(sby)), (0.0, 1.0));
    assert_eq!(ui.rect(r), Rect::new(0, 0, 88, 100));
    assert_eq!(ui.rect(sby), Rect::new(88, 0, 12, 100));
    assert_eq!(ui.padding_rect(sc), Rect::new(0, 0, 88, 100));
    assert_eq!(ui.scroll_info(sc).unwrap().max, Point::new(0, 200));

    // Always both.
    ui.with_widget::<ScrollContainer, _>(sc, |w, cx| w.set_scroll_mode(cx, ScrollMode::XY));
    ui.update();
    assert_eq!((ui.opacity(sbx), ui.opacity(sby)), (1.0, 1.0));
    assert_eq!(ui.rect(r), Rect::new(0, 0, 88, 90));
    assert_eq!(ui.rect(sbx), Rect::new(0, 90, 88, 10));
    assert_eq!(ui.scroll_info(sc).unwrap().max, Point::new(0, 210));

    // adapt-content: the content fills the root when it is smaller.
    fixed(&mut ui, c, 30, 40);
    ui.with_widget::<ScrollContainer, _>(sc, |w, cx| w.set_adapt_content_size(cx, true));
    ui.update();
    assert_eq!(ui.rect(c), Rect::new(0, 0, 88, 90));
    assert_eq!(ui.scroll_info(sc).unwrap().max, Point::ZERO);

    // Dragging the Y scroll bar's handle scrolls the content.
    fixed(&mut ui, c, 30, 490);
    ui.update();
    let handle = ui.get::<ScrollBar>(sby).unwrap().handle().unwrap();
    let hp = ui.to_root(handle, Point::new(5, 5));
    ui.pointer_down(hp, 1, Modifiers::NONE);
    ui.pointer_move(hp + Point::new(0, 20), Modifiers::NONE);
    ui.pointer_up(hp + Point::new(0, 20), Modifiers::NONE);
    let y = pos(&ui, sc).y;
    assert!(y > 50, "scrolled to {y}");
    assert_eq!(ui.to_root(c, Point::ZERO).y, -y);
}

#[test]
fn preferred_size_and_adapt_to_content() {
    let mut ui = ui();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Off));
    let c = ui.add(ui.content_root(sc), Plain);
    fixed(&mut ui, c, 300, 400);
    let ps = ui.preferred_size_free(sc);
    assert_eq!((ps.pref, ps.min), (Size::new(100, 100), Size::new(50, 50)));
    ui.with_widget::<ScrollContainer, _>(sc, |w, cx| w.set_adapt_to_content_size(cx, true));
    assert_eq!(ui.preferred_size_free(sc).pref, Size::new(300, 400));
}

#[test]
fn inflate_scroll_container() {
    let mut ui = ui();
    let root = ui.root();
    let ids = ui
        .inflate(
            root,
            "TBScrollContainer: scroll-mode: auto, adapt-content: 1, id: sc\n\tTBLayout: axis: y, id: content\n\t\tTBButton: text: A\n\t\tTBButton: text: B\n",
        )
        .unwrap();
    let sc = ids[0];
    let w = ui.get::<ScrollContainer>(sc).unwrap();
    assert_eq!(w.scroll_mode(), ScrollMode::Auto);
    assert!(w.adapt_content_size());
    assert!(!w.adapt_to_content_size());
    let content = ui.find(root, tbid!("content")).unwrap();
    assert_eq!(ui.parent(content), w.root());
    assert_eq!(ui.gravity(sc), Gravity::ALL);
}

#[test]
fn wheel_scrolls_and_reports_handled() {
    let mut ui = ui();
    let (sc, _c) = container(&mut ui, ScrollMode::Off, Size::new(100, 400));
    assert!(ui.wheel(Point::new(50, 50), Point::new(0, 1), Modifiers::NONE));
    assert_eq!(pos(&ui, sc), Point::new(0, 40));
    ui.wheel(Point::new(50, 50), Point::new(0, 100), Modifiers::NONE);
    assert_eq!(pos(&ui, sc), Point::new(0, 300));
    ui.wheel(Point::new(50, 50), Point::new(0, -3), Modifiers::NONE);
    assert_eq!(pos(&ui, sc), Point::new(0, 180));
    // With a modifier the wheel is left to others (zooming, for example).
    ui.wheel(Point::new(50, 50), Point::new(0, 1), Modifiers::CTRL);
    assert_eq!(pos(&ui, sc), Point::new(0, 180));
}

#[test]
fn keys_scroll_smoothly() {
    let mut ui = ui();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Off));
    let b = ui.add(ui.content_root(sc), Button::new("focus me"));
    fixed(&mut ui, b, 100, 1000);
    ui.set_rect(sc, Rect::new(0, 0, 100, 100));
    ui.update();
    assert!(ui.set_focus(b));
    let key = |ui: &mut Ui, k| ui.key(None, k, Modifiers::NONE, true);

    assert!(key(&mut ui, SpecialKey::Down));
    assert_eq!(pos(&ui, sc), Point::ZERO, "nothing moves until time passes");
    assert!(ui.is_scrolling(sc));
    assert_eq!(ui.next_wake_time(), Some(ui.now()));
    settle(&mut ui, 5000);
    assert_eq!(pos(&ui, sc).y, 40);
    assert_eq!(ui.next_wake_time(), None);

    // Two quick presses add up.
    key(&mut ui, SpecialKey::Down);
    ui.set_time(ui.now() + 16);
    ui.update();
    key(&mut ui, SpecialKey::Down);
    settle(&mut ui, 5000);
    assert!((pos(&ui, sc).y - 120).abs() <= 1, "got {}", pos(&ui, sc).y);

    key(&mut ui, SpecialKey::PageDown);
    settle(&mut ui, 5000);
    assert!((pos(&ui, sc).y - 220).abs() <= 2, "got {}", pos(&ui, sc).y);
    key(&mut ui, SpecialKey::End);
    settle(&mut ui, 5000);
    assert_eq!(pos(&ui, sc).y, 900);
    assert!(!key(&mut ui, SpecialKey::Down), "at the end the key is not handled");
    key(&mut ui, SpecialKey::Home);
    settle(&mut ui, 5000);
    assert_eq!(pos(&ui, sc).y, 0);
    assert!(!key(&mut ui, SpecialKey::Up));
    assert!(!key(&mut ui, SpecialKey::Left));
}

#[test]
fn smooth_scroll_reaches_its_target() {
    let mut ui = ui();
    let (sc, _c) = container(&mut ui, ScrollMode::Off, Size::new(1000, 1000));
    ui.scroll_to_smooth(sc, Point::new(300, 150));
    assert!(ui.is_scrolling(sc) && ui.is_scroller_started(sc));
    // It decelerates: more distance in the first 100 ms than in the next 100 ms.
    let mut last = Point::ZERO;
    let mut steps = Vec::new();
    for _ in 0..4 {
        ui.set_time(ui.now() + 100);
        ui.update();
        steps.push(pos(&ui, sc).x - last.x);
        last = pos(&ui, sc);
    }
    assert!(steps[0] > steps[1] && steps[1] > steps[2] && steps[2] > steps[3], "{steps:?}");
    let ms = settle(&mut ui, 10_000);
    assert!(ms < 3000, "took {ms} ms");
    let p = pos(&ui, sc);
    assert!((p.x - 300).abs() <= 1 && (p.y - 150).abs() <= 1, "{p:?}");
    assert!(!ui.is_scroller_started(sc));

    // Scrolling towards a limit stops there, at once (no waiting for the time).
    ui.scroll_by_smooth(sc, Point::new(0, 5000));
    let ms = settle(&mut ui, 10_000);
    assert_eq!(pos(&ui, sc).y, 900);
    assert!(ms < 1500, "stopped at the end after {ms} ms");
}

#[test]
fn pan_follows_the_pointer_then_flicks() {
    let mut ui = ui();
    let (sc, c) = container(&mut ui, ScrollMode::Off, Size::new(100, 2000));
    let log = events(&mut ui, c);
    // The first move is under the threshold (5 px): no pan yet.
    ui.pointer_down(Point::new(50, 90), 1, Modifiers::NONE);
    ui.pointer_move(Point::new(50, 87), Modifiers::NONE);
    assert_eq!(pos(&ui, sc), Point::ZERO);
    assert!(!ui.is_click_cancelled());
    ui.pointer_up(Point::new(50, 87), Modifiers::NONE);
    assert!(log.borrow().contains(&EventKind::Click));
    log.borrow_mut().clear();

    // Drag up by 10 px every 10 ms: the content follows, less the threshold.
    let end = drag(&mut ui, Point::new(50, 90), Point::new(0, -10), 6);
    assert_eq!(pos(&ui, sc), Point::new(0, 60 - 4));
    assert!(ui.is_click_cancelled());
    assert!(ui.is_scroller_started(sc) && !ui.is_scrolling(sc));
    // Released while moving: it keeps scrolling and slows down.
    ui.pointer_up(end, Modifiers::NONE);
    assert!(!log.borrow().contains(&EventKind::Click), "a pan is not a click");
    assert!(ui.is_scrolling(sc));
    assert_eq!(ui.next_wake_time(), Some(ui.now()));
    let released_at = pos(&ui, sc).y;
    let ms = settle(&mut ui, 10_000);
    let flicked = pos(&ui, sc).y - released_at;
    // 10 px per 10 ms is 1 px/ms; with a decay of 200 ms that is close to 200 px.
    assert!((150..=260).contains(&flicked), "flick distance {flicked}");
    assert!(ms > 300 && ms < 3000, "flick took {ms} ms");
    assert_eq!(ui.next_wake_time(), None);
    assert!(!ui.is_scroller_started(sc));
}

#[test]
fn slow_release_does_not_flick() {
    let mut ui = ui();
    let (sc, _c) = container(&mut ui, ScrollMode::Off, Size::new(100, 2000));
    let end = drag(&mut ui, Point::new(50, 90), Point::new(0, -10), 4);
    let y = pos(&ui, sc).y;
    assert_eq!(y, 36);
    // The finger rests before it lifts.
    ui.set_time(ui.now() + 100);
    ui.pointer_up(end, Modifiers::NONE);
    assert!(!ui.is_scrolling(sc) && !ui.is_scroller_started(sc));
    ui.set_time(ui.now() + 100);
    ui.update();
    assert_eq!(pos(&ui, sc).y, y);
}

#[test]
fn pan_when_the_scrolled_widget_itself_is_captured() {
    let mut ui = ui();
    // The content is only 50 high, so the lower half of the root is bare.
    let (sc, _c) = container(&mut ui, ScrollMode::Off, Size::new(600, 50));
    let root = ui.get::<ScrollContainer>(sc).unwrap().root().unwrap();
    assert_eq!(ui.widget_at(ui.root(), Point::new(60, 90)), Some(root));
    drag(&mut ui, Point::new(100, 90), Point::new(-10, 0), 5);
    assert_eq!(ui.captured(), Some(root));
    assert_eq!(pos(&ui, sc), Point::new(50 - 4, 0));
    // Back the other way: it follows without the threshold this time.
    for _ in 0..3 {
        let p = ui.to_root(root, ui.pointer_move_pos()) + Point::new(10, 0);
        ui.pointer_move(p, Modifiers::NONE);
    }
    assert_eq!(pos(&ui, sc), Point::new(16, 0));
    ui.pointer_cancel();
}

#[test]
fn touching_a_running_scroll_stops_it_without_a_click() {
    let mut ui = ui();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Off));
    let b = ui.add(ui.content_root(sc), Button::new("big"));
    fixed(&mut ui, b, 100, 3000);
    ui.set_rect(sc, Rect::new(0, 0, 100, 100));
    ui.update();
    let log = events(&mut ui, b);
    let end = drag(&mut ui, Point::new(50, 90), Point::new(0, -12), 6);
    ui.pointer_up(end, Modifiers::NONE);
    assert!(ui.is_scrolling(sc));
    ui.set_time(ui.now() + 32);
    ui.update();
    let y = pos(&ui, sc).y;

    log.borrow_mut().clear();
    ui.pointer_down(Point::new(50, 50), 1, Modifiers::NONE);
    assert!(!ui.is_scrolling(sc) && !ui.is_scroller_started(sc));
    ui.pointer_up(Point::new(50, 50), Modifiers::NONE);
    assert!(!log.borrow().contains(&EventKind::Click), "{:?}", log.borrow());
    ui.set_time(ui.now() + 100);
    ui.update();
    assert_eq!(pos(&ui, sc).y, y);
    // The next press is a normal click again.
    ui.pointer_down(Point::new(50, 50), 1, Modifiers::NONE);
    ui.pointer_up(Point::new(50, 50), Modifiers::NONE);
    assert!(log.borrow().contains(&EventKind::Click));
}

#[test]
fn quick_pans_gain_power() {
    let flick = |pans: i32| {
        let mut ui = ui();
        let (sc, _c) = container(&mut ui, ScrollMode::Off, Size::new(100, 100_000));
        let mut total = 0;
        for _ in 0..pans {
            let before = pos(&ui, sc).y;
            let end = drag(&mut ui, Point::new(50, 90), Point::new(0, -10), 6);
            ui.pointer_up(end, Modifiers::NONE);
            let released_at = pos(&ui, sc).y;
            assert!(released_at > before);
            // Let it run a little, then pan again while it still moves.
            for _ in 0..5 {
                ui.set_time(ui.now() + 16);
                ui.update();
            }
            total = pos(&ui, sc).y - released_at;
        }
        let at = pos(&ui, sc).y;
        settle(&mut ui, 20_000);
        total + pos(&ui, sc).y - at
    };
    let one = flick(1);
    let three = flick(3);
    assert!(three > one * 3 / 2, "one pan flicks {one}, the third of three flicks {three}");
}

#[test]
fn snap_chooses_where_scrolls_end() {
    let mut ui = ui();
    let (sc, _c) = container(&mut ui, ScrollMode::Off, Size::new(1000, 100));
    // Pages of 100 px, like TB's demo page window.
    ui.set_scroll_snap(
        sc,
        Some(Rc::new(|ui: &Ui, id, target: Point| {
            let page = ui.padding_rect(id).w;
            Point::new((target.x + page / 2) / page * page, target.y)
        })),
    );
    ui.scroll_by_smooth(sc, Point::new(130, 0));
    settle(&mut ui, 10_000);
    assert_eq!(pos(&ui, sc).x, 100);
    ui.scroll_by_smooth(sc, Point::new(170, 0));
    settle(&mut ui, 10_000);
    assert_eq!(pos(&ui, sc).x, 300);

    // A slow pan that ends between pages snaps to the nearest one on release.
    let end = drag(&mut ui, Point::new(90, 50), Point::new(-10, 0), 4);
    assert_eq!(pos(&ui, sc).x, 336);
    ui.set_time(ui.now() + 200);
    ui.pointer_up(end, Modifiers::NONE);
    assert!(ui.is_scrolling(sc));
    settle(&mut ui, 10_000);
    assert_eq!(pos(&ui, sc).x, 300);

    ui.set_scroll_snap(sc, None);
    ui.scroll_by_smooth(sc, Point::new(130, 0));
    settle(&mut ui, 10_000);
    assert!((pos(&ui, sc).x - 430).abs() <= 1);
}

#[test]
fn a_scroll_stops_when_something_else_moves_the_content() {
    let mut ui = ui();
    let (sc, _c) = container(&mut ui, ScrollMode::Off, Size::new(100, 5000));
    ui.scroll_by_smooth(sc, Point::new(0, 1000));
    ui.set_time(ui.now() + 32);
    ui.update();
    assert!(ui.is_scrolling(sc));
    ui.scroll_to(sc, Point::new(0, 3000));
    ui.set_time(ui.now() + 16);
    ui.update();
    assert!(!ui.is_scrolling(sc));
    assert_eq!(pos(&ui, sc).y, 3000);
    assert_eq!(ui.next_wake_time(), None);

    // `stop_scroller` stops it by hand.
    ui.scroll_by_smooth(sc, Point::new(0, 1000));
    ui.stop_scroller(sc);
    assert!(!ui.has_running_scrollers());
}

#[test]
fn nested_scrolling_continues_in_the_parent() {
    let mut ui = ui();
    let root = ui.root();
    let outer = ui.add(root, ScrollContainer::new(ScrollMode::Off));
    let inner = ui.add(ui.content_root(outer), ScrollContainer::new(ScrollMode::Off));
    fixed(&mut ui, inner, 100, 300);
    let c = ui.add(ui.content_root(inner), Plain);
    fixed(&mut ui, c, 100, 330);
    ui.set_rect(outer, Rect::new(0, 0, 100, 100));
    ui.update();
    assert_eq!(ui.scroll_info(inner).unwrap().max.y, 30);
    assert_eq!(ui.scroll_info(outer).unwrap().max.y, 200);
    // 50 px of wheel: the inner one takes what it can; the rest is not passed on
    // (the wheel is handled by the first widget that moves).
    ui.wheel(Point::new(50, 50), Point::new(0, 1), Modifiers::NONE);
    assert_eq!((pos(&ui, inner).y, pos(&ui, outer).y), (30, 0));
    // A pan uses up the inner range and then scrolls the outer one.
    ui.scroll_to(inner, Point::ZERO);
    drag(&mut ui, Point::new(50, 90), Point::new(0, -10), 6);
    assert_eq!((pos(&ui, inner).y, pos(&ui, outer).y), (30, 26));
    ui.pointer_cancel();
    assert_eq!(ui.scroll_by_recursive(inner, Point::new(7, 500)), Point::new(7, 326));
}

#[test]
fn removing_a_scrolling_widget_is_safe() {
    let mut ui = ui();
    let (sc, c) = container(&mut ui, ScrollMode::Off, Size::new(100, 5000));
    ui.scroll_by_smooth(sc, Point::new(0, 1000));
    assert!(ui.has_running_scrollers());
    ui.remove(sc);
    assert!(!ui.has_running_scrollers());
    assert_eq!(ui.next_wake_time(), None);
    ui.set_time(ui.now() + 16);
    ui.update();
    assert!(!ui.exists(c));
    // Stale ids are ignored.
    ui.scroll_by_smooth(sc, Point::new(0, 10));
    ui.stop_scroller(sc);
    assert!(!ui.is_scrolling(sc));

    // Removing the captured widget in the middle of a pan ends the pan.
    let (sc, c) = container(&mut ui, ScrollMode::Off, Size::new(100, 5000));
    drag(&mut ui, Point::new(50, 90), Point::new(0, -10), 3);
    assert!(ui.is_scroller_started(sc));
    ui.remove(c);
    assert!(!ui.is_scroller_started(sc));
    assert_eq!(ui.captured(), None);
}

#[test]
fn layout_overflow_pans_and_scrolls() {
    let mut ui = ui();
    let root = ui.root();
    let l = ui.add(root, Layout::new(Axis::X));
    for _ in 0..5 {
        let c = ui.add(l, Plain);
        fixed(&mut ui, c, 50, 20);
    }
    ui.set_rect(l, Rect::new(0, 0, 100, 20));
    ui.update();
    let overflow = ui.get::<Layout>(l).unwrap().overflow();
    assert!(overflow >= 150, "overflow {overflow}");
    assert_eq!(ui.scroll_info(l).unwrap().max, Point::new(overflow, 0));
    let end = drag(&mut ui, Point::new(90, 10), Point::new(-10, 0), 5);
    assert_eq!(ui.get::<Layout>(l).unwrap().overflow_scroll(), 46);
    ui.pointer_up(end, Modifiers::NONE);
    settle(&mut ui, 10_000);
    assert_eq!(ui.get::<Layout>(l).unwrap().overflow_scroll(), overflow, "the flick runs to the end");
    ui.scroll_to_smooth(l, Point::ZERO);
    settle(&mut ui, 10_000);
    assert_eq!(ui.get::<Layout>(l).unwrap().overflow_scroll(), 0);
}

/// Counts `arrange` calls.
struct Counter(Rc<Cell<u32>>);
impl Widget for Counter {
    fn type_name(&self) -> &'static str {
        "Counter"
    }
    fn arrange(&mut self, cx: &mut Ctx, old: Size, new: Size) {
        self.0.set(self.0.get() + 1);
        cx.default_arrange(old, new);
    }
}

#[test]
fn a_scroll_container_is_a_layout_boundary() {
    let mut ui = ui();
    let root = ui.root();
    let count = Rc::new(Cell::new(0));
    let outer = ui.add(root, Counter(count.clone()));
    let sc = ui.add(outer, ScrollContainer::new(ScrollMode::Off));
    let c = ui.add(ui.content_root(sc), Plain);
    fixed(&mut ui, c, 300, 400);
    ui.set_rect(outer, Rect::new(0, 0, 200, 200));
    ui.set_rect(sc, Rect::new(0, 0, 100, 100));
    ui.update();
    count.set(0);

    // The content changes size: the container re-lays out, its parent does not.
    fixed(&mut ui, c, 300, 800);
    ui.update();
    assert_eq!(count.get(), 0);
    assert_eq!(ui.scroll_info(sc).unwrap().max.y, 700);

    // When the container's own size follows the content, the parent has to know.
    ui.with_widget::<ScrollContainer, _>(sc, |w, cx| w.set_adapt_to_content_size(cx, true));
    ui.update();
    count.set(0);
    fixed(&mut ui, c, 300, 900);
    ui.update();
    assert_eq!(count.get(), 1);
}

#[test]
fn hooks_can_ask_about_their_own_scroll_state() {
    // While a widget's hook runs its behaviour is taken out; scroll info and child
    // translation must still be answered (from the value at hook entry).
    struct Probe(Rc<RefCell<Vec<(Option<Point>, Point)>>>, WidgetId);
    impl Widget for Probe {
        fn type_name(&self) -> &'static str {
            "Probe"
        }
        fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
            if ev.kind == EventKind::Custom(tbid!("probe")) {
                let me = cx.this();
                let own = cx.scroll_info(me).map(|i| i.pos);
                self.0.borrow_mut().push((own, cx.to_root(self.1, Point::ZERO)));
            }
            false
        }
        fn scroll_info(&self) -> Option<ScrollInfo> {
            Some(ScrollInfo { min: Point::ZERO, max: Point::new(0, 100), pos: Point::new(0, 30) })
        }
        fn child_translation(&self) -> Point {
            Point::new(0, -30)
        }
    }
    let mut ui = ui();
    let root = ui.root();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let child = ui.create(Plain);
    let p = ui.add(root, Probe(seen.clone(), child));
    ui.attach(p, child, Z::Top);
    ui.set_rect(p, Rect::new(5, 5, 50, 50));
    ui.set_rect(child, Rect::new(1, 2, 10, 10));
    assert_eq!(ui.to_root(child, Point::ZERO), Point::new(6, 7 - 30));
    ui.send(p, EventKind::Custom(tbid!("probe")));
    assert_eq!(*seen.borrow(), vec![(Some(Point::new(0, 30)), Point::new(6, 7 - 30))]);
}
