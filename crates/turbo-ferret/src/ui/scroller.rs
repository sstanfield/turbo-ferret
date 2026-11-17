//! Kinetic and smooth scrolling (port-design.md sections 4, 9 and 10).
//!
//! TB: tb_scroller.cpp (TBScroller, TBScrollerFunction) and the scroller parts of
//! tb_widgets.cpp (FindStartedScroller, GetReadyScroller, ScrollToSmooth,
//! ScrollBySmooth). TB's scroller was a message handler owned by the widget (H-08);
//! here the state lives in a side table and `update()` steps it.

use alloc::rc::Rc;

use crate::arena::WidgetId;
use crate::geom::Point;
use crate::math;
use crate::msg::Millis;

use super::Ui;

const PAN_START_THRESHOLD_MS: Millis = 50;
const PAN_POWER_ACC_THRESHOLD_MS: Millis = 600;
const PAN_POWER_MULTIPLIER: f32 = 1.3;
const SCROLL_DECAY: f32 = 200.0;
const SF_GATE_THRESHOLD: f32 = 0.01;

/// Time, speed and distance of a decaying scroll. Speed is in pixels per millisecond,
/// durations in milliseconds, distances in pixels (both may be negative).
/// TB: TBScrollerFunction
#[derive(Copy, Clone, Debug)]
pub struct ScrollerFunction {
    pub decay: f32,
}

impl Default for ScrollerFunction {
    fn default() -> Self {
        ScrollerFunction { decay: SCROLL_DECAY }
    }
}

impl ScrollerFunction {
    /// How long a scroll that starts at `start_speed` lasts. TB: GetDurationFromSpeed
    pub fn duration_from_speed(&self, start_speed: f32) -> f32 {
        let s = math::abs(start_speed);
        if s <= SF_GATE_THRESHOLD {
            return 0.0;
        }
        -math::ln(SF_GATE_THRESHOLD / s) * self.decay
    }

    /// The start speed that covers `distance`. TB: GetSpeedFromDistance
    pub fn speed_from_distance(&self, distance: f32) -> f32 {
        let speed = distance / self.decay;
        if distance > SF_GATE_THRESHOLD {
            speed + SF_GATE_THRESHOLD
        } else if distance < -SF_GATE_THRESHOLD {
            speed - SF_GATE_THRESHOLD
        } else {
            speed
        }
    }

    /// TB: GetDistanceAtTime
    pub fn distance_at_time(&self, start_speed: f32, elapsed_ms: f32) -> f32 {
        start_speed * (1.0 - math::exp(-elapsed_ms.max(0.0) / self.decay)) * self.decay
    }

    /// TB: GetDistanceAtTimeInt
    pub fn distance_at_time_int(&self, start_speed: f32, elapsed_ms: f32) -> i32 {
        let d = self.distance_at_time(start_speed, elapsed_ms);
        (if d < 0.0 { d - 0.5 } else { d + 0.5 }) as i32
    }

    /// The whole distance a scroll starting at `start_speed` covers.
    pub fn total_distance_int(&self, start_speed: f32) -> i32 {
        self.distance_at_time_int(start_speed, self.duration_from_speed(start_speed))
    }
}

const FUNC: ScrollerFunction = ScrollerFunction { decay: SCROLL_DECAY };

/// May change the position a scroll will end at: called with the scrolled widget and
/// the suggested target, returns the target to use. TB: TBScrollerSnapListener
pub type ScrollSnap = Rc<dyn Fn(&Ui, WidgetId, Point) -> Point>;

/// Pan tracking and kinetic scroll state of one scrollable widget. TB: TBScroller
#[derive(Clone)]
pub(crate) struct Scroller {
    pub started: bool,
    /// A scroll is running (TB: a pending "scroll" message).
    pub scrolling: bool,
    pan_d: [f32; 2],
    prev_pan_d: [f32; 2],
    pan_time: Option<Millis>,
    pan_delta_time: f64,
    speed: [f32; 2],
    scroll_start: Option<Millis>,
    duration: [f32; 2],
    start_scroll: Point,
    power: [f32; 2],
    expected: Point,
    snap: Option<ScrollSnap>,
}

impl Default for Scroller {
    fn default() -> Self {
        Scroller {
            started: false,
            scrolling: false,
            pan_d: [0.0; 2],
            prev_pan_d: [0.0; 2],
            pan_time: None,
            pan_delta_time: 0.0,
            speed: [0.0; 2],
            scroll_start: None,
            duration: [0.0; 2],
            start_scroll: Point::ZERO,
            power: [1.0; 2],
            expected: Point::ZERO,
            snap: None,
        }
    }
}

