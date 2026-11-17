//! Re-entrancy fuzzer for the core, from the review (2026-11): random tree operations from inside hooks and listeners,
//! random input, and structural invariants after every top-level call.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::anim::{AnimSpec, OnDone};
use turbo_ferret::*;

thread_local! { static ORPHANS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

struct Fz {
    rng: u64,
    ids: Vec<WidgetId>,
    depth: u32,
    /// Probability (in 1/1000) that a hook does something.
    hook_p: u32,
    ops: u64,
    listeners: Vec<ListenerId>,
    trace: Vec<String>,
}

type Sh = Rc<RefCell<Fz>>;

fn rnd(sh: &Sh) -> u64 {
    let mut s = sh.borrow_mut();
    // xorshift64*
    s.rng ^= s.rng >> 12;
    s.rng ^= s.rng << 25;
    s.rng ^= s.rng >> 27;
    s.rng.wrapping_mul(0x2545F4914F6CDD1D)
}
fn below(sh: &Sh, n: u64) -> u64 {
    rnd(sh) % n
}
fn pick(sh: &Sh) -> WidgetId {
    let n = sh.borrow().ids.len() as u64;
    let i = below(sh, n) as usize;
    sh.borrow().ids[i]
}
fn tr(sh: &Sh, s: String) {
    let mut f = sh.borrow_mut();
    if f.trace.len() > 400 {
        f.trace.remove(0);
    }
    f.trace.push(s);
}

struct F {
    sh: Sh,
    scroll: Option<ScrollInfo>,
    dest: Option<WidgetId>,
    value: i32,
}

impl F {
    fn new(sh: &Sh) -> F {
        let scroll =
            (below(sh, 4) == 0).then(|| ScrollInfo { min: Point::ZERO, max: Point::new(50, 200), pos: Point::ZERO });
        let dest = (below(sh, 10) == 0).then(|| pick(sh));
        F { sh: sh.clone(), scroll, dest, value: 0 }
    }
    fn maybe(&mut self, cx: &mut Ctx, what: &str) {
        let p = self.sh.borrow().hook_p;
        if below(&self.sh, 1000) < p as u64 {
            let d = self.sh.borrow().depth;
            if d < 6 {
                self.sh.borrow_mut().depth += 1;
                tr(&self.sh, format!("{}hook {what} on {:?}", " ".repeat(d as usize), cx.this()));
                let me = cx.this();
                random_op(&self.sh, cx.ui(), Some(me));
                self.sh.borrow_mut().depth -= 1;
            }
        }
    }
}

impl Widget for F {
    fn type_name(&self) -> &'static str {
        "F"
    }
    fn on_added(&mut self, cx: &mut Ctx) {
        self.maybe(cx, "added")
    }
    fn on_removed(&mut self, cx: &mut Ctx) {
        self.maybe(cx, "removed")
    }
    fn on_child_added(&mut self, cx: &mut Ctx, _c: WidgetId) {
        self.maybe(cx, "child_added")
    }
    fn on_child_removed(&mut self, cx: &mut Ctx, _c: WidgetId) {
        self.maybe(cx, "child_removed")
    }
    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        self.maybe(cx, "die");
        match below(&self.sh, 4) {
            0 => DieAction::KeepAlive,
            1 => {
                let me = cx.this();
                cx.animate(AnimSpec::opacity(me, 1.0, 0.001).on_done(OnDone::Remove));
                DieAction::KeepAlive
            }
            _ => DieAction::Remove,
        }
    }
    fn on_focus_changed(&mut self, cx: &mut Ctx, _f: bool) {
        self.maybe(cx, "focus")
    }
    fn on_capture_changed(&mut self, cx: &mut Ctx, _c: bool) {
        self.maybe(cx, "capture")
    }
    fn on_visibility_changed(&mut self, cx: &mut Ctx) {
        self.maybe(cx, "vis")
    }
    fn on_window_active_changed(&mut self, cx: &mut Ctx, _a: bool) {
        self.maybe(cx, "winactive")
    }
    fn on_event(&mut self, cx: &mut Ctx, _ev: &Event) -> bool {
        self.maybe(cx, "event");
        below(&self.sh, 5) == 0
    }
    fn on_message(&mut self, cx: &mut Ctx, _m: &Message) {
        self.maybe(cx, "message")
    }
    fn on_process_states(&mut self, cx: &mut Ctx) {
        self.maybe(cx, "states")
    }
    fn on_process(&mut self, cx: &mut Ctx) {
        self.maybe(cx, "process")
    }
    fn on_process_after_children(&mut self, cx: &mut Ctx) {
        self.maybe(cx, "after")
    }
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        self.maybe(cx, "measure");
        let mut ps = cx.default_measure_content(sc);
        ps.pref = ps.pref.max(Size::new(20, 20));
        ps
    }
    fn arrange(&mut self, cx: &mut Ctx, old: Size, new: Size) {
        self.maybe(cx, "arrange");
        cx.default_arrange(old, new)
    }
    fn value(&self) -> Value {
        Value::Int(self.value)
    }
    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        let v = v.as_int();
        if v != self.value {
            self.value = v;
            self.maybe(cx, "set_value");
            cx.emit_changed(Value::Int(v));
        }
    }
    fn event_destination(&self, this: NodeRef<'_>) -> Option<WidgetId> {
        self.dest.or(this.parent())
    }
    fn scroll_info(&self) -> Option<ScrollInfo> {
        self.scroll
    }
    fn child_translation(&self) -> Point {
        self.scroll.map(|s| -s.pos).unwrap_or(Point::ZERO)
    }
    fn scroll_to(&mut self, cx: &mut Ctx, p: Point) {
        if let Some(s) = &mut self.scroll {
            let np = Point::new(p.x.clamp(s.min.x, s.max.x), p.y.clamp(s.min.y, s.max.y));
            if np != s.pos {
                s.pos = np;
                cx.invalidate();
                self.maybe(cx, "scroll_to");
            }
        }
    }
}

