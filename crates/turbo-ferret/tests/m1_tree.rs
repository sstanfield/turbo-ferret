//! M1: tree, dispatch, input, focus, messages and value binding, headless, with stub
//! widgets (port-design.md section 20).

use std::cell::RefCell;
use std::rc::Rc;

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

/// Records hooks; optional behaviours driven by fields.
#[derive(Default)]
struct Probe {
    name: &'static str,
    log: Option<Log>,
    handle: Option<EventKind>,
    remove_self_on: Option<EventKind>,
    remove_on_process: Option<WidgetId>,
    keep_alive: bool,
    value: i32,
    reentry: Option<Rc<RefCell<Option<bool>>>>,
}

impl Probe {
    fn new(name: &'static str, log: &Log) -> Probe {
        Probe { name, log: Some(log.clone()), ..Default::default() }
    }
    fn rec(&self, s: impl Into<String>) {
        if let Some(l) = &self.log {
            l.borrow_mut().push(format!("{}:{}", self.name, s.into()));
        }
    }
}

impl Widget for Probe {
    fn type_name(&self) -> &'static str {
        "Probe"
    }
    fn on_added(&mut self, _cx: &mut Ctx) {
        self.rec("added");
    }
    fn on_removed(&mut self, _cx: &mut Ctx) {
        self.rec("removed");
    }
    fn on_child_added(&mut self, _cx: &mut Ctx, _c: WidgetId) {
        self.rec("child_added");
    }
    fn on_child_removed(&mut self, _cx: &mut Ctx, _c: WidgetId) {
        self.rec("child_removed");
    }
    fn on_die(&mut self, _cx: &mut Ctx) -> DieAction {
        self.rec("die");
        if self.keep_alive { DieAction::KeepAlive } else { DieAction::Remove }
    }
    fn on_focus_changed(&mut self, _cx: &mut Ctx, f: bool) {
        self.rec(format!("focus={f}"));
    }
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        self.rec(format!("{:?}", ev.kind));
        if let Some(r) = &self.reentry {
            let me = cx.this();
            let got = cx.with_widget::<Probe, ()>(me, |_, _| ()).is_some();
            *r.borrow_mut() = Some(got);
        }
        if self.remove_self_on == Some(ev.kind) {
            let me = cx.this();
            cx.remove(me);
            assert!(cx.node(me).is_none(), "detached immediately");
            return true;
        }
        self.handle == Some(ev.kind)
    }
    fn on_message(&mut self, _cx: &mut Ctx, msg: &Message) {
        self.rec(format!("msg {}", msg.data.as_int()));
    }
    fn on_process(&mut self, cx: &mut Ctx) {
        self.rec("process");
        if let Some(s) = self.remove_on_process.take() {
            cx.remove(s);
        }
    }
    fn value(&self) -> Value {
        Value::Int(self.value)
    }
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        let v = v.as_int();
        if v != self.value {
            self.value = v;
            self.rec(format!("set_value {v}"));
            cx.emit_changed(Value::Int(v));
        }
    }
}

fn entries(l: &Log) -> Vec<String> {
    l.borrow().clone()
}

fn take(l: &Log) -> Vec<String> {
    std::mem::take(&mut *l.borrow_mut())
}

#[test]
fn add_order_of_effects() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let p = ui.add(root, Probe::new("p", &l));
    take(&l);
    let c = ui.add(p, Probe::new("c", &l));
    assert_eq!(entries(&l), ["p:child_added", "c:added"]);
    assert_eq!(ui.parent(c), Some(p));
    assert_eq!(ui.children(p), &[c]);
}

#[test]
fn self_removal_inside_on_event() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let p = ui.add(root, Probe::new("p", &l));
    let c = ui.add(p, Probe { remove_self_on: Some(EventKind::Click), ..Probe::new("c", &l) });
    take(&l);
    let handled = ui.send(c, EventKind::Click);
    assert!(handled);
    assert!(ui.node(c).is_none());
    assert!(ui.children(p).is_empty());
    // The busy widget does not get its own on_removed (it knows); the parent does.
    assert_eq!(entries(&l), ["c:Click", "p:child_removed"]);
    // Freed after the top-level call returned.
    assert_eq!(ui.node_count(), 2);
}

