//! M4: basic widget behaviour (headless): click, toggle and radio groups, slider and
//! scroll bar values with `connection:`, window activation, close fade then free,
//! button auto repeat, click labels, progress spinner, animations.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::anim::{AnimSpec, Curve, OnDone};
use turbo_ferret::widgets::*;
use turbo_ferret::*;

fn ui() -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(800, 600));
    ui
}

fn click(ui: &mut Ui, id: WidgetId) {
    let r = ui.rect(id);
    let c = ui.to_root(id, Point::new(r.w / 2, r.h / 2));
    ui.pointer_down(c, 1, Modifiers::NONE);
    ui.pointer_up(c, Modifiers::NONE);
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

#[test]
fn button_click_and_toggle() {
    let mut ui = ui();
    let root = ui.root();
    let b = ui.add(root, Button::new("OK"));
    ui.set_rect(b, Rect::new(10, 10, 80, 30));
    ui.update();
    assert_eq!(ui.text(b), Some("OK"));
    let log = events(&mut ui, b);
    click(&mut ui, b);
    assert!(log.borrow().contains(&EventKind::Click));
    assert_eq!(ui.value(b), Value::Int(0), "plain buttons do not toggle");
    // The button gets focus by pointer; children are not hit.
    assert_eq!(ui.focused(), Some(b));
    let inner = ui.children(b)[0];
    assert_eq!(ui.widget_at(root, Point::new(50, 25)), Some(b));
    assert!(ui.get::<Layout>(inner).is_some());

    ui.with_widget::<Button, _>(b, |w, _| w.toggle_mode = true);
    log.borrow_mut().clear();
    click(&mut ui, b);
    assert_eq!(ui.value(b), Value::Int(1));
    assert!(ui.has_state(b, WidgetState::PRESSED));
    assert!(log.borrow().contains(&EventKind::Changed));
    click(&mut ui, b);
    assert_eq!(ui.value(b), Value::Int(0));
}

#[test]
fn button_click_by_keyboard() {
    let mut ui = ui();
    let root = ui.root();
    let b = ui.add(root, Button::new("OK"));
    let log = events(&mut ui, b);
    ui.set_focus(b);
    ui.key(None, SpecialKey::Enter, Modifiers::NONE, true);
    ui.key(None, SpecialKey::Enter, Modifiers::NONE, false);
    assert_eq!(*log.borrow(), [EventKind::Click]);
}

#[test]
fn button_auto_repeat() {
    let mut ui = ui();
    let root = ui.root();
    let b = ui.add(root, Button::new("+"));
    ui.with_widget::<Button, _>(b, |w, _| w.auto_repeat = true);
    ui.set_rect(b, Rect::new(0, 0, 40, 40));
    let log = events(&mut ui, b);
    ui.set_time(0);
    ui.pointer_down(Point::new(10, 10), 1, Modifiers::NONE);
    let clicks = |log: &Rc<RefCell<Vec<EventKind>>>| log.borrow().iter().filter(|k| **k == EventKind::Click).count();
    ui.set_time(499);
    ui.update();
    assert_eq!(clicks(&log), 0);
    ui.set_time(500);
    ui.update();
    assert_eq!(clicks(&log), 1);
    ui.set_time(600);
    ui.update();
    assert_eq!(clicks(&log), 2);
    ui.pointer_up(Point::new(10, 10), Modifiers::NONE);
    assert_eq!(clicks(&log), 3, "plus the normal click on release");
    ui.set_time(900);
    ui.update();
    assert_eq!(clicks(&log), 3, "repeat stops on release");
}

#[test]
fn checkbox_and_radio_group() {
    let mut ui = ui();
    let root = ui.root();
    let cb = ui.add(root, CheckBox::default());
    ui.set_rect(cb, Rect::new(0, 0, 20, 20));
    click(&mut ui, cb);
    assert_eq!(ui.value(cb), Value::Int(1));
    assert!(ui.has_state(cb, WidgetState::SELECTED));
    click(&mut ui, cb);
    assert_eq!(ui.value(cb), Value::Int(0));

    let group = ui.add(root, Layout::new(Axis::X));
    ui.set_group_root(group, true);
    ui.set_rect(group, Rect::new(0, 50, 300, 20));
    let radios: Vec<WidgetId> = (0..3).map(|_| ui.add(group, RadioButton::default())).collect();
    for (i, &r) in radios.iter().enumerate() {
        ui.set_group_id(r, tbid!("g"));
        ui.set_rect(r, Rect::new(i as i32 * 30, 0, 20, 20));
    }
    click(&mut ui, radios[0]);
    click(&mut ui, radios[2]);
    let vals: Vec<i32> = radios.iter().map(|&r| ui.value(r).as_int()).collect();
    assert_eq!(vals, [0, 0, 1]);
    click(&mut ui, radios[2]);
    assert_eq!(ui.value(radios[2]), Value::Int(1), "a selected radio stays selected");
    // Keyboard: space toggles a focused check box.
    ui.set_focus(cb);
    ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, true);
    ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, false);
    assert_eq!(ui.value(cb), Value::Int(1));
}

