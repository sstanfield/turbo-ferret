//! Core regressions found in review (2026-11): busy widgets, the message batch, animations started during the pass, removal, overflow.
#![allow(clippy::type_complexity)]

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::anim::{AnimSpec, Animate, Curve, OnDone};
use turbo_ferret::*;

type Log = Rc<RefCell<Vec<String>>>;

fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn ui() -> Ui {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(400, 300));
    ui
}

/// A widget whose hooks are closures.
#[derive(Default)]
struct W {
    name: &'static str,
    log: Option<Log>,
    ev: Option<Box<dyn FnMut(&mut Ctx, &Event) -> bool>>,
    msg: Option<Box<dyn FnMut(&mut Ctx, &Message)>>,
    removed: Option<Box<dyn FnMut(&mut Ctx)>>,
    child_removed: Option<Box<dyn FnMut(&mut Ctx, WidgetId)>>,
    die: Option<Box<dyn FnMut(&mut Ctx) -> DieAction>>,
    focus: Option<Box<dyn FnMut(&mut Ctx, bool)>>,
    capture: Option<Box<dyn FnMut(&mut Ctx, bool)>>,
    process: Option<Box<dyn FnMut(&mut Ctx)>>,
    arrange: Option<Box<dyn FnMut(&mut Ctx)>>,
}

impl W {
    fn new(name: &'static str, log: &Log) -> W {
        W { name, log: Some(log.clone()), ..Default::default() }
    }
    fn rec(&self, s: impl Into<String>) {
        if let Some(l) = &self.log {
            l.borrow_mut().push(format!("{}:{}", self.name, s.into()));
        }
    }
}

impl Widget for W {
    fn type_name(&self) -> &'static str {
        "W"
    }
    fn on_removed(&mut self, cx: &mut Ctx) {
        self.rec("removed");
        if let Some(f) = &mut self.removed {
            f(cx)
        }
    }
    fn on_child_removed(&mut self, cx: &mut Ctx, c: WidgetId) {
        self.rec("child_removed");
        if let Some(f) = &mut self.child_removed {
            f(cx, c)
        }
    }
    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        self.rec("die");
        match &mut self.die {
            Some(f) => f(cx),
            None => DieAction::Remove,
        }
    }
    fn on_focus_changed(&mut self, cx: &mut Ctx, f: bool) {
        self.rec(format!("focus={f}"));
        if let Some(h) = &mut self.focus {
            h(cx, f)
        }
    }
    fn on_capture_changed(&mut self, cx: &mut Ctx, c: bool) {
        self.rec(format!("capture={c}"));
        if let Some(h) = &mut self.capture {
            h(cx, c)
        }
    }
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        self.rec(format!("{:?}", ev.kind));
        match &mut self.ev {
            Some(f) => f(cx, ev),
            None => false,
        }
    }
    fn on_message(&mut self, cx: &mut Ctx, m: &Message) {
        self.rec(format!("msg {}", m.data.as_int()));
        if let Some(f) = &mut self.msg {
            f(cx, m)
        }
    }
    fn on_process(&mut self, cx: &mut Ctx) {
        if let Some(f) = &mut self.process {
            f(cx)
        }
    }
    fn arrange(&mut self, cx: &mut Ctx, old: Size, new: Size) {
        if let Some(f) = &mut self.arrange {
            f(cx)
        }
        cx.default_arrange(old, new)
    }
}

fn entries(l: &Log) -> Vec<String> {
    l.borrow().clone()
}
fn take(l: &Log) -> Vec<String> {
    std::mem::take(&mut *l.borrow_mut())
}

// ---------------------------------------------------------------- messages

/// A handler cancels another message that is due in the same update.
#[test]
fn cancel_in_same_batch() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let b = ui.add(root, W::new("b", &l));
    let a = ui.add(
        root,
        W {
            msg: Some(Box::new(move |cx, _| {
                cx.cancel_messages(b, None);
                assert!(!cx.has_message(b, tbid!("m")), "has_message says it is gone");
            })),
            ..W::new("a", &l)
        },
    );
    ui.set_time(0);
    ui.post_message_delayed(a, tbid!("m"), Value::Int(1), 10);
    ui.post_message_delayed(b, tbid!("m"), Value::Int(2), 20);
    ui.set_time(100);
    ui.update();
    assert_eq!(entries(&l), ["a:msg 1"], "b's message was cancelled before it was delivered");
}