#[test]
fn removing_a_sibling_during_a_walk_skips_it() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let b = ui.add(root, Probe::new("b", &l));
    ui.get_mut::<Probe>(a).unwrap().remove_on_process = Some(b);
    take(&l);
    ui.update();
    let e = entries(&l);
    assert!(e.contains(&"a:process".to_string()));
    assert!(!e.contains(&"b:process".to_string()), "{e:?}");
    assert!(e.contains(&"b:removed".to_string()));
    assert!(!ui.exists(b));
}

#[test]
fn busy_reentry_returns_none() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let flag = Rc::new(RefCell::new(None));
    let w = ui.add(root, Probe { reentry: Some(flag.clone()), ..Probe::new("w", &l) });
    ui.send(w, EventKind::Click);
    assert_eq!(*flag.borrow(), Some(false), "own behaviour is out during its hook");
    // Outside of hooks it is available.
    assert!(ui.with_widget::<Probe, _>(w, |_, _| ()).is_some());
    assert!(ui.get::<Probe>(w).is_some());
}

#[test]
fn stale_ids_after_flush() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let a1 = ui.add(a, Probe::new("a1", &l));
    let before = ui.node_count();
    ui.remove(a);
    assert_eq!(ui.node_count(), before - 2, "subtree freed at end of top-level call");
    assert!(ui.node(a).is_none() && ui.node(a1).is_none());
    let b = ui.add(root, Probe::new("b", &l));
    assert_ne!(a, b);
    assert!(ui.node(a).is_none(), "old id does not resolve to the reused slot");
    assert!(ui.get::<Probe>(a).is_none());
    ui.set_rect(a, Rect::new(0, 0, 10, 10)); // no-op, no panic
    ui.remove(a);
    assert!(!ui.send(a, EventKind::Click));
}

#[test]
fn removal_inside_hook_frees_after_top_level_returns() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let b = ui.add(root, Probe::new("b", &l));
    let count = ui.node_count();
    // Remove b from inside a's hook; b must be detached but not freed until the end.
    let seen = ui.with_widget::<Probe, _>(a, move |_, cx| {
        cx.remove(b);
        (cx.node(b).is_none(), cx.node_count())
    });
    assert_eq!(seen, Some((true, count)), "detached now, slot still allocated inside the hook");
    assert_eq!(ui.node_count(), count - 1);
}

#[test]
fn focus_moves_on_removal() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let b = ui.add(root, Probe::new("b", &l));
    ui.set_focusable(a, true);
    ui.set_focusable(b, true);
    assert!(ui.set_focus(a));
    assert_eq!(ui.focused(), Some(a));
    take(&l);
    ui.remove(a);
    assert_eq!(ui.focused(), Some(b));
    let e = entries(&l);
    assert!(e.contains(&"b:focus=true".to_string()), "{e:?}");
    // Removing the last focusable clears focus.
    ui.remove(b);
    assert_eq!(ui.focused(), None);
}

#[test]
fn focus_rules() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    assert!(!ui.set_focus(a), "not focusable");
    ui.set_focusable(a, true);
    ui.set_disabled(a, true);
    assert!(!ui.set_focus(a), "disabled");
    ui.set_disabled(a, false);
    ui.set_visibility(a, Visibility::Invisible);
    assert!(!ui.set_focus(a), "invisible");
    ui.set_visibility(a, Visibility::Visible);
    assert!(ui.set_focus(a));
}

#[test]
fn die_with_keep_alive() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe { keep_alive: true, ..Probe::new("a", &l) });
    take(&l);
    ui.die(a);
    assert!(ui.exists(a), "kept alive");
    assert!(ui.is_dying(a));
    assert_eq!(entries(&l), ["a:die"]);
    ui.die(a); // second die is a no-op
    assert_eq!(entries(&l), ["a:die"]);
    ui.remove(a);
    assert!(!ui.exists(a));

    let b = ui.add(root, Probe::new("b", &l));
    ui.die(b);
    assert!(!ui.exists(b));
}