impl Scroller {
    /// TB: TBScroller::Reset (the previous pan direction, the pan power and the
    /// start time survive: the next pan needs them).
    fn reset(&mut self) {
        self.started = false;
        self.scrolling = false;
        self.pan_d = [0.0; 2];
        self.pan_time = None;
        self.pan_delta_time = 0.0;
        self.speed = [0.0; 2];
        self.start_scroll = Point::ZERO;
        self.expected = Point::ZERO;
    }
}

fn xy(p: Point) -> [i32; 2] {
    [p.x, p.y]
}

impl Ui {
    fn scroller_mut(&mut self, id: WidgetId) -> &mut Scroller {
        self.tree.scrollers.get_or_insert_with(id, Scroller::default)
    }

    /// The widget whose scroller is started, from `id` upwards. TB: FindStartedScroller
    pub(crate) fn find_started_scroller(&self, id: WidgetId) -> Option<WidgetId> {
        self.tree.ancestor_ids_or_self(id).find(|&c| self.tree.scrollers.get(c).is_some_and(|s| s.started))
    }

    /// A started scroller, else the nearest scrollable widget. TB: GetReadyScroller
    pub(crate) fn ready_scroller(&self, id: WidgetId, x: bool, y: bool) -> Option<WidgetId> {
        self.find_started_scroller(id).or_else(|| self.find_scrollable(id, x, y))
    }

    /// True while `id` is being panned or is scrolling by itself. TB: IsStarted
    pub fn is_scroller_started(&self, id: WidgetId) -> bool {
        self.tree.scrollers.get(id).is_some_and(|s| s.started)
    }

    /// True while a kinetic or smooth scroll of `id` is running.
    pub fn is_scrolling(&self, id: WidgetId) -> bool {
        self.tree.scrollers.get(id).is_some_and(|s| s.scrolling)
    }

    /// True while any kinetic or smooth scroll is running.
    pub fn has_running_scrollers(&self) -> bool {
        !self.active_scrollers.is_empty()
    }

    /// Let `snap` choose where scrolls of `id` end (for example on page boundaries).
    /// TB: TBScroller::SetSnapListener
    pub fn set_scroll_snap(&mut self, id: WidgetId, snap: Option<ScrollSnap>) {
        if self.tree.is_live(id) {
            self.scroller_mut(id).snap = snap;
        }
    }

    /// Scroll to a position with a decaying speed. TB: ScrollToSmooth
    pub fn scroll_to_smooth(&mut self, id: WidgetId, p: Point) {
        let Some(info) = self.scroll_info(id) else { return };
        let d = p - info.pos;
        if let Some(target) = self.ready_scroller(id, d.x != 0, d.y != 0) {
            self.top(|ui| ui.scroller_scroll_by(target, d, false));
        }
    }

    /// Scroll by a delta with a decaying speed, added to a running scroll.
    /// TB: ScrollBySmooth
    pub fn scroll_by_smooth(&mut self, id: WidgetId, d: Point) {
        if d == Point::ZERO {
            return;
        }
        if let Some(target) = self.ready_scroller(id, d.x != 0, d.y != 0) {
            self.top(|ui| ui.scroller_scroll_by(target, d, true));
        }
    }

    /// Stop pan tracking and any running scroll of `id`. TB: TBScroller::Stop
    pub fn stop_scroller(&mut self, id: WidgetId) {
        if let Some(s) = self.tree.scrollers.get_mut(id) {
            s.reset();
        }
        self.active_scrollers.retain(|&w| w != id);
    }

    /// TB: TBScroller::Start
    fn scroller_start(&mut self, id: WidgetId) {
        let now = self.now;
        let s = self.scroller_mut(id);
        if s.started {
            return;
        }
        s.started = true;
        // Pans that follow each other quickly get more power.
        if s.scroll_start.is_some_and(|t| now < t + PAN_POWER_ACC_THRESHOLD_MS) {
            s.power[0] *= PAN_POWER_MULTIPLIER;
            s.power[1] *= PAN_POWER_MULTIPLIER;
        } else {
            s.power = [1.0; 2];
        }
    }

    /// TB: TBScroller::OnScrollBy
    fn scroller_scroll_by(&mut self, id: WidgetId, d: Point, accumulative: bool) {
        self.scroller_start(id);
        let d = xy(d);
        let mut ppms = [FUNC.speed_from_distance(d[0] as f32), FUNC.speed_from_distance(d[1] as f32)];
        let s = self.scroller_mut(id);
        if accumulative && s.scrolling {
            let (speed, start) = (s.speed, xy(s.start_scroll));
            let pos = xy(self.scroll_info(id).unwrap_or_default().pos);
            for a in 0..2 {
                // Same direction as the running scroll: add what is left of it.
                if (ppms[a] < 0.0) == (speed[a] < 0.0) {
                    let remaining = start[a].saturating_add(FUNC.total_distance_int(speed[a])).saturating_sub(pos[a]);
                    let total = remaining.saturating_add(FUNC.total_distance_int(ppms[a]));
                    ppms[a] = FUNC.speed_from_distance(total as f32);
                }
            }
        }
        self.scroller_adjust_and_scroll(id, ppms);
    }