/// `has_message` guard: a timer chain doubles when the guard runs while the
/// timer message is taken out for delivery.
#[test]
fn has_message_in_same_batch() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    // b: a self-reposting timer (like the spinner or the caret blink).
    let b = ui.add(
        root,
        W {
            msg: Some(Box::new(move |cx, _| {
                let me = cx.this();
                cx.post_message_delayed(me, tbid!("tick"), Value::Int(7), 100);
            })),
            ..W::new("b", &l)
        },
    );
    // a: "start the timer if it is not running" (ProgressSpinner::set_value).
    let a = ui.add(
        root,
        W {
            msg: Some(Box::new(move |cx, _| {
                if !cx.has_message(b, tbid!("tick")) {
                    cx.post_message_delayed(b, tbid!("tick"), Value::Int(7), 100);
                }
            })),
            ..W::new("a", &l)
        },
    );
    ui.set_time(0);
    ui.post_message_delayed(b, tbid!("tick"), Value::Int(7), 100);
    ui.post_message_delayed(a, tbid!("go"), Value::Int(1), 90);
    ui.set_time(100);
    ui.update();
    take(&l);
    ui.set_time(200);
    ui.update();
    let ticks = entries(&l).iter().filter(|e| e.starts_with("b:")).count();
    assert_eq!(ticks, 1, "one timer chain, got {:?}", entries(&l));
}

/// The same with a built-in widget: a spinner stopped from a message handler in the
/// update in which its own timer message is due keeps running its timer forever.
#[test]
fn spinner_stopped_from_a_message_handler_keeps_ticking() {
    use turbo_ferret::widgets::ProgressSpinner;
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let sp = ui.add(root, ProgressSpinner::default());
    // "Loading finished": stop the spinner.
    let app = ui.add(root, W { msg: Some(Box::new(move |cx, _| cx.set_value(sp, 0))), ..W::new("app", &l) });
    ui.set_time(0);
    ui.set_value(sp, 1);
    ui.post_message_delayed(app, tbid!("done"), Value::Null, 20);
    ui.set_time(40); // one frame: both the app's message (20) and the spin tick (33) are due
    ui.update();
    assert!(!ui.get::<ProgressSpinner>(sp).unwrap().is_running());
    assert_eq!(ui.next_wake_time(), None, "a stopped spinner has no timer");
}

// ---------------------------------------------------------------- tree

#[test]
fn add_to_removed_parent_leaks() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let p = ui.add(root, W::new("p", &l));
    ui.remove(p);
    let before = ui.node_count();
    let c = ui.add(p, W::new("c", &l));
    assert!(!ui.exists(c) || ui.parent(c).is_some(), "orphan: exists={} parent={:?}", ui.exists(c), ui.parent(c));
    assert_eq!(ui.node_count(), before);
}

#[test]
fn die_root() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, W::new("a", &l));
    ui.set_rect(a, Rect::new(0, 0, 100, 100));
    ui.set_focusable(a, true);
    ui.die(root);
    assert!(ui.exists(root));
    assert!(!ui.is_dying(a), "root can not die, so nothing should be dying");
    assert!(ui.pointer_down(Point::new(10, 10), 1, Modifiers::NONE));
}

#[test]
fn descendants_get_on_removed() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let p = ui.add(root, W::new("p", &l));
    let _c = ui.add(p, W::new("c", &l));
    take(&l);
    ui.remove(p);
    assert!(entries(&l).contains(&"c:removed".to_string()), "{:?}", entries(&l));
}

#[test]
fn extreme_pointer_coordinates() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, W::new("a", &l));
    ui.set_rect(a, Rect::new(10, 10, 100, 100));
    ui.pointer_move(Point::new(i32::MIN, i32::MIN), Modifiers::NONE);
    ui.pointer_move(Point::new(i32::MAX, i32::MAX), Modifiers::NONE);
}

// ---------------------------------------------------------------- animations

struct Cb(Box<dyn FnMut(&mut Ui, f32)>);
impl Animate for Cb {
    fn update(&mut self, ui: &mut Ui, p: f32) {
        (self.0)(ui, p)
    }
}

/// `animate` promises to abort a running animation of the same property; from a
/// callback that runs during `update` it does not.
#[test]
fn animate_during_update_replaces() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, W::new("a", &l));
    ui.set_time(0);
    let r1 = Rect::new(0, 0, 50, 50);
    let r2 = Rect::new(200, 200, 20, 20);
    ui.animate(AnimSpec::rect(a, Rect::new(0, 0, 10, 10), r1).duration(1000));
    let mut done = false;
    ui.animate_custom(
        Box::new(Cb(Box::new(move |ui, _| {
            if !done {
                done = true;
                assert!(ui.is_animating(a), "is_animating during update");
                ui.animate(AnimSpec::rect(a, Rect::new(0, 0, 10, 10), r2).duration(100));
            }
        }))),
        Curve::Linear,
        50,
    );
    for t in (16..3000).step_by(16) {
        ui.set_time(t);
        ui.update();
    }
    assert_eq!(ui.rect(a), r2, "the last animate() wins");
}

