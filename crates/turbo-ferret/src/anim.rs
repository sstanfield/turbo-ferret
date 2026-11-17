//! Animations (port-design.md section 11).
//!
//! TB: animation/tb_animation.cpp (TBAnimationManager, curves),
//! animation/tb_widget_animation.cpp (opacity and rect animations). Built-in
//! animations write node data (opacity, rect) and never touch behaviours.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::arena::{Arena, WidgetId, arena_key};
use crate::geom::Rect;
use crate::msg::Millis;
use crate::ui::Ui;

arena_key!(
    /// Handle of a running animation.
    AnimId
);

/// TB: ANIMATION_CURVE
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Curve {
    Linear,
    /// Fast start, slow end (TB's default).
    #[default]
    SlowDown,
    SpeedUp,
    /// Smoothstep.
    Bezier,
    Smooth,
}

/// TB: ANIMATION_DEFAULT_DURATION
pub const DEFAULT_DURATION: u32 = 200;
/// Opacity used instead of 0 so a fading widget stays "visible". TB: TB_ALMOST_ZERO_OPACITY
pub const ALMOST_ZERO_OPACITY: f32 = 0.001;

impl Curve {
    /// Map linear progress (0..1) through the curve. TB: TBAnimationManager::Update
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Curve::Linear => t,
            Curve::SlowDown => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            Curve::SpeedUp => t * t * t,
            Curve::Bezier => t * t * (3.0 - 2.0 * t),
            Curve::Smooth => smooth_curve(t, 0.6),
        }
    }
}

/// TB: tb_animation.cpp sc
fn sc(x: f32) -> f32 {
    let s = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    if x >= 1.0 {
        return s;
    }
    s * ((x / (1.0 + x * x)) / 0.5)
}

/// TB: tb_animation.cpp SmoothCurve
fn smooth_curve(x: f32, a: f32) -> f32 {
    let r = a * x / (2.0 * a * x - a - x + 1.0);
    let r = (r - 0.5) * 2.0;
    sc(r) * 0.5 + 0.5
}

/// What a built-in animation changes.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AnimProp {
    Opacity {
        from: f32,
        to: f32,
    },
    Rect {
        from: Rect,
        to: Rect,
    },
    /// From the current rect to the current rect plus `delta` (read on first update).
    RectDeltaIn(Rect),
    /// From the current rect plus `delta` to the current rect.
    RectDeltaOut(Rect),
}

/// TB: ANIMATION_TIME
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Start {
    #[default]
    Immediately,
    /// The clock starts on the first update (skips a slow first frame).
    FirstUpdate,
}

/// What happens to the target when the animation completes.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum OnDone {
    #[default]
    Nothing,
    Remove,
    Die,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AnimSpec {
    pub target: WidgetId,
    pub prop: AnimProp,
    pub curve: Curve,
    pub duration_ms: u32,
    pub start: Start,
    pub on_done: OnDone,
}

impl AnimSpec {
    pub fn opacity(target: WidgetId, from: f32, to: f32) -> AnimSpec {
        AnimSpec {
            target,
            prop: AnimProp::Opacity { from, to },
            curve: Curve::Bezier,
            duration_ms: DEFAULT_DURATION,
            start: Start::Immediately,
            on_done: OnDone::Nothing,
        }
    }
    pub fn rect(target: WidgetId, from: Rect, to: Rect) -> AnimSpec {
        AnimSpec {
            target,
            prop: AnimProp::Rect { from, to },
            curve: Curve::SlowDown,
            duration_ms: DEFAULT_DURATION,
            start: Start::Immediately,
            on_done: OnDone::Nothing,
        }
    }
    pub fn curve(mut self, c: Curve) -> Self {
        self.curve = c;
        self
    }
    pub fn duration(mut self, ms: u32) -> Self {
        self.duration_ms = ms;
        self
    }
    pub fn start(mut self, s: Start) -> Self {
        self.start = s;
        self
    }
    pub fn on_done(mut self, d: OnDone) -> Self {
        self.on_done = d;
        self
    }
}

/// An app animation (external callback): gets `&mut Ui` and the curved progress.
#[allow(unused_variables)]
pub trait Animate {
    fn start(&mut self, ui: &mut Ui) {}
    fn update(&mut self, ui: &mut Ui, progress: f32);
    fn stop(&mut self, ui: &mut Ui, aborted: bool) {}
}