    /// Pan `id` (or its ancestors) by `d` while the pointer is down, tracking the
    /// speed. Returns true if anything scrolled. TB: TBScroller::OnPan
    pub(crate) fn scroller_pan(&mut self, id: WidgetId, d: Point) -> bool {
        self.scroller_start(id);
        let left = self.scroll_by_recursive(id, d);
        let now = self.now;
        let s = self.scroller_mut(id);
        let d = xy(d);
        if s.pan_time == Some(now) {
            // Several moves at the same time stamp (a host that sets the time once
            // per frame, or a fast mouse) are one movement sample: add to the last
            // one instead of averaging in a zero time step, which would inflate the
            // speed.
            for (a, &da) in d.iter().enumerate() {
                s.pan_d[a] += da as f32 / 2.0;
            }
        } else {
            // Smooth the speed with the previous one to reduce fluctuation.
            if let Some(t) = s.pan_time {
                let dt = now.saturating_sub(t) as f64;
                s.pan_delta_time = if s.pan_delta_time != 0.0 { (dt + s.pan_delta_time) / 2.0 } else { dt };
            }
            s.pan_time = Some(now);
            for (a, &da) in d.iter().enumerate() {
                s.pan_d[a] = (s.pan_d[a] + da as f32) / 2.0;
            }
        }
        for a in 0..2 {
            // A change of direction resets the pan power of that axis.
            if s.pan_d[a] != 0.0 && (s.prev_pan_d[a] < 0.0) != (s.pan_d[a] < 0.0) {
                s.power[a] = 1.0;
            }
        }
        s.prev_pan_d = s.pan_d;
        xy(left) != d
    }

    /// The pointer was released after panning: keep scrolling if it was a flick.
    /// TB: TBScroller::OnPanReleased
    pub(crate) fn scroller_pan_released(&mut self, id: WidgetId) {
        let now = self.now;
        let Some(s) = self.tree.scrollers.get(id) else { return };
        let (pan_d, dt, power) = (s.pan_d, s.pan_delta_time, s.power);
        // The pointer rested before it was released: no flick.
        if s.pan_time.is_none_or(|t| now >= t + PAN_START_THRESHOLD_MS) {
            return self.scroller_stop_or_snap(id);
        }
        // Too little speed: do not scroll by accident.
        let threshold = 2.0 * self.config.dpi as f32 / 100.0;
        if (math::abs(pan_d[0]) < threshold && math::abs(pan_d[1]) < threshold) || dt == 0.0 {
            return self.scroller_stop_or_snap(id);
        }
        let ppms = [pan_d[0] / dt as f32 * power[0], pan_d[1] / dt as f32 * power[1]];
        self.scroller_adjust_and_scroll(id, ppms);
    }

    /// TB: TBScroller::StopOrSnapScroll
    fn scroller_stop_or_snap(&mut self, id: WidgetId) {
        self.scroller_adjust_and_scroll(id, [0.0; 2]);
        if !self.is_scrolling(id) {
            self.stop_scroller(id);
        }
    }

    /// TB: TBScroller::AdjustToSnappingAndScroll
    fn scroller_adjust_and_scroll(&mut self, id: WidgetId, mut ppms: [f32; 2]) {
        let snap = self.tree.scrollers.get(id).and_then(|s| s.snap.clone());
        if let Some(snap) = snap {
            let distance = Point::new(FUNC.total_distance_int(ppms[0]), FUNC.total_distance_int(ppms[1]));
            let pos = self.scroll_info(id).unwrap_or_default().pos;
            let distance = snap(self, id, pos + distance) - pos;
            ppms = [FUNC.speed_from_distance(distance.x as f32), FUNC.speed_from_distance(distance.y as f32)];
        }
        self.scroller_scroll(id, ppms);
    }

    /// TB: TBScroller::Scroll
    fn scroller_scroll(&mut self, id: WidgetId, ppms: [f32; 2]) {
        let now = self.now;
        let start_scroll = self.scroller_target_scroll(id);
        let translation = self.scroller_target_translation(id);
        let s = self.scroller_mut(id);
        s.scroll_start = Some(now);
        s.start_scroll = start_scroll;
        s.speed = ppms;
        s.duration = [FUNC.duration_from_speed(ppms[0]), FUNC.duration_from_speed(ppms[1])];
        if s.duration == [0.0; 2] {
            // Almost still. (TB kept the scroller alive for one more timer tick.)
            return self.stop_scroller(id);
        }
        if !s.scrolling {
            s.scrolling = true;
            s.expected = translation;
            self.active_scrollers.push(id);
            self.repaint = true;
        }
    }