#[test]
fn abort_animations_for_during_update() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, W::new("a", &l));
    ui.set_time(0);
    ui.animate(AnimSpec::rect(a, Rect::new(0, 0, 10, 10), Rect::new(0, 0, 500, 500)).duration(1000));
    let mut done = false;
    ui.animate_custom(
        Box::new(Cb(Box::new(move |ui, _| {
            if !done {
                done = true;
                ui.abort_animations_for(a);
            }
        }))),
        Curve::Linear,
        50,
    );
    ui.set_time(16);
    ui.update();
    let r = ui.rect(a);
    assert!(!ui.is_animating(a), "aborted");
    ui.set_time(500);
    ui.update();
    assert_eq!(ui.rect(a), r, "an aborted animation stops where it is");
}

/// As `animate_during_update_replaces`, without the `is_animating` check.
#[test]
fn animate_during_update_last_wins() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, W::new("a", &l));
    ui.set_time(0);
    let r1 = Rect::new(0, 0, 50, 50);
    let r2 = Rect::new(200, 200, 20, 20);
    ui.animate(AnimSpec::rect(a, Rect::new(0, 0, 10, 10), r1).duration(1000));
    let mut done = false;
    ui.animate_custom(
        Box::new(Cb(Box::new(move |ui, _| {
            if !done {
                done = true;
                ui.animate(AnimSpec::rect(a, Rect::new(0, 0, 10, 10), r2).duration(100));
            }
        }))),
        Curve::Linear,
        50,
    );
    for t in (16..3000).step_by(16) {
        ui.set_time(t);
        ui.update();
    }
    assert_eq!(ui.rect(a), r2, "the last animate() wins");
}

/// A fade-out with OnDone::Remove is replaced by a fade-in during update: the
/// widget must survive.
#[test]
fn replaced_remove_animation_still_removes() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, W::new("a", &l));
    ui.set_time(0);
    ui.animate(AnimSpec::opacity(a, 1.0, 0.001).duration(300).on_done(OnDone::Remove));
    let mut done = false;
    ui.animate_custom(
        Box::new(Cb(Box::new(move |ui, _| {
            if !done {
                done = true;
                ui.animate(AnimSpec::opacity(a, 0.001, 1.0).duration(100)); // show it again
            }
        }))),
        Curve::Linear,
        50,
    );
    for t in (16..1000).step_by(16) {
        ui.set_time(t);
        ui.update();
    }
    assert!(ui.exists(a), "the fade-out was replaced, so nothing removes the widget");
}

// ---------------------------------------------------------------- busy: layout

/// Fixed preferred size; children follow gravity.
struct Fixed(i32, i32, Option<Box<dyn FnMut(&mut Ctx, &Event) -> bool>>);
impl Widget for Fixed {
    fn type_name(&self) -> &'static str {
        "Fixed"
    }
    fn measure_content(&mut self, _cx: &mut Ctx, _sc: SizeConstraints) -> PreferredSize {
        PreferredSize::fixed(self.0, self.1)
    }
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        match &mut self.2 {
            Some(f) => f(cx, ev),
            None => false,
        }
    }
}

/// A widget changes (its layout is invalidated) and asks its parent to fit, from
/// its own hook: the parent measures the busy child as 0x0 and keeps that.
#[test]
fn measuring_a_busy_child_poisons_the_parent() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let p = ui.add(root, W::new("p", &l));
    let c = ui.add(
        p,
        Fixed(
            50,
            20,
            Some(Box::new(move |cx, _| {
                let me = cx.this();
                cx.invalidate_layout(me); // what a text or content change does
                cx.resize_to_preferred(p);
                true
            })),
        ),
    );
    ui.resize_to_preferred(p);
    assert_eq!(ui.rect(p).size(), Size::new(50, 20));
    ui.send(c, EventKind::Click);
    ui.update();
    ui.update();
    assert_eq!(ui.preferred_size_free(p).pref, Size::new(50, 20), "parent's cached size");
    assert_eq!(ui.rect(p).size(), Size::new(50, 20), "parent's rect");
}

/// The same from the widget's own hook: "resize to fit" on itself after adding content.
#[test]
fn resize_self_to_preferred_from_own_hook() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let w = ui.add(
        root,
        W {
            ev: Some(Box::new(move |cx, _| {
                let me = cx.this();
                let c = cx.add(me, Fixed(70, 30, None));
                cx.set_gravity(c, Gravity::ALL);
                cx.resize_to_preferred(me);
                true
            })),
            ..W::new("w", &l)
        },
    );
    ui.set_rect(w, Rect::new(0, 0, 10, 10));
    ui.update();
    ui.send(w, EventKind::Click);
    ui.update();
    assert_eq!(ui.rect(w).size(), Size::new(70, 30));
}

