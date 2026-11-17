//! `Window`: a movable, resizable, activatable container with a title bar.
//! TB: tb_window.cpp (TBWindow) + its fades from TBWidgetsAnimationManager

use alloc::string::String;

use crate::anim::{ALMOST_ZERO_OPACITY, AnimSpec, OnDone};
use crate::arena::WidgetId;
use crate::event::{Event, EventKind};
use crate::flags::flags;
use crate::geom::{Edges, Rect, Size};
use crate::layout::size::{PreferredSize, SizeConstraints, Visibility};
use crate::ui::{Ctx, NodeRef, Ui};
use crate::widget::{DieAction, Widget, WidgetState, Z};

use super::button::Button;
use super::misc::{Mover, Resizer};
use super::text::TextField;

flags! {
    /// TB: WINDOW_SETTINGS
    pub struct WindowSettings: u8 {
        const TITLEBAR = 1;
        const RESIZABLE = 2;
        const CLOSE_BUTTON = 4;
        const CAN_ACTIVATE = 8;
    }
}

impl WindowSettings {
    /// TB: WINDOW_SETTINGS_DEFAULT
    pub const DEFAULT: WindowSettings = WindowSettings(1 | 2 | 4 | 8);
}

/// How `resize_to_fit_content` sizes a window. TB: TBWindow::RESIZE_FIT
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ResizeFit {
    Preferred,
    Minimal,
    CurrentOrNeeded,
}

pub struct Window {
    title: String,
    settings: WindowSettings,
    /// Fade in on add and out on die.
    pub animate: bool,
    title_h: i32,
    /// The title height the content was last placed for (only `arrange` sets it).
    arranged_title_h: i32,
    mover: Option<WidgetId>,
    resizer: Option<WidgetId>,
    textfield: Option<WidgetId>,
    close: Option<WidgetId>,
}

impl Default for Window {
    fn default() -> Self {
        Window {
            title: String::new(),
            settings: WindowSettings::DEFAULT,
            animate: true,
            title_h: 0,
            arranged_title_h: 0,
            mover: None,
            resizer: None,
            textfield: None,
            close: None,
        }
    }
}

impl Window {
    pub fn new(title: &str) -> Window {
        Window { title: String::from(title), ..Default::default() }
    }

    pub fn with_settings(mut self, s: WindowSettings) -> Window {
        self.settings = s;
        self
    }

    pub fn settings(&self) -> WindowSettings {
        self.settings
    }

    pub fn close_button(&self) -> Option<WidgetId> {
        self.close
    }

    fn is_internal(&self, id: WidgetId) -> bool {
        Some(id) == self.mover || Some(id) == self.resizer
    }

    /// TB: TBWindow::SetSettings
    pub fn set_settings(&mut self, cx: &mut Ctx, s: WindowSettings) {
        self.settings = s;
        self.apply_settings(cx);
    }

    /// TB: TBWindow::SetSettings (hidden parts are made `Gone`).
    fn apply_settings(&self, cx: &mut Ctx) {
        let me = cx.this();
        let s = self.settings;
        cx.set_window(me, true, s.contains(WindowSettings::CAN_ACTIVATE));
        let vis = |on: bool| if on { Visibility::Visible } else { Visibility::Gone };
        if let Some(m) = self.mover {
            cx.set_visibility(m, vis(s.contains(WindowSettings::TITLEBAR)));
        }
        if let Some(r) = self.resizer {
            cx.set_visibility(r, vis(s.contains(WindowSettings::RESIZABLE)));
        }
        if let Some(c) = self.close {
            cx.set_visibility(c, vis(s.contains(WindowSettings::CLOSE_BUTTON)));
        }
        cx.invalidate_layout(me);
    }

    /// TB: TBWindow::GetTitleHeight
    fn title_height(&self, cx: &mut Ctx) -> i32 {
        match self.mover {
            Some(m) if self.settings.contains(WindowSettings::TITLEBAR) => {
                cx.preferred_size(m, SizeConstraints::NONE).pref.h
            }
            _ => 0,
        }
    }
}