#[test]
fn die_listener_can_keep_alive() {
    struct Keeper;
    impl UiListener for Keeper {
        fn on_dying(&mut self, _ui: &mut Ui, _id: WidgetId) -> bool {
            true
        }
    }
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let lid = ui.add_listener(Box::new(Keeper));
    ui.die(a);
    assert!(ui.exists(a));
    ui.remove_listener(lid);
}

/// Redirects events to a fixed destination.
struct Redirect(Option<WidgetId>, Log);
impl Widget for Redirect {
    fn type_name(&self) -> &'static str {
        "Redirect"
    }
    fn event_destination(&self, _this: NodeRef) -> Option<WidgetId> {
        self.0
    }
    fn on_event(&mut self, _cx: &mut Ctx, ev: &Event) -> bool {
        self.1.borrow_mut().push(format!("redirect:{:?}", ev.kind));
        false
    }
}

#[test]
fn bubbling_through_event_destination() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe { handle: Some(EventKind::Click), ..Probe::new("a", &l) });
    let b = ui.add(root, Probe::new("b", &l));
    let r = ui.add(b, Redirect(Some(a), l.clone()));
    take(&l);
    assert!(ui.send(r, EventKind::Click));
    assert_eq!(entries(&l), ["redirect:Click", "a:Click"], "skips its parent b");

    // Unhandled events bubble to the root and return false.
    let c = ui.add(b, Probe::new("c", &l));
    take(&l);
    assert!(!ui.send(c, EventKind::KeyDown));
    assert_eq!(entries(&l), ["c:KeyDown", "b:KeyDown"]);
}

#[test]
fn listeners_swallow_and_closures() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let hits = Rc::new(RefCell::new(0));
    let h = hits.clone();
    let lid = ui
        .on_event(a, move |_ui, ev| {
            *h.borrow_mut() += 1;
            ev.kind == EventKind::Click
        })
        .unwrap();
    take(&l);
    assert!(ui.send(a, EventKind::Click));
    assert!(entries(&l).is_empty(), "swallowed before the widget");
    assert!(!ui.send(a, EventKind::KeyDown));
    assert_eq!(entries(&l), ["a:KeyDown"]);
    assert_eq!(*hits.borrow(), 2);
    ui.remove_listener(lid);
    ui.send(a, EventKind::Click);
    assert_eq!(*hits.borrow(), 2);
}

#[test]
fn listener_removing_target_counts_as_handled() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    ui.on_event(a, move |ui, ev| {
        ui.remove(ev.target);
        false
    });
    assert!(ui.send(a, EventKind::Click));
    assert!(!ui.exists(a));
}

#[test]
fn message_order_and_cancellation_on_removal() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let b = ui.add(root, Probe::new("b", &l));
    ui.set_time(1000);
    ui.post_message_delayed(a, TbId::new("t"), Value::Int(3), 30);
    ui.post_message_delayed(a, TbId::new("t"), Value::Int(2), 20);
    ui.post_message(a, TbId::new("t"), Value::Int(1));
    ui.post_message(b, TbId::new("t"), Value::Int(9));
    assert_eq!(ui.next_wake_time(), Some(1000));
    take(&l);
    ui.update();
    let msgs: Vec<String> = entries(&l).into_iter().filter(|e| e.contains("msg")).collect();
    assert_eq!(msgs, ["a:msg 1", "b:msg 9"]);
    assert_eq!(ui.next_wake_time(), Some(1020));
    ui.post_message_delayed(b, TbId::new("t"), Value::Int(8), 5);
    ui.remove(b);
    assert!(!ui.has_message(b, TbId::new("t")));
    take(&l);
    ui.set_time(1031);
    ui.update();
    let msgs: Vec<String> = entries(&l).into_iter().filter(|e| e.contains("msg")).collect();
    assert_eq!(msgs, ["a:msg 2", "a:msg 3"], "delayed in fire order");
    assert_eq!(ui.next_wake_time(), None);
    ui.post_message_delayed(a, TbId::new("x"), Value::Null, 10);
    assert!(ui.has_message(a, TbId::new("x")));
    ui.cancel_messages(a, Some(TbId::new("x")));
    assert!(!ui.has_message(a, TbId::new("x")));
}

