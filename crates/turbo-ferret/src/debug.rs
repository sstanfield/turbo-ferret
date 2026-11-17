//! Runtime debugging aids (feature `debug`): overlays drawn by the paint walk, and a
//! window to switch them on and to watch events go by.
//!
//! TB: tb_debug.h, tb_debug.cpp (TBDebugInfo, DebugSettingsWindow), compiled in with
//! `TB_RUNTIME_DEBUG_INFO`. TB kept the settings in a global; here they belong to
//! the `Ui`.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;

use crate::arena::WidgetId;
use crate::event::{Event, EventKind};
use crate::geom::{Point, Rect};
use crate::id::TbId;
use crate::msg::Millis;
use crate::paint::PaintCtx;
use crate::ui::{Ctx, ListenerId, Ui, UiListener};
use crate::value::Value;
use crate::widget::{DieAction, Widget};
use crate::widgets::window::forward_to_window;
use crate::widgets::{ResizeFit, Window};

/// What to show. TB: TBDebugInfo::SETTING
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugSettings {
    /// Outline every widget. TB: LAYOUT_BOUNDS
    pub layout_bounds: bool,
    /// Outline the area widgets clip their children to. TB: LAYOUT_CLIPPING
    pub layout_clipping: bool,
    /// Flash widgets that measure their preferred size (yellow) and layouts that
    /// arrange their children (red). TB: LAYOUT_PS_DEBUGGING
    pub layout_size_calculation: bool,
    /// Tint every render batch in a color of its own, and count batches and
    /// triangles (see [`Ui::debug_frame_stats`]). TB: RENDER_BATCHES
    pub render_batches: bool,
    /// Show the skin's atlas pages next to the debug window.
    /// TB: RENDER_SKIN_BITMAP_FRAGMENTS
    pub skin_bitmap_fragments: bool,
    /// Show the glyph cache's pages next to the debug window.
    /// TB: RENDER_FONT_BITMAP_FRAGMENTS
    pub font_bitmap_fragments: bool,
}

/// What a frame cost, counted while [`DebugSettings::render_batches`] is on.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Draw calls.
    pub batches: u32,
    pub triangles: u32,
}

/// How long a measured or arranged widget stays highlighted.
pub(crate) const HIGHLIGHT_MS: Millis = 300;

/// Most lines the event log keeps.
const LOG_LINES: usize = 200;

/// The debug state of a `Ui`.
#[derive(Default)]
pub(crate) struct DebugState {
    pub settings: DebugSettings,
    /// When widgets last measured and arranged (while `layout_size_calculation` is on).
    pub measured: BTreeMap<WidgetId, Millis>,
    pub arranged: BTreeMap<WidgetId, Millis>,
    /// The strings ids were made from. TB: TBID::debug_string
    pub id_names: BTreeMap<TbId, String>,
    pub frame: FrameStats,
}

impl DebugState {
    /// Forget highlights that have run out.
    pub fn prune(&mut self, now: Millis) {
        self.measured.retain(|_, t| now < *t + HIGHLIGHT_MS);
        self.arranged.retain(|_, t| now < *t + HIGHLIGHT_MS);
    }
}

impl Ui {
    /// TB: g_tb_debug.settings
    pub fn debug_settings(&self) -> DebugSettings {
        self.debug.settings
    }

    /// Change what is shown, and repaint.
    pub fn set_debug_settings(&mut self, settings: DebugSettings) {
        if !settings.layout_size_calculation {
            self.debug.measured.clear();
            self.debug.arranged.clear();
        }
        self.debug.settings = settings;
        self.repaint = true;
    }

    /// Batches and triangles of the last painted frame, if
    /// [`DebugSettings::render_batches`] was on for it (zero otherwise). The tint
    /// drawn over each batch is not counted.
    pub fn debug_frame_stats(&self) -> FrameStats {
        self.debug.frame
    }

    pub(crate) fn debug_mark_measured(&mut self, id: WidgetId) {
        if self.debug.settings.layout_size_calculation {
            self.debug.measured.insert(id, self.now);
        }
    }

    pub(crate) fn debug_mark_arranged(&mut self, id: WidgetId) {
        if self.debug.settings.layout_size_calculation {
            self.debug.arranged.insert(id, self.now);
        }
    }