/// The `Widget` methods of a window, forwarded to a `Window` field, for widgets that
/// are windows with something added (TB: TBWindow subclasses). The widget writes
/// `type_name`, `on_create`, `on_event`, `on_die` and what else it changes itself.
macro_rules! forward_to_window {
    ($field:ident) => {
        fn extra_padding(&self, this: $crate::ui::NodeRef<'_>) -> $crate::geom::Edges {
            self.$field.extra_padding(this)
        }
        fn measure_adds_skin(&self) -> bool {
            self.$field.measure_adds_skin()
        }
        fn measure_content(
            &mut self,
            cx: &mut $crate::ui::Ctx,
            sc: $crate::layout::size::SizeConstraints,
        ) -> $crate::layout::size::PreferredSize {
            self.$field.measure_content(cx, sc)
        }
        fn arrange(&mut self, cx: &mut $crate::ui::Ctx, old: $crate::geom::Size, new: $crate::geom::Size) {
            self.$field.arrange(cx, old, new)
        }
        fn on_child_added(&mut self, cx: &mut $crate::ui::Ctx, child: $crate::arena::WidgetId) {
            self.$field.on_child_added(cx, child)
        }
        fn on_window_active_changed(&mut self, cx: &mut $crate::ui::Ctx, active: bool) {
            self.$field.on_window_active_changed(cx, active)
        }
        fn on_added(&mut self, cx: &mut $crate::ui::Ctx) {
            self.$field.on_added(cx)
        }
        fn text(&self) -> Option<&str> {
            self.$field.text()
        }
        fn set_text(&mut self, cx: &mut $crate::ui::Ctx, text: &str) {
            self.$field.set_text(cx, text)
        }
    };
}
pub(crate) use forward_to_window;

impl Widget for Window {
    fn type_name(&self) -> &'static str {
        "Window"
    }

    /// TB: TBWindow::TBWindow
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBWindow"));
        cx.set_group_root(me, true);
        let mover = cx.add(me, Mover);
        cx.set_skin_quiet(mover, crate::tbid!("TBWindow.mover"));
        let resizer = cx.add(me, Resizer);
        let tf = cx.add(mover, TextField::new(&self.title));
        cx.set_ignore_input(tf, true);
        let close = cx.add(mover, Button::default());
        cx.set_skin_quiet(close, crate::tbid!("TBWindow.close"));
        cx.set_focusable(close, false);
        cx.set_id(close, crate::tbid!("TBWindow.close"));
        self.mover = Some(mover);
        self.resizer = Some(resizer);
        self.textfield = Some(tf);
        self.close = Some(close);
        self.apply_settings(cx);
        // Know the title height before content is inflated and placed.
        self.title_h = self.title_height(cx);
    }

    fn extra_padding(&self, _this: NodeRef) -> Edges {
        Edges::new(0, self.title_h, 0, 0)
    }

    fn measure_adds_skin(&self) -> bool {
        false
    }

    /// Children plus skin padding plus the title. TB: TBWindow::OnCalculatePreferredSize
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        self.title_h = self.title_height(cx);
        let mut ps = cx.default_measure_content(sc);
        let me = cx.this();
        if let Some(e) = cx.skin_element(me) {
            let pad = e.padding;
            ps.min.w += pad.horizontal();
            ps.pref.w += pad.horizontal();
            ps.min.h += pad.vertical();
            ps.pref.h += pad.vertical();
        }
        ps.min.h += self.title_h;
        ps.pref.h += self.title_h;
        ps
    }

    /// TB: TBWindow::OnResized
    fn arrange(&mut self, cx: &mut Ctx, old: Size, new: Size) {
        let me = cx.this();
        // If the title height changed (skin or font), move the content with it.
        let th = self.title_height(cx);
        let dt = th - self.arranged_title_h;
        self.title_h = th;
        self.arranged_title_h = th;
        if dt != 0 {
            for c in cx.children(me).to_vec() {
                if self.is_internal(c) {
                    continue;
                }
                let mut r = cx.rect(c);
                r.y += dt;
                if cx.gravity(c).fills_y() {
                    r.h -= dt;
                }
                cx.set_rect(c, r);
            }
        }
        cx.default_arrange(old, new);
        let w = cx.rect(me).w;
        let h = cx.rect(me).h;
        if let Some(m) = self.mover {
            cx.set_rect(m, Rect::new(0, 0, w, self.title_h));
        }
        if let Some(r) = self.resizer {
            let ps = cx.preferred_size(r, SizeConstraints::NONE);
            cx.set_rect(r, Rect::new(w - ps.pref.w, h - ps.pref.h, ps.pref.w, ps.pref.h));
        }
        let (Some(m), Some(c), Some(t)) = (self.mover, self.close, self.textfield) else { return };
        let mr = cx.rect(m);
        let mp = cx.padding_rect(m);
        let pad_right = mr.w - (mp.x + mp.w);
        let cps = cx.preferred_size(c, SizeConstraints::NONE);
        let bw = cps.pref.w;
        let bh = cps.pref.h.max(mp.h);
        cx.set_rect(c, Rect::new(mp.x + mp.w - bw, mp.y, bw, bh));
        let mut tr = mp;
        if self.settings.contains(WindowSettings::CLOSE_BUTTON) {
            tr.w -= pad_right + bw;
        }
        cx.set_rect(t, tr);
    }

    /// TB: TBWindow::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if Some(ev.target) == self.close {
            if ev.kind == EventKind::Click {
                let me = cx.this();
                cx.die(me);
            }
            return true;
        }
        false
    }

    /// Keep the resizer on top. TB: TBWindow::OnChildAdded
    fn on_child_added(&mut self, cx: &mut Ctx, child: WidgetId) {
        if let Some(r) = self.resizer {
            cx.set_z(r, Z::Top);
        }
        // The first content is placed below the title as it is now: from here on
        // only `arrange` moves content when the title height changes.
        let me = cx.this();
        let first = !self.is_internal(child) && !cx.children(me).iter().any(|&c| c != child && !self.is_internal(c));
        if first {
            self.arranged_title_h = self.title_h;
        }
    }

    /// TB: TBWindow::SetWindowActiveState (the mover shows the active state too)
    fn on_window_active_changed(&mut self, cx: &mut Ctx, active: bool) {
        if let Some(m) = self.mover {
            cx.set_state(m, WidgetState::SELECTED, active);
        }
    }

    fn on_added(&mut self, cx: &mut Ctx) {
        if self.animate {
            let me = cx.this();
            cx.animate(AnimSpec::opacity(me, ALMOST_ZERO_OPACITY, 1.0));
        }
    }

    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        if !self.animate {
            return DieAction::Remove;
        }
        let me = cx.this();
        cx.animate(AnimSpec::opacity(me, 1.0, ALMOST_ZERO_OPACITY).on_done(OnDone::Remove));
        DieAction::KeepAlive
    }

    fn text(&self) -> Option<&str> {
        Some(&self.title)
    }

    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        self.title = String::from(text);
        if let Some(t) = self.textfield {
            cx.set_text(t, text);
        }
    }
}