enum Kind {
    Builtin(AnimSpec),
    Custom(Box<dyn Animate>),
}

struct Anim {
    kind: Option<Kind>,
    curve: Curve,
    duration: u32,
    start_time: Millis,
    adjust_start: bool,
}

#[derive(Default)]
pub(crate) struct Animations {
    arena: Arena<Anim, AnimId>,
    order: Vec<AnimId>,
    blocked: u32,
}

impl Ui {
    /// Start a built-in animation. A running animation of the same property on the
    /// same widget is aborted first.
    pub fn animate(&mut self, spec: AnimSpec) -> AnimId {
        let same = |k: &Kind| match k {
            Kind::Builtin(s) => {
                s.target == spec.target && core::mem::discriminant(&s.prop) == core::mem::discriminant(&spec.prop)
            }
            Kind::Custom(_) => false,
        };
        let old: Vec<AnimId> = self
            .anims
            .order
            .iter()
            .copied()
            .filter(|&id| self.anims.arena.get(id).and_then(|a| a.kind.as_ref()).is_some_and(same))
            .collect();
        for id in old {
            self.abort_animation(id, false);
        }
        self.start_anim(Kind::Builtin(spec), spec.curve, spec.duration_ms, spec.start)
    }

    /// Start an app animation.
    pub fn animate_custom(&mut self, a: Box<dyn Animate>, curve: Curve, duration_ms: u32) -> AnimId {
        self.start_anim(Kind::Custom(a), curve, duration_ms, Start::Immediately)
    }

    /// TB: TBAnimationManager::StartAnimation
    fn start_anim(&mut self, kind: Kind, curve: Curve, duration: u32, start: Start) -> AnimId {
        let duration = if self.anims.blocked > 0 { 0 } else { duration };
        let id = self.anims.arena.insert(Anim {
            kind: None,
            curve,
            duration,
            start_time: self.now,
            adjust_start: start == Start::FirstUpdate,
        });
        self.anims.order.push(id);
        let mut kind = kind;
        match &mut kind {
            Kind::Builtin(s) => match s.prop {
                AnimProp::Opacity { from, .. } => self.set_opacity(s.target, from),
                AnimProp::Rect { from, .. } => self.set_rect(s.target, from),
                _ => self.repaint = true,
            },
            Kind::Custom(a) => a.start(self),
        }
        if let Some(a) = self.anims.arena.get_mut(id) {
            a.kind = Some(kind);
        }
        self.repaint = true;
        id
    }

    /// Stop an animation. With `jump_to_end` it is completed (including `on_done`);
    /// otherwise it stops where it is. TB: AbortAnimation
    pub fn abort_animation(&mut self, id: AnimId, jump_to_end: bool) {
        let Some(mut a) = self.anims.arena.remove(id) else { return };
        self.anims.order.retain(|&x| x != id);
        if let Some(k) = a.kind.take() {
            self.finish_anim(k, a.curve, jump_to_end, true);
        }
    }

    /// Abort every animation targeting `target` (without jumping to the end).
    pub fn abort_animations_for(&mut self, target: WidgetId) {
        let ids: Vec<AnimId> = self
            .anims
            .order
            .iter()
            .copied()
            .filter(|&id| {
                matches!(self.anims.arena.get(id).and_then(|a| a.kind.as_ref()), Some(Kind::Builtin(s)) if s.target == target)
            })
            .collect();
        for id in ids {
            self.abort_animation(id, false);
        }
    }