    /// Open a window with a check box for each debug setting and a log of the
    /// events that are dispatched (pointer events left out). It is centered in the
    /// root. TB: ShowDebugInfoSettingsWindow
    pub fn show_debug_window(&mut self) -> WidgetId {
        let root = self.root();
        let win = self.add(root, DebugWindow::default());
        self.set_text(win, "Debug settings");
        if let Err(e) = self.inflate(win, LAYOUT) {
            self.warn(format_args!("debug window: {e}"));
        }
        let s = self.debug.settings;
        for (id, on) in checks(&s) {
            if let Some(check) = self.find(win, id) {
                self.set_value(check, Value::Int(on as i32));
            }
        }
        let output = self.find(win, crate::tbid!("output"));
        let listener = self.add_listener(Box::new(EventLog { window: win, output, lines: VecDeque::new() }));
        self.with_widget::<DebugWindow, _>(win, |w, _| w.listener = Some(listener));

        let bounds = Rect::from_size(self.rect(root).size());
        let r = self.fit_content_rect(win, ResizeFit::Preferred);
        self.set_rect(win, r.center_in(&bounds).move_in(&bounds).clip(&bounds));
        win
    }
}

// TB: DebugSettingsWindow::DebugSettingsWindow (the layout and AddCheckbox)
const LAYOUT: &str = "TBLayout: axis: y, distribution: available, position: left\n\
\tTBLayout: id: 'container', axis: y, size: available\n\
\t\tTBClickLabel: text: 'Layout bounds'\n\t\t\tTBCheckBox: id: 'debug.layout_bounds'\n\
\t\tTBClickLabel: text: 'Layout clipping'\n\t\t\tTBCheckBox: id: 'debug.layout_clipping'\n\
\t\tTBClickLabel: text: 'Layout size calculation'\n\t\t\tTBCheckBox: id: 'debug.layout_size_calculation'\n\
\t\tTBClickLabel: text: 'Render batches'\n\t\t\tTBCheckBox: id: 'debug.render_batches'\n\
\t\tTBClickLabel: text: 'Render skin bitmap fragments'\n\t\t\tTBCheckBox: id: 'debug.skin_bitmap_fragments'\n\
\t\tTBClickLabel: text: 'Render font bitmap fragments'\n\t\t\tTBCheckBox: id: 'debug.font_bitmap_fragments'\n\
\tTBTextField: text: 'Event output:'\n\
\tTBEditField: id: 'output', gravity: all, multiline: 1, wrap: 0, readonly: 1\n\
\t\tlp: pref-height: 100dp\n";

/// The check box id of each setting, with its value.
fn checks(s: &DebugSettings) -> [(TbId, bool); 6] {
    [
        (crate::tbid!("debug.layout_bounds"), s.layout_bounds),
        (crate::tbid!("debug.layout_clipping"), s.layout_clipping),
        (crate::tbid!("debug.layout_size_calculation"), s.layout_size_calculation),
        (crate::tbid!("debug.render_batches"), s.render_batches),
        (crate::tbid!("debug.skin_bitmap_fragments"), s.skin_bitmap_fragments),
        (crate::tbid!("debug.font_bitmap_fragments"), s.font_bitmap_fragments),
    ]
}

/// The window opened by [`Ui::show_debug_window`]. TB: DebugSettingsWindow
#[derive(Default)]
pub struct DebugWindow {
    window: Window,
    listener: Option<ListenerId>,
}

impl Widget for DebugWindow {
    fn type_name(&self) -> &'static str {
        "DebugWindow"
    }

    fn on_create(&mut self, cx: &mut Ctx) {
        self.window.on_create(cx);
    }

    /// TB: DebugSettingsWindow::~DebugSettingsWindow
    fn on_removed(&mut self, cx: &mut Ctx) {
        if let Some(l) = self.listener.take() {
            cx.remove_listener(l);
        }
    }

    fn on_free(&mut self, ui: &mut Ui) {
        if let Some(l) = self.listener.take() {
            ui.remove_listener(l);
        }
    }

    /// TB: DebugSettingsWindow::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if ev.kind == EventKind::Changed {
            let id = cx.node(ev.target).map_or(TbId::NONE, |n| n.id());
            let on = ev.value.as_ref().is_some_and(|v| v.as_int() != 0);
            let mut s = cx.debug_settings();
            let field = match () {
                _ if id == crate::tbid!("debug.layout_bounds") => &mut s.layout_bounds,
                _ if id == crate::tbid!("debug.layout_clipping") => &mut s.layout_clipping,
                _ if id == crate::tbid!("debug.layout_size_calculation") => &mut s.layout_size_calculation,
                _ if id == crate::tbid!("debug.render_batches") => &mut s.render_batches,
                _ if id == crate::tbid!("debug.skin_bitmap_fragments") => &mut s.skin_bitmap_fragments,
                _ if id == crate::tbid!("debug.font_bitmap_fragments") => &mut s.font_bitmap_fragments,
                _ => return self.window.on_event(cx, ev),
            };
            *field = on;
            // Update setting and invalidate
            cx.set_debug_settings(s);
            return true;
        }
        self.window.on_event(cx, ev)
    }

    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        self.window.on_die(cx)
    }

    /// Draw the atlas pages to the right of the window. Each atlas starts at the
    /// window's edge, so with both settings on the glyph cache is drawn over the
    /// skin (as in TB). TB: DebugSettingsWindow::OnPaint, TBBitmapFragmentManager::Debug
    fn paint(&self, p: &mut PaintCtx) {
        let s = p.debug.settings;
        let mut pages = alloc::vec::Vec::new();
        let mut collect = |atlas: &crate::render::atlas::Atlas| {
            let mut x = p.rect().w;
            for page in 0..atlas.page_count() as u16 {
                let size = atlas.page_size(page);
                if let Some(bitmap) = atlas.bitmap(page) {
                    pages.push((bitmap, Rect::from_origin_size(Point::new(x, 0), size)));
                }
                x += size.w + 5;
            }
        };
        if s.skin_bitmap_fragments {
            collect(p.skin().atlas());
        }
        if s.font_bitmap_fragments {
            collect(p.fonts().glyph_cache().atlas());
        }
        for (bitmap, dst) in pages {
            p.draw_bitmap(dst, Rect::from_size(dst.size()), bitmap, crate::geom::Color::WHITE);
        }
    }

    forward_to_window!(window);
}