/// A widget that resizes itself from its own hook: its children never hear of it.
#[test]
fn set_rect_on_self_from_own_hook_arranges_children() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let w = ui.add(
        root,
        W {
            ev: Some(Box::new(move |cx, _| {
                let me = cx.this();
                cx.set_rect(me, Rect::new(0, 0, 200, 150));
                true
            })),
            ..W::new("w", &l)
        },
    );
    ui.set_rect(w, Rect::new(0, 0, 100, 100));
    let c = ui.add(w, W::new("c", &l));
    ui.set_gravity(c, Gravity::ALL);
    ui.set_rect(c, Rect::new(0, 0, 100, 100));
    ui.update();
    // From outside it works.
    ui.set_rect(w, Rect::new(0, 0, 120, 100));
    assert_eq!(ui.rect(c).size(), Size::new(120, 100));
    ui.send(w, EventKind::Click);
    ui.update();
    ui.update();
    assert_eq!(ui.rect(w).size(), Size::new(200, 150));
    assert_eq!(ui.rect(c).size(), Size::new(200, 150), "child with gravity ALL follows");
}

// ---------------------------------------------------------------- dispatch

struct Dest(WidgetId);
impl Widget for Dest {
    fn type_name(&self) -> &'static str {
        "Dest"
    }
    fn event_destination(&self, _this: NodeRef<'_>) -> Option<WidgetId> {
        Some(self.0)
    }
}

/// Nobody handles the event, but a stale destination makes it "handled".
#[test]
fn stale_event_destination_reports_handled() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let t = ui.add(root, W::new("t", &l));
    let a = ui.add(root, Dest(t));
    ui.set_focusable(a, true);
    let b = ui.add(root, W::new("b", &l));
    ui.set_focusable(b, true);
    ui.set_focus(a);
    assert!(!ui.send(a, EventKind::Click), "not handled while the target lives");
    ui.remove(t);
    assert!(!ui.send(a, EventKind::Click), "not handled by a dead target either");
}

#[test]
fn stale_event_destination_swallows_tab() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let t = ui.add(root, W::new("t", &l));
    let a = ui.add(root, Dest(t));
    ui.set_focusable(a, true);
    let b = ui.add(root, W::new("b", &l));
    ui.set_focusable(b, true);
    ui.set_focus(a);
    ui.remove(t);
    ui.key(None, SpecialKey::Tab, Modifiers::NONE, true);
    assert_eq!(ui.focused(), Some(b), "Tab moved the focus");
}

// ---------------------------------------------------------------- overflow

#[test]
fn time_near_the_end() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, W::new("a", &l));
    ui.set_time(u64::MAX - 5);
    ui.post_message_delayed(a, tbid!("m"), Value::Null, 100);
}

#[test]
fn scroll_by_max() {
    use turbo_ferret::widgets::*;
    let mut ui = ui();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Off));
    let c = ui.add(ui.content_root(sc), W::default());
    ui.set_layout_params(c, Some(LayoutParams::fixed(100, 5000)));
    ui.set_rect(sc, Rect::new(10, 10, 100, 100));
    ui.update();
    ui.scroll_to(sc, Point::new(0, 50));
    ui.scroll_to(sc, Point::new(0, i32::MAX)); // fine: clamped
    ui.scroll_to(sc, Point::new(0, 50));
    ui.scroll_by(sc, Point::new(0, i32::MAX)); // "to the end"
}

#[test]
fn smooth_scroll_to_max() {
    use turbo_ferret::widgets::*;
    let mut ui = ui();
    let root = ui.root();
    let sc = ui.add(root, ScrollContainer::new(ScrollMode::Off));
    let c = ui.add(ui.content_root(sc), W::default());
    ui.set_layout_params(c, Some(LayoutParams::fixed(100, 5000)));
    ui.set_rect(sc, Rect::new(10, 10, 100, 100));
    ui.set_time(1000);
    ui.update();
    ui.scroll_to(sc, Point::new(0, 50));
    ui.scroll_to_smooth(sc, Point::new(0, i32::MAX));
    // A long frame (the app was in the background, a debugger stop ...).
    ui.set_time(9000);
    ui.update();
    assert_eq!(ui.scroll_info(sc).unwrap().pos.y, 4900);
}

/// Work left over by `update` is not announced.
#[test]
fn pending_layout_after_update_is_not_announced() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let p = ui.add(root, W::new("p", &l));
    let mut once = true;
    let c = ui.add(
        p,
        W {
            process: Some(Box::new(move |cx| {
                if std::mem::take(&mut once) {
                    let me = cx.this();
                    cx.set_layout_params(me, Some(LayoutParams::fixed(33, 44)));
                }
            })),
            ..W::new("c", &l)
        },
    );
    let _ = c;
    ui.update();
    ui.take_repaint();
    let pending = ui.has_flag(p, NodeFlags::NEEDS_ARRANGE);
    assert!(!pending || ui.next_wake_time().is_some(), "layout pending but next_wake_time is None");
}