impl Ui {
    /// Set a window's settings (title bar, resizer, close button, activation).
    pub fn set_window_settings(&mut self, win: WidgetId, s: WindowSettings) {
        self.with_widget::<Window, _>(win, |w, cx| w.set_settings(cx, s));
    }

    /// The rect a window would get from `resize_to_fit_content`. TB: GetResizeToFitContentRect
    pub fn fit_content_rect(&mut self, win: WidgetId, fit: ResizeFit) -> Rect {
        let ps = self.preferred_size(win, SizeConstraints::NONE);
        let r = self.rect(win);
        let (mut w, mut h) = match fit {
            ResizeFit::Preferred => (ps.pref.w, ps.pref.h),
            ResizeFit::Minimal => (ps.min.w, ps.min.h),
            ResizeFit::CurrentOrNeeded => {
                (r.w.clamp(ps.min.w, ps.max.w.max(ps.min.w)), r.h.clamp(ps.min.h, ps.max.h.max(ps.min.h)))
            }
        };
        if let Some(p) = self.parent(win) {
            let pr = self.rect(p);
            w = w.min(pr.w);
            h = h.min(pr.h);
        }
        Rect::new(r.x, r.y, w, h)
    }

    /// TB: TBWindow::ResizeToFitContent
    pub fn resize_to_fit_content(&mut self, win: WidgetId, fit: ResizeFit) {
        let r = self.fit_content_rect(win, fit);
        self.set_rect(win, r);
    }

    /// Center a window in its parent.
    pub fn center_in_parent(&mut self, win: WidgetId) {
        let Some(p) = self.parent(win) else { return };
        let pr = Rect::from_size(self.rect(p).size());
        let r = self.rect(win).center_in(&pr);
        self.set_rect(win, r);
    }

    /// Close a window (it fades out, then is removed). TB: TBWindow::Close
    pub fn close_window(&mut self, win: WidgetId) {
        self.die(win);
    }
}