#[test]
fn click_label_forwards_to_checkbox() {
    let mut ui = ui();
    let root = ui.root();
    ui.inflate(root, "TBClickLabel\n\tid lbl\n\ttext Check me\n\tTBCheckBox\n\t\tid cb\n").unwrap();
    let lbl = ui.find(root, tbid!("lbl")).unwrap();
    let cb = ui.find(root, tbid!("cb")).unwrap();
    ui.update();
    assert_eq!(ui.text(lbl), Some("Check me"));
    // Click on the label's text, not on the check box.
    let tf = ui.find_as::<TextField>(lbl, TbId::NONE).unwrap();
    let r = ui.rect(tf);
    let p = ui.to_root(tf, Point::new(r.w / 2, r.h / 2));
    assert_ne!(ui.widget_at(root, p), Some(cb));
    ui.pointer_down(p, 1, Modifiers::NONE);
    ui.pointer_up(p, Modifiers::NONE);
    assert_eq!(ui.value(cb), Value::Int(1));
    assert_eq!(ui.focused(), Some(cb));
}

#[test]
fn slider_and_scrollbar_with_connection() {
    let mut ui = ui();
    let root = ui.root();
    ui.set_bound_value(tbid!("vol"), Value::Float(0.25));
    ui.inflate(
        root,
        "TBLayout\n\taxis y\n\tTBSlider\n\t\tid s\n\t\tmin 0\n\t\tmax 1\n\t\tconnection vol\n\
         \tTBScrollBar\n\t\tid b\n\t\tconnection vol\n",
    )
    .unwrap();
    ui.update();
    let s = ui.find(root, tbid!("s")).unwrap();
    let b = ui.find(root, tbid!("b")).unwrap();
    assert_eq!(ui.value(s), Value::Float(0.25), "connected to an existing value");
    assert_eq!(ui.value(b), Value::Float(0.25));
    ui.set_value(s, 0.75f32);
    assert_eq!(ui.value(b), Value::Float(0.75));
    assert_eq!(ui.bound_value(tbid!("vol")), Some(&Value::Float(0.75)));
    ui.set_value(s, 5.0f32);
    assert_eq!(ui.value(s), Value::Float(1.0), "clamped to max");
    assert_eq!(ui.value(b), Value::Float(1.0));

    // Keyboard on the slider: small step = 1/100 of the range.
    ui.set_focus(s);
    ui.key(None, SpecialKey::Left, Modifiers::NONE, true);
    assert!((ui.value(s).as_float() - 0.99).abs() < 1e-4);

    // Dragging the slider handle (a slider of its own, outside the layout).
    let s = ui.add(root, Slider::default());
    let h = ui.get::<Slider>(s).unwrap().handle().unwrap();
    // No skin in this test: give the handle a size.
    ui.set_layout_params(h, Some(LayoutParams::fixed(20, 20)));
    ui.set_rect(s, Rect::new(0, 0, 200, 20));
    ui.invalidate_layout(s);
    ui.update();
    let hr = ui.rect(h);
    let start = ui.to_root(h, Point::new(hr.w / 2, hr.h / 2));
    ui.pointer_down(start, 1, Modifiers::NONE);
    assert_eq!(ui.captured(), Some(h));
    let range = (200 - hr.w) as f32;
    ui.pointer_move(start + Point::new(range as i32 / 2, 0), Modifiers::NONE);
    ui.pointer_up(start + Point::new(range as i32 / 2, 0), Modifiers::NONE);
    assert!((ui.value(s).as_float() - 0.5).abs() < 0.02, "{:?}", ui.value(s));
}

#[test]
fn scrollbar_page_and_handle() {
    let mut ui = ui();
    let root = ui.root();
    let sb = ui.add(root, ScrollBar::default());
    ui.set_rect(sb, Rect::new(0, 0, 200, 16));
    ui.with_widget::<ScrollBar, _>(sb, |w, cx| w.set_limits(cx, 0.0, 300.0, 100.0));
    let h = ui.get::<ScrollBar>(sb).unwrap().handle().unwrap();
    // visible 100 of 400 total: 25% of 200px = 50px handle.
    assert_eq!(ui.rect(h).w, 50);
    // Click after the handle pages forward by `visible`.
    ui.pointer_down(Point::new(150, 8), 1, Modifiers::NONE);
    ui.pointer_up(Point::new(150, 8), Modifiers::NONE);
    assert_eq!(ui.value(sb), Value::Float(100.0));
    assert_eq!(ui.rect(h).x, 50);
}