#[test]
fn messages_posted_during_delivery_wait() {
    struct Repost;
    impl Widget for Repost {
        fn type_name(&self) -> &'static str {
            "Repost"
        }
        fn on_message(&mut self, cx: &mut Ctx, m: &Message) {
            let me = cx.this();
            let n = m.data.as_int() + 1;
            cx.post_message(me, m.id, Value::Int(n));
        }
    }
    let mut ui = ui();
    let root = ui.root();
    let w = ui.add(root, Repost);
    ui.post_message(w, tbid!("tick"), Value::Int(0));
    ui.update(); // must terminate
    assert!(ui.has_message(w, tbid!("tick")));
}

#[test]
fn value_binding_without_read_back() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let b = ui.add(root, Probe::new("b", &l));
    let c = ui.add(root, Probe::new("c", &l));
    let name = tbid!("volume");
    ui.connect_value(a, name);
    ui.connect_value(b, name);
    ui.connect_value(c, name);
    take(&l);
    ui.set_value(a, 7);
    // a fired Changed while busy; b and c got the value from the event.
    assert_eq!(ui.value(b), Value::Int(7));
    assert_eq!(ui.value(c), Value::Int(7));
    assert_eq!(ui.bound_value(name), Some(&Value::Int(7)));
    let e = entries(&l);
    assert!(e.contains(&"b:set_value 7".to_string()) && e.contains(&"c:set_value 7".to_string()));
    assert_eq!(e.iter().filter(|s| s.contains("set_value")).count(), 3, "no loops: {e:?}");

    ui.set_bound_value(name, 3);
    assert_eq!((ui.value(a), ui.value(b), ui.value(c)), (Value::Int(3), Value::Int(3), Value::Int(3)));

    ui.remove(b);
    ui.set_value(a, 5);
    assert_eq!(ui.value(c), Value::Int(5));
}

/// A simple rect-filling widget for pointer tests.
#[derive(Default)]
struct Pad {
    log: Option<Log>,
    name: &'static str,
}
impl Widget for Pad {
    fn type_name(&self) -> &'static str {
        "Pad"
    }
    fn on_event(&mut self, _cx: &mut Ctx, ev: &Event) -> bool {
        if let Some(l) = &self.log {
            l.borrow_mut().push(format!("{}:{:?}@{},{}", self.name, ev.kind, ev.pos.x, ev.pos.y));
        }
        false
    }
}

fn pad(l: &Log, name: &'static str) -> Pad {
    Pad { log: Some(l.clone()), name }
}

#[test]
fn pointer_click_capture_and_hover() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, pad(&l, "a"));
    ui.set_rect(a, Rect::new(10, 10, 100, 50));
    let b = ui.add(a, pad(&l, "b"));
    ui.set_rect(b, Rect::new(5, 5, 20, 20));

    ui.pointer_move(Point::new(16, 16), Modifiers::NONE);
    assert_eq!(ui.hovered(), Some(b));
    assert!(ui.auto_state(b).contains(WidgetState::HOVERED));
    take(&l);

    ui.pointer_down(Point::new(16, 16), 1, Modifiers::NONE);
    assert_eq!(ui.captured(), Some(b));
    assert!(ui.auto_state(b).contains(WidgetState::PRESSED));
    ui.pointer_up(Point::new(17, 16), Modifiers::NONE);
    assert_eq!(ui.captured(), None);
    // Events bubble from b to a; positions stay in b's coordinates.
    assert_eq!(
        entries(&l),
        ["b:PointerDown@1,1", "a:PointerDown@1,1", "b:PointerUp@2,1", "a:PointerUp@2,1", "b:Click@2,1", "a:Click@2,1"]
    );

    // Release outside: no click.
    take(&l);
    ui.pointer_down(Point::new(16, 16), 1, Modifiers::NONE);
    ui.pointer_move(Point::new(200, 200), Modifiers::NONE);
    assert_eq!(ui.captured(), Some(b), "capture holds while moving outside");
    ui.pointer_up(Point::new(200, 200), Modifiers::NONE);
    assert!(!entries(&l).iter().any(|e| e.contains("Click")));
}