struct L(Sh);
impl L {
    fn maybe(&mut self, ui: &mut Ui, what: &str) {
        let p = self.0.borrow().hook_p;
        if below(&self.0, 1000) < p as u64 {
            let d = self.0.borrow().depth;
            if d < 6 {
                self.0.borrow_mut().depth += 1;
                tr(&self.0, format!("{}listener {what}", " ".repeat(d as usize)));
                random_op(&self.0, ui, None);
                self.0.borrow_mut().depth -= 1;
            }
        }
    }
}
impl UiListener for L {
    fn on_added(&mut self, ui: &mut Ui, _p: WidgetId, _c: WidgetId) {
        self.maybe(ui, "added")
    }
    fn on_removing(&mut self, ui: &mut Ui, _p: Option<WidgetId>, _c: WidgetId) {
        self.maybe(ui, "removing")
    }
    fn on_dying(&mut self, ui: &mut Ui, _id: WidgetId) -> bool {
        self.maybe(ui, "dying");
        below(&self.0, 8) == 0
    }
    fn on_focus_changed(&mut self, ui: &mut Ui, _id: WidgetId, _f: bool) {
        self.maybe(ui, "focus")
    }
    fn on_event(&mut self, ui: &mut Ui, _t: WidgetId, _ev: &Event) -> bool {
        self.maybe(ui, "event");
        below(&self.0, 20) == 0
    }
}

fn rpoint(sh: &Sh) -> Point {
    Point::new(below(sh, 440) as i32 - 20, below(sh, 340) as i32 - 20)
}

fn pick_live(sh: &Sh, ui: &Ui) -> WidgetId {
    for _ in 0..8 {
        let p = pick(sh);
        if ui.exists(p) && (p != ui.root() || below(sh, 4) == 0) {
            return p;
        }
    }
    pick(sh)
}