    /// The node whose child translation moves when `id` scrolls. TB: GetScrollRoot
    pub fn scroll_root(&self, id: WidgetId) -> WidgetId {
        self.tree.scroll_root(id)
    }

    /// Child translation of `id`'s scroll root.
    pub(crate) fn scroll_root_translation(&self, id: WidgetId) -> Point {
        self.tree.child_translation(self.scroll_root(id))
    }

    /// Where the scrolled content is, in root coordinates: used to notice that
    /// something else moved it. TB: TBScroller::GetTargetChildTranslation
    fn scroller_target_translation(&self, id: WidgetId) -> Point {
        self.to_root(self.scroll_root(id), Point::ZERO) + self.scroll_root_translation(id)
    }

    /// Scroll positions summed from the scroll root upwards.
    /// TB: TBScroller::GetTargetScrollXY
    fn scroller_target_scroll(&self, id: WidgetId) -> Point {
        let mut p = Point::ZERO;
        for c in self.tree.ancestor_ids_or_self(self.scroll_root(id)) {
            if let Some(i) = self.scroll_info(c) {
                p += i.pos;
            }
        }
        p
    }

    /// Advance every running scroll to the current time (called by `update`).
    /// TB: TBScroller::OnMessageReceived, without the 60 Hz timer: the host's update
    /// rate sets the pace.
    pub(crate) fn update_scrollers(&mut self) {
        if self.active_scrollers.is_empty() {
            return;
        }
        let ids = self.active_scrollers.clone();
        for id in ids {
            self.scroller_step(id);
        }
    }

    fn scroller_step(&mut self, id: WidgetId) {
        let Some(s) = self.tree.scrollers.get(id).filter(|s| s.scrolling) else {
            self.active_scrollers.retain(|&w| w != id);
            return;
        };
        let (speed, duration, start, start_scroll, expected) =
            (s.speed, s.duration, s.scroll_start.unwrap_or(self.now), s.start_scroll, s.expected);
        if !self.tree.is_live(id) {
            return self.stop_scroller(id);
        }
        // Something else moved the content (the limits shrank, another action
        // scrolled, or a nested scroller started moving the parents): abort.
        let actual = self.scroller_target_translation(id);
        if actual != expected {
            return self.stop_scroller(id);
        }
        // The position for the elapsed time, each axis clipped to its duration.
        let elapsed = self.now.saturating_sub(start) as f32;
        let e = [elapsed.min(duration[0]), elapsed.min(duration[1])];
        let want = Point::new(
            FUNC.distance_at_time_int(speed[0], e[0]).saturating_add(start_scroll.x),
            FUNC.distance_at_time_int(speed[1], e[1]).saturating_add(start_scroll.y),
        );
        let d = want - self.scroller_target_scroll(id);
        if d != Point::ZERO {
            self.scroll_by_recursive(id, d);
        }
        let moved = self.scroller_target_translation(id);
        if let Some(s) = self.tree.scrollers.get_mut(id) {
            s.expected = moved;
        }
        // An axis is done when its time is up or when it tried to move and could not
        // (the end was reached). TB stopped at the end only when both axes were
        // stuck in the same step, and otherwise ran until the time was up.
        let done_x = elapsed >= duration[0] || (d.x != 0 && moved.x == actual.x);
        let done_y = elapsed >= duration[1] || (d.y != 0 && moved.y == actual.y);
        if done_x && done_y {
            self.stop_scroller(id);
        }
        self.repaint = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_round_trips() {
        let f = ScrollerFunction::default();
        assert_eq!(f.duration_from_speed(0.0), 0.0);
        assert_eq!(f.duration_from_speed(0.005), 0.0);
        assert_eq!(f.speed_from_distance(0.0), 0.0);
        for &dist in &[1.0f32, 10.0, 100.0, 1234.0, -77.0, -2500.0] {
            let speed = f.speed_from_distance(dist);
            let got = f.total_distance_int(speed);
            assert!((got - dist as i32).abs() <= 1, "distance {dist}: speed {speed} gives {got}");
            assert!(f.duration_from_speed(speed) > 0.0);
        }
    }

    #[test]
    fn distance_grows_and_settles() {
        let f = ScrollerFunction::default();
        let speed = 2.0;
        let dur = f.duration_from_speed(speed);
        let mut last = 0.0;
        let mut t = 0.0;
        while t <= dur {
            let d = f.distance_at_time(speed, t);
            assert!(d >= last);
            last = d;
            t += 16.0;
        }
        // The speed has decayed to the gate threshold, so almost all of the
        // asymptotic distance (speed * decay) is covered.
        assert!((f.distance_at_time(speed, dur) - speed * f.decay).abs() < 3.0);
        assert_eq!(f.distance_at_time_int(-1.0, 1e9), -200);
    }
}