#[test]
fn hit_testing_skips_disabled_ignored_and_invisible() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, pad(&l, "a"));
    ui.set_rect(a, Rect::new(0, 0, 100, 100));
    let top = ui.add(root, pad(&l, "top"));
    ui.set_rect(top, Rect::new(0, 0, 100, 100));
    assert_eq!(ui.widget_at(root, Point::new(5, 5)), Some(top), "topmost wins");
    ui.set_ignore_input(top, true);
    assert_eq!(ui.widget_at(root, Point::new(5, 5)), Some(a));
    ui.set_ignore_input(top, false);
    ui.set_disabled(top, true);
    assert_eq!(ui.widget_at(root, Point::new(5, 5)), Some(a));
    ui.set_disabled(top, false);
    ui.set_visibility(top, Visibility::Gone);
    assert_eq!(ui.widget_at(root, Point::new(5, 5)), Some(a));
}

#[test]
fn click_by_key_and_tab_navigation() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Probe::new("a", &l));
    let b = ui.add(root, Probe::new("b", &l));
    let c = ui.add(root, Probe::new("c", &l));
    for w in [a, b, c] {
        ui.set_focusable(w, true);
    }
    ui.set_click_by_key(b, true);
    ui.set_focus(a);
    assert!(ui.key(None, SpecialKey::Tab, Modifiers::NONE, true));
    assert_eq!(ui.focused(), Some(b));
    assert!(ui.auto_state(b).contains(WidgetState::FOCUSED), "focus shown after Tab");
    take(&l);
    ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, true);
    assert!(ui.has_state(b, WidgetState::PRESSED));
    ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, false);
    assert!(!ui.has_state(b, WidgetState::PRESSED));
    assert_eq!(entries(&l), ["b:Click"]);
    ui.key(None, SpecialKey::Tab, Modifiers::NONE, true);
    assert_eq!(ui.focused(), Some(c));
    ui.key(None, SpecialKey::Tab, Modifiers::NONE, true);
    assert_eq!(ui.focused(), Some(a), "wraps around");
    ui.key(None, SpecialKey::Tab, Modifiers::SHIFT, true);
    assert_eq!(ui.focused(), Some(c), "shift+tab goes back");
    take(&l);
    assert!(!ui.key(Some('x'), SpecialKey::None, Modifiers::NONE, true));
    assert_eq!(entries(&l), ["c:KeyDown"]);
}

#[test]
fn windows_activate_and_restore_focus() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let w1 = ui.create(pad(&l, "w1"));
    ui.set_window(w1, true, true);
    ui.attach(root, w1, Z::Top);
    ui.set_rect(w1, Rect::new(0, 0, 100, 100));
    let f1 = ui.add(w1, pad(&l, "f1"));
    ui.set_rect(f1, Rect::new(0, 0, 50, 50));
    ui.set_focusable(f1, true);
    ui.set_focus(f1);
    assert!(ui.is_active_window(w1));
    assert!(ui.has_state(w1, WidgetState::SELECTED));
    assert_eq!(ui.focused(), Some(f1));

    let w2 = ui.create(pad(&l, "w2"));
    ui.set_window(w2, true, true);
    ui.attach(root, w2, Z::Top);
    ui.set_rect(w2, Rect::new(200, 0, 100, 100));
    let f2 = ui.add(w2, pad(&l, "f2"));
    ui.set_focusable(f2, true);
    ui.set_rect(f2, Rect::new(0, 0, 50, 50));
    assert!(ui.is_active_window(w2), "added on top activates");
    assert!(!ui.is_active_window(w1));
    ui.ensure_focus(w2);
    assert_eq!(ui.focused(), Some(f2));

    // Clicking in w1 activates it, brings it to the top and restores its focus.
    ui.pointer_down(Point::new(60, 60), 1, Modifiers::NONE);
    ui.pointer_up(Point::new(60, 60), Modifiers::NONE);
    assert!(ui.is_active_window(w1));
    assert_eq!(ui.children(root).last(), Some(&w1));
    assert_eq!(ui.focused(), Some(f1));

    // Focus requested in an inactive window is recorded, not applied.
    assert!(ui.set_focus(f2));
    assert_eq!(ui.focused(), Some(f1));
    ui.activate_window(w2);
    assert_eq!(ui.focused(), Some(f2));

    // Removing the active window activates the next one.
    ui.remove(w2);
    assert!(ui.is_active_window(w1));
    assert_eq!(ui.focused(), Some(f1));
}