fn random_op(sh: &Sh, ui: &mut Ui, me: Option<WidgetId>) {
    sh.borrow_mut().ops += 1;
    let own = me.filter(|_| below(sh, 3) == 0);
    let a = if let Some(me) = own {
        me
    } else if below(sh, 8) == 0 {
        pick(sh)
    } else {
        pick_live(sh, ui)
    };
    let b = pick_live(sh, ui);
    let op = below(sh, 44);
    tr(sh, format!("{}op {op} a={a:?} b={b:?}", " ".repeat(sh.borrow().depth as usize)));
    match op {
        0..=4 => {
            if sh.borrow().ids.len() < 400 {
                let w = F::new(sh);
                let id = ui.add(a, w);
                sh.borrow_mut().ids.push(id);
                ui.set_rect(
                    id,
                    Rect::new(
                        below(sh, 200) as i32,
                        below(sh, 150) as i32,
                        20 + below(sh, 200) as i32,
                        20 + below(sh, 150) as i32,
                    ),
                );
                if below(sh, 2) == 0 {
                    ui.set_focusable(id, true);
                }
                if below(sh, 3) == 0 {
                    ui.set_click_by_key(id, true);
                }
                if below(sh, 3) == 0 {
                    ui.set_want_long_click(id, true);
                }
                if below(sh, 6) == 0 {
                    ui.set_window(id, true, below(sh, 2) == 0);
                }
            }
        }
        5 | 6 => ui.remove(a),
        7 | 8 => ui.die(a),
        9 => {
            ui.set_focus(a);
        }
        10 => ui.set_rect(
            a,
            Rect::new(below(sh, 200) as i32, below(sh, 150) as i32, below(sh, 300) as i32, below(sh, 200) as i32),
        ),
        11 => {
            ui.set_visibility(a, [Visibility::Visible, Visibility::Invisible, Visibility::Gone][below(sh, 3) as usize])
        }
        12 => ui.set_disabled(a, below(sh, 2) == 0),
        13 => ui.post_message(a, tbid!("m"), Value::Int(1)),
        14 => ui.post_message_delayed(a, tbid!("m"), Value::Int(2), below(sh, 100) as u32),
        15 => ui.cancel_messages(a, None),
        16 => {
            ui.animate(
                AnimSpec::opacity(a, 0.2, 1.0)
                    .duration(below(sh, 100) as u32)
                    .on_done([OnDone::Nothing, OnDone::Remove, OnDone::Die][below(sh, 3) as usize]),
            );
        }
        17 => {
            let r = ui.rect(a);
            ui.animate(AnimSpec::rect(a, r, Rect::new(5, 5, 60, 60)).duration(below(sh, 100) as u32));
        }
        18 => ui.abort_animations_for(a),
        19 => ui.set_z(a, if below(sh, 2) == 0 { Z::Top } else { Z::Bottom }),
        20 => ui.connect_value(a, TbId(1 + below(sh, 3) as u32)),
        21 => ui.set_bound_value(TbId(1 + below(sh, 3) as u32), below(sh, 5) as i32),
        22 => ui.set_value(a, below(sh, 5) as i32),
        23 => ui.scroll_by_smooth(a, Point::new(below(sh, 100) as i32 - 50, below(sh, 300) as i32 - 150)),
        24 => ui.scroll_to_smooth(a, Point::new(below(sh, 100) as i32, below(sh, 300) as i32)),
        25 => ui.release_capture(a),
        26 => {
            ui.send(
                a,
                [EventKind::Click, EventKind::Changed, EventKind::KeyDown, EventKind::Custom(TbId(5))]
                    [below(sh, 4) as usize],
            );
        }
        27 => ui.activate_window(a),
        28 => {
            ui.with_behaviour(a, |w, cx| {
                w.on_process(cx);
            });
        }
        29 => {
            if sh.borrow().listeners.len() < 6 {
                let id = if below(sh, 2) == 0 {
                    Some(ui.add_listener(Box::new(L(sh.clone()))))
                } else {
                    ui.add_widget_listener(a, Box::new(L(sh.clone())))
                };
                if let Some(id) = id {
                    sh.borrow_mut().listeners.push(id);
                }
            }
        }
        30 => {
            let n = sh.borrow().listeners.len();
            if n > 0 {
                let i = below(sh, n as u64) as usize;
                let id = sh.borrow_mut().listeners.swap_remove(i);
                ui.remove_listener(id);
            }
        }
        31 => {
            if sh.borrow().ids.len() < 400 {
                let w = F::new(sh);
                let id = ui.add_relative(a, w, if below(sh, 2) == 0 { ZRel::Before } else { ZRel::After });
                sh.borrow_mut().ids.push(id);
            }
        }
        32 => {
            ui.move_focus(below(sh, 2) == 0);
        }
        33 => ui.set_opacity(a, below(sh, 3) as f32 / 2.0),
        34 => {
            ui.set_layout_params(a, if below(sh, 2) == 0 { Some(LayoutParams::fixed(30, 30)) } else { None });
        }
        35 => {
            ui.preferred_size_free(a);
            ui.place_child(a, b);
        }
        36 => ui.scroll_to(a, Point::new(below(sh, 100) as i32, below(sh, 300) as i32)),
        37 => ui.scroll_into_view_recursive(a),
        38 => ui.set_layout_boundary(a, below(sh, 2) == 0),
        39 => ui.stop_scroller(a),
        40 => ui.pointer_cancel(),
        41 => {
            ui.set_focus_recursive(a);
            ui.ensure_focus(a);
        }
        42 => ui.set_window(a, below(sh, 2) == 0, below(sh, 2) == 0),
        _ => {
            ui.invalidate_layout(a);
            ui.resize_to_preferred(a);
        }
    }
}