    /// Run `f` with animations blocked: animations started inside finish at once.
    pub fn block_animations<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.anims.blocked += 1;
        let r = f(self);
        self.anims.blocked -= 1;
        r
    }

    pub fn is_animating(&self, target: WidgetId) -> bool {
        self.anims.order.iter().any(|&id| {
            matches!(self.anims.arena.get(id).and_then(|a| a.kind.as_ref()), Some(Kind::Builtin(s)) if s.target == target)
        })
    }

    pub(crate) fn has_running_animations(&self) -> bool {
        !self.anims.order.is_empty()
    }

    /// Drop animations targeting a removed subtree (no callbacks).
    pub(crate) fn forget_animations(&mut self, root: WidgetId) {
        let tree = &self.tree;
        let arena = &mut self.anims.arena;
        self.anims.order.retain(|&id| {
            let gone = matches!(arena.get(id).and_then(|a| a.kind.as_ref()), Some(Kind::Builtin(s)) if tree.is_ancestor_of(root, s.target));
            if gone {
                arena.remove(id);
            }
            !gone
        });
    }

    /// Advance every animation. TB: TBAnimationManager::Update
    pub(crate) fn update_animations(&mut self) {
        if self.anims.order.is_empty() {
            return;
        }
        let now = self.now;
        let order = core::mem::take(&mut self.anims.order);
        let mut keep = Vec::with_capacity(order.len());
        for id in order {
            let Some(a) = self.anims.arena.get_mut(id) else { continue };
            if a.adjust_start {
                a.start_time = now;
                a.adjust_start = false;
            }
            let mut t = 1.0;
            if a.duration != 0 {
                t = (now.saturating_sub(a.start_time)) as f32 / a.duration as f32;
                t = t.min(1.0);
            }
            let p = a.curve.apply(t);
            let curve = a.curve;
            let Some(mut kind) = a.kind.take() else { continue };
            let alive = self.step_anim(&mut kind, p);
            if !alive {
                self.anims.arena.remove(id);
                continue;
            }
            if t >= 1.0 {
                self.anims.arena.remove(id);
                self.finish_anim(kind, curve, true, false);
            } else {
                if let Some(a) = self.anims.arena.get_mut(id) {
                    a.kind = Some(kind);
                }
                keep.push(id);
            }
        }
        // Animations started during the pass were pushed onto the (empty) list.
        keep.append(&mut self.anims.order);
        self.anims.order = keep;
        self.repaint = true;
    }

    /// Apply progress. Returns false if the target is gone (the animation is dropped).
    fn step_anim(&mut self, kind: &mut Kind, p: f32) -> bool {
        match kind {
            Kind::Builtin(s) => {
                if !self.exists(s.target) {
                    return false;
                }
                match s.prop {
                    AnimProp::Opacity { from, to } => self.set_opacity(s.target, from + (to - from) * p),
                    AnimProp::RectDeltaIn(d) | AnimProp::RectDeltaOut(d) => {
                        // TB: TBWidgetAnimationRect::OnAnimationUpdate (delta modes)
                        let cur = self.rect(s.target);
                        if cur == Rect::ZERO {
                            return false; // not laid out yet
                        }
                        let moved = Rect::new(cur.x + d.x, cur.y + d.y, cur.w + d.w, cur.h + d.h);
                        let (from, to) =
                            if matches!(s.prop, AnimProp::RectDeltaIn(_)) { (cur, moved) } else { (moved, cur) };
                        s.prop = AnimProp::Rect { from, to };
                        self.set_rect(s.target, lerp_rect(from, to, p));
                    }
                    AnimProp::Rect { from, to } => self.set_rect(s.target, lerp_rect(from, to, p)),
                }
                true
            }
            Kind::Custom(a) => {
                a.update(self, p);
                true
            }
        }
    }

    fn finish_anim(&mut self, kind: Kind, _curve: Curve, to_end: bool, aborted: bool) {
        match kind {
            Kind::Builtin(s) => {
                if to_end {
                    match s.prop {
                        AnimProp::Opacity { to, .. } => self.set_opacity(s.target, to),
                        AnimProp::Rect { to, .. } => self.set_rect(s.target, to),
                        _ => {}
                    }
                    match s.on_done {
                        OnDone::Nothing => {}
                        OnDone::Remove => self.remove(s.target),
                        OnDone::Die => self.die(s.target),
                    }
                }
            }
            Kind::Custom(mut a) => a.stop(self, aborted),
        }
    }
}

fn lerp(a: i32, b: i32, t: f32) -> i32 {
    (a as f32 + (b - a) as f32 * t) as i32
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t), lerp(a.w, b.w, t), lerp(a.h, b.h, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_hit_their_ends() {
        for c in [Curve::Linear, Curve::SlowDown, Curve::SpeedUp, Curve::Bezier, Curve::Smooth] {
            assert!((c.apply(0.0)).abs() < 1e-3, "{c:?}");
            assert!((c.apply(1.0) - 1.0).abs() < 1e-3, "{c:?}");
            let mid = c.apply(0.5);
            assert!(mid > 0.0 && mid < 1.0, "{c:?} {mid}");
        }
        assert!(Curve::SlowDown.apply(0.5) > 0.5);
        assert!(Curve::SpeedUp.apply(0.5) < 0.5);
    }
}