#[test]
fn long_click_on_touch() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, pad(&l, "a"));
    ui.set_rect(a, Rect::new(0, 0, 100, 100));
    ui.set_want_long_click(a, true);
    ui.set_time(100);
    ui.touch_down(0, Point::new(10, 10), 1, Modifiers::NONE);
    assert_eq!(ui.next_wake_time(), Some(600));
    take(&l);
    ui.set_time(599);
    ui.update();
    assert!(entries(&l).is_empty());
    ui.set_time(600);
    ui.update();
    assert_eq!(entries(&l), ["a:LongClick@10,10", "a:ContextMenu@10,10"]);
    take(&l);
    ui.touch_up(0, Point::new(10, 10), Modifiers::NONE);
    assert_eq!(entries(&l), ["a:PointerUp@10,10", "a:Click@10,10"], "unhandled long click keeps the click");
}

#[test]
fn removing_captured_widget_mid_press() {
    let l = log();
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, pad(&l, "a"));
    ui.set_rect(a, Rect::new(0, 0, 100, 100));
    ui.pointer_down(Point::new(10, 10), 1, Modifiers::NONE);
    assert_eq!(ui.captured(), Some(a));
    ui.remove(a);
    assert_eq!(ui.captured(), None);
    assert_eq!(ui.hovered(), None);
    assert!(!ui.pointer_up(Point::new(10, 10), Modifiers::NONE));
}

#[test]
fn default_arrange_follows_gravity() {
    let mut ui = ui();
    let root = ui.root();
    let p = ui.add(root, Pad::default());
    ui.set_rect(p, Rect::new(0, 0, 100, 100));
    let right = ui.add(p, Pad::default());
    ui.set_gravity(right, Gravity::RIGHT | Gravity::TOP);
    ui.set_rect(right, Rect::new(80, 0, 20, 20));
    let fill = ui.add(p, Pad::default());
    ui.set_gravity(fill, Gravity::ALL);
    ui.set_rect(fill, Rect::new(10, 10, 80, 80));
    ui.set_rect(p, Rect::new(0, 0, 150, 120));
    assert_eq!(ui.rect(right), Rect::new(130, 0, 20, 20));
    assert_eq!(ui.rect(fill), Rect::new(10, 10, 130, 100));
}

#[test]
fn coordinates_round_trip() {
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Pad::default());
    ui.set_rect(a, Rect::new(10, 20, 100, 100));
    let b = ui.add(a, Pad::default());
    ui.set_rect(b, Rect::new(5, 7, 10, 10));
    assert_eq!(ui.from_root(b, Point::new(15, 27)), Point::ZERO);
    assert_eq!(ui.to_root(b, Point::new(1, 1)), Point::new(16, 28));
}

#[test]
fn find_by_name() {
    let mut ui = ui();
    let root = ui.root();
    let a = ui.add(root, Pad::default());
    let b = ui.add(a, Pad::default());
    ui.set_id(b, tbid!("ok"));
    assert_eq!(ui.find(root, tbid!("ok")), Some(b));
    assert_eq!(ui.find_as::<Pad>(root, tbid!("ok")), Some(b));
    assert_eq!(ui.find_as::<Probe>(root, tbid!("ok")), None);
    assert_eq!(ui.find(root, tbid!("nope")), None);
}