fn check(sh: &Sh, ui: &Ui, what: &str) {
    let ids = sh.borrow().ids.clone();
    let dump = || sh.borrow().trace.join("\n");
    let mut live = 1;
    for &id in &ids[1..] {
        if !ui.exists(id) {
            assert!(ui.children(id).is_empty());
            continue;
        }
        live += 1;
        if let Some(p) = ui.parent(id) {
            assert!(ui.exists(p), "{what}: parent of live {id:?} is dead\n{}", dump());
            assert!(ui.children(p).contains(&id), "{what}: {id:?} not in its parent's children\n{}", dump());
        }
        for &c in ui.children(id) {
            assert!(ui.exists(c), "{what}: dead child\n{}", dump());
            assert_eq!(ui.parent(c), Some(id), "{what}: child's parent\n{}", dump());
        }
        assert_eq!(
            ui.children(id).iter().filter(|&&c| ui.children(id).iter().filter(|&&d| d == c).count() > 1).count(),
            0
        );
    }
    assert_eq!(ui.node_count(), live, "{what}: node_count vs live ids\n{}", dump());
    for (n, w) in [("hovered", ui.hovered()), ("captured", ui.captured()), ("focused", ui.focused())] {
        if let Some(w) = w {
            assert!(ui.exists(w), "{what}: {n} is dead\n{}", dump());
        }
    }
    if let Some(f) = ui.focused() {
        assert!(!ui.is_dying(f), "{what}: focused widget is dying\n{}", dump());
    }
    if let Some(f) = ui.captured() {
        assert!(!ui.is_dying(f), "{what}: captured widget is dying\n{}", dump());
    }
    for &id in &ids {
        if ui.is_scrolling(id) {
            assert!(ui.exists(id), "{what}: dead widget scrolling\n{}", dump());
            assert!(ui.has_running_scrollers(), "{what}: scrolling but not active\n{}", dump());
        }
        if ui.captured().is_none() && !ui.has_running_scrollers() {
            assert!(!ui.is_scroller_started(id), "{what}: scroller of {id:?} stuck started\n{}", dump());
        }
    }
}