#[test]
fn progress_spinner_runs_on_messages() {
    let mut ui = ui();
    let root = ui.root();
    let sp = ui.add(root, ProgressSpinner::default());
    ui.set_time(0);
    ui.set_value(sp, 1);
    assert!(ui.get::<ProgressSpinner>(sp).unwrap().is_running());
    assert_eq!(ui.next_wake_time(), Some(33));
    ui.set_value(sp, 0);
    assert_eq!(ui.next_wake_time(), None);
}

#[test]
fn window_activation_close_fades_then_frees() {
    let mut ui = ui();
    let root = ui.root();
    ui.set_time(0);
    let w = ui.add(root, Window::new("Hello"));
    ui.inflate(w, "TBButton\n\tid ok\n\ttext OK\n").unwrap();
    ui.resize_to_fit_content(w, ResizeFit::Preferred);
    ui.center_in_parent(w);
    assert!(ui.is_active_window(w));
    let ok = ui.find(w, tbid!("ok")).unwrap();
    ui.ensure_focus(w);
    assert_eq!(ui.focused(), Some(ok));
    // Fades in from almost zero.
    assert!(ui.opacity(w) < 0.01);
    ui.set_time(1000);
    ui.update();
    assert_eq!(ui.opacity(w), 1.0);
    assert_eq!(ui.next_wake_time(), None);

    // The close button closes it: dying, still there while fading, then freed.
    let close = ui.get::<Window>(w).unwrap().close_button().unwrap();
    ui.update();
    // (No skin here, so the close button has no size to click; send the click.)
    ui.send(close, EventKind::Click);
    assert!(ui.exists(w) && ui.is_dying(w));
    assert_eq!(ui.focused(), None, "focus left the dying window");
    assert_eq!(ui.widget_at(root, ui.to_root(ok, Point::new(2, 2))), None, "no input while dying");
    ui.set_time(1100);
    ui.update();
    assert!(ui.exists(w));
    assert!(ui.opacity(w) < 1.0);
    ui.set_time(1300);
    ui.update();
    assert!(!ui.exists(w), "removed when the fade completes");
    assert!(!ui.exists(ok));
}

#[test]
fn window_title_and_mover() {
    let mut ui = ui();
    let root = ui.root();
    let w = ui.add(root, Window::new("Title"));
    ui.with_widget::<Window, _>(w, |win, _| win.animate = false);
    ui.set_rect(w, Rect::new(100, 100, 200, 150));
    ui.update();
    assert_eq!(ui.text(w), Some("Title"));
    // Content starts below the title bar.
    let pr = ui.padding_rect(w);
    assert!(pr.y > 0, "{pr:?}");
    // Drag the title bar.
    let mover = ui.children(w)[0];
    let mr = ui.rect(mover);
    assert_eq!((mr.x, mr.y, mr.w), (0, 0, 200));
    let grab = ui.to_root(mover, Point::new(20, mr.h / 2));
    ui.pointer_down(grab, 1, Modifiers::NONE);
    assert_eq!(ui.captured(), Some(mover));
    ui.pointer_move(grab + Point::new(30, 40), Modifiers::NONE);
    ui.pointer_up(grab + Point::new(30, 40), Modifiers::NONE);
    assert_eq!(ui.rect(w).origin(), Point::new(130, 140));
}

#[test]
fn animations_curves_and_on_done() {
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Plain);
    ui.set_time(0);
    ui.animate(
        AnimSpec::rect(a, Rect::new(0, 0, 10, 10), Rect::new(100, 0, 10, 10)).curve(Curve::Linear).duration(100),
    );
    ui.set_time(50);
    ui.update();
    assert_eq!(ui.rect(a).x, 50);
    ui.set_time(100);
    ui.update();
    assert_eq!(ui.rect(a).x, 100);
    assert_eq!(ui.next_wake_time(), None);
    ui.animate(AnimSpec::opacity(a, 1.0, 0.0).duration(100).on_done(OnDone::Remove));
    ui.set_time(250);
    ui.update();
    assert!(!ui.exists(a));
    // Blocked animations finish on the next update.
    let b = ui.add(root, Plain);
    ui.block_animations(|ui| {
        ui.animate(AnimSpec::opacity(b, 0.0, 1.0).duration(1000));
    });
    ui.update();
    assert_eq!(ui.opacity(b), 1.0);
}