/// Writes the events that go by into the debug window's output field.
/// TB: DebugSettingsWindow::OnWidgetInvokeEvent
struct EventLog {
    window: WidgetId,
    output: Option<WidgetId>,
    lines: VecDeque<String>,
}

impl UiListener for EventLog {
    fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool {
        // Skip these events for now
        if ev.is_pointer() {
            return false;
        }
        // Always ignore activity in this window
        if target == self.window || ui.is_ancestor_of(self.window, target) {
            return false;
        }
        let Some(output) = self.output.filter(|&o| ui.exists(o)) else {
            return false;
        };
        let type_name = ui.node(target).map_or("?", |n| n.type_name());
        let id = ui.node(ev.target).map_or(TbId::NONE, |n| n.id());
        let mut line = format!("{} ({type_name}) id: {}", event_name(ev.kind), id_string(ui, id));
        if ev.ref_id.is_some() {
            line.push_str(&format!(", ref_id: {}", id_string(ui, ev.ref_id)));
        }
        if ev.kind == EventKind::Changed {
            if let Some(v) = &ev.value {
                let mut text = v.as_str().into_owned();
                if text.chars().count() > 24 {
                    text = text.chars().take(20).chain("...".chars()).collect();
                }
                line.push_str(&format!(", value: {:.2} (\"{}\")", v.as_float(), text.replace('\n', " ")));
            }
        }
        // Remove lines from the top if there are too many.
        self.lines.push_back(line);
        while self.lines.len() > LOG_LINES {
            self.lines.pop_front();
        }
        let text = self.lines.iter().fold(String::new(), |mut all, l| {
            all.push_str(l);
            all.push('\n');
            all
        });
        ui.set_text(output, &text);
        ui.scroll_to(output, Point::new(0, i32::MAX / 2));
        false
    }
}

/// TB: DebugSettingsWindow::GetIDString
fn id_string(ui: &Ui, id: TbId) -> String {
    match ui.id_name(id) {
        Some(name) => format!("\"{name}\""),
        None => format!("{}", id.0),
    }
}

/// TB: DebugSettingsWindow::GetEventTypeStr
fn event_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Click => "CLICK",
        EventKind::LongClick => "LONG_CLICK",
        EventKind::PointerDown => "POINTER_DOWN",
        EventKind::PointerUp => "POINTER_UP",
        EventKind::PointerMove => "POINTER_MOVE",
        EventKind::TouchDown => "TOUCH_DOWN",
        EventKind::TouchUp => "TOUCH_UP",
        EventKind::TouchMove => "TOUCH_MOVE",
        EventKind::TouchCancel => "TOUCH_CANCEL",
        EventKind::Wheel => "WHEEL",
        EventKind::Changed => "CHANGED",
        EventKind::KeyDown => "KEY_DOWN",
        EventKind::KeyUp => "KEY_UP",
        EventKind::Shortcut => "SHORT_CUT",
        EventKind::ContextMenu => "CONTEXT_MENU",
        EventKind::FileDrop => "FILE_DROP",
        EventKind::Custom(_) => "CUSTOM",
    }
}