fn run(seed: u64, steps: u32, hook_p: u32) {
    let mut ui = Ui::new(Config::default(), Services::with_loader(MemLoader::new()));
    ui.services_mut().log = None;
    ui.set_viewport(Size::new(400, 300));
    let sh: Sh = Rc::new(RefCell::new(Fz {
        rng: seed.wrapping_mul(0x9E3779B97F4A7C15) | 1,
        ids: vec![ui.root()],
        depth: 0,
        hook_p,
        ops: 0,
        listeners: Vec::new(),
        trace: Vec::new(),
    }));
    let mut now = 1000u64;
    for step in 0..steps {
        let k = below(&sh, 20);
        tr(&sh, format!("step {step} kind {k}"));
        match k {
            0..=5 => random_op(&sh, &mut ui, None),
            6 => {
                ui.pointer_down(rpoint(&sh), 1 + below(&sh, 2) as u32, Modifiers::NONE);
            }
            7 | 8 => ui.pointer_move(rpoint(&sh), Modifiers::NONE),
            9 => {
                ui.pointer_up(rpoint(&sh), Modifiers::NONE);
            }
            10 => {
                ui.wheel(rpoint(&sh), Point::new(0, below(&sh, 5) as i32 - 2), Modifiers::NONE);
            }
            11 => {
                let down = below(&sh, 2) == 0;
                match below(&sh, 4) {
                    0 => ui.key(
                        None,
                        SpecialKey::Tab,
                        if below(&sh, 2) == 0 { Modifiers::SHIFT } else { Modifiers::NONE },
                        down,
                    ),
                    1 => ui.key(None, SpecialKey::Enter, Modifiers::NONE, down),
                    2 => ui.key(Some(' '), SpecialKey::None, Modifiers::NONE, down),
                    _ => ui.key(Some('x'), SpecialKey::None, Modifiers::NONE, down),
                };
            }
            12 => {
                let t = below(&sh, 3) as u32;
                match below(&sh, 4) {
                    0 => {
                        ui.touch_down(t, rpoint(&sh), 1, Modifiers::NONE);
                    }
                    1 => {
                        ui.touch_up(t, rpoint(&sh), Modifiers::NONE);
                    }
                    2 => ui.touch_move(t, rpoint(&sh), Modifiers::NONE),
                    _ => ui.touch_cancel(t),
                }
            }
            _ => {
                now += below(&sh, 40);
                ui.set_time(now);
                ui.update();
                let _ = ui.next_wake_time();
            }
        }
        check(&sh, &ui, &format!("seed {seed} step {step}"));
    }
    // Teardown: everything goes away and the UI comes to rest.
    sh.borrow_mut().hook_p = 0;
    let ls = std::mem::take(&mut sh.borrow_mut().listeners);
    for l in ls {
        ui.remove_listener(l);
    }
    ui.pointer_cancel();
    for t in 1..3 {
        ui.touch_cancel(t);
    }
    let kids = ui.children(ui.root()).to_vec();
    for k in kids {
        ui.remove(k);
    }
    let ids = sh.borrow().ids.clone();
    let mut orphans = 0;
    for &id in &ids[1..] {
        if ui.exists(id) {
            orphans += 1;
            assert!(ui.parent_root(id) != ui.root());
        }
    }
    for &id in &ids[1..] {
        ui.remove(id);
    }
    for _ in 0..50 {
        now += 500;
        ui.set_time(now);
        ui.update();
    }
    let dump = || sh.borrow().trace.join("\n");
    assert_eq!(ui.node_count(), 1, "seed {seed}: nodes left\n{}", dump());
    assert_eq!(ui.next_wake_time(), None, "seed {seed}: still wants to wake\n{}", dump());
    assert!(ui.focused().is_none() && ui.hovered().is_none() && ui.captured().is_none());
    ORPHANS.with(|o| o.set(o.get() + orphans));
}

#[test]
fn fuzz_light() {
    for seed in 1..=150 {
        run(seed, 1500, 30);
    }
}

#[test]
fn fuzz_heavy() {
    for seed in 1..=150 {
        run(seed, 600, 200);
    }
}

/// The full run (about 15 s): `cargo test -p turbo-ferret --test review_core_fuzz -- --ignored`
#[test]
#[ignore = "slow"]
fn fuzz_full() {
    for seed in 1..=1500 {
        run(seed, 1500, 30);
        run(seed, 600, 200);
    }
}
