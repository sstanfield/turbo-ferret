//! The Turbo Ferret demo: Turbo Badger's demo01 windows, built from its resources.
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author. The demo font is rasterized from a copyrighted
//! font (see resources/default_font/license.txt); it is for the demo only.

use turbo_ferret::anim::{ALMOST_ZERO_OPACITY, AnimSpec, Curve};
use turbo_ferret::widgets::{ResizeFit, Window};
use turbo_ferret::*;

/// Where the demo's resources live (the repository's `resources/` folder).
pub fn default_resource_root() -> String {
    format!("{}/../resources", env!("CARGO_MANIFEST_DIR"))
}

/// Receives the demo's messages and shows them (an app widget: messages are
/// delivered to widgets only).
#[derive(Default)]
struct MessageSink;

impl Widget for MessageSink {
    fn type_name(&self) -> &'static str {
        "MessageSink"
    }
    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {
        let text = if msg.id == tbid!("instantmsg") {
            "Got an instant message".to_string()
        } else {
            format!("Got a delayed message at {} ms", cx.now())
        };
        let root = cx.root();
        if let Some(status) = cx.find(root, tbid!("msg-status")) {
            cx.set_text(status, &text);
        }
    }
}

/// Handles clicks in the demo windows (a global listener sees every event before
/// it is dispatched).
struct DemoListener;

impl UiListener for DemoListener {
    fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool {
        if ev.kind != EventKind::Click {
            return false;
        }
        let Some(n) = ui.node(target) else { return false };
        let id = n.id();
        let data = n.data().as_str().into_owned();
        let win = ui.parent_window(target);
        if id == tbid!("test-layout") {
            open_resource_window(ui, &format!("demo01/ui_resources/{data}"));
            return true;
        }
        if id == tbid!("test-skin-conditions") {
            open_resource_window(ui, "demo01/ui_resources/test_skin_conditions01.tb.txt");
            open_resource_window(ui, "demo01/ui_resources/test_skin_conditions02.tb.txt");
            return true;
        }
        if id == tbid!("test-animations") {
            open_resource_window(ui, "demo01/ui_resources/test_animations.tb.txt");
            return true;
        }
        if id == tbid!("Animate!") {
            if let Some(w) = win {
                animate_window(ui, w);
            }
            return true;
        }
        if id == tbid!("new") {
            open_main_window(ui);
            return true;
        }
        if id == tbid!("close-main") {
            if let Some(w) = win {
                ui.close_window(w);
            }
            return true;
        }
        if id == tbid!("test context lost") {
            ui.renderer_lost();
            return true;
        }
        if id == tbid!("msg") || id == tbid!("delayedmsg") {
            let root = ui.root();
            if let Some(sink) = ui.find_as::<MessageSink>(root, tbid!("msg-sink")) {
                if id == tbid!("msg") {
                    ui.post_message(sink, tbid!("instantmsg"), Value::Null);
                } else {
                    ui.post_message_delayed(sink, tbid!("delayedmsg"), Value::Null, 2000);
                    if let Some(status) = ui.find(root, tbid!("msg-status")) {
                        ui.set_text(status, "Delayed message posted (2 s)...");
                    }
                }
            }
            return true;
        }
        false
    }
}

/// Slide a window in from the left (and fade it in). TB: AnimationsWindow::Animate
fn animate_window(ui: &mut Ui, w: WidgetId) {
    ui.abort_animations_for(w);
    let r = ui.rect(w);
    let from = r.offset(-r.x - r.w, 0);
    ui.animate(AnimSpec::rect(w, from, r).curve(Curve::SlowDown).duration(500));
    ui.animate(AnimSpec::opacity(w, ALMOST_ZERO_OPACITY, 1.0).curve(Curve::SlowDown).duration(500));
}

/// Create a window and fill it from a resource file, using its `WindowInfo` for the
/// title, size and position. TB: DemoWindow::LoadResourceFile
pub fn open_resource_window(ui: &mut Ui, path: &str) -> Option<WidgetId> {
    let node = match ui.load_resource(path) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("demo: {e}");
            return None;
        }
    };
    let root = ui.root();
    let w = ui.add(root, Window::default());
    if let Err(e) = ui.inflate_node(w, &node) {
        eprintln!("demo: {path}: {e}");
    }
    let title = node.str("WindowInfo>title", "").into_owned();
    ui.set_text(w, &title);
    let parent = Rect::from_size(ui.rect(root).size());
    let mut r = ui.fit_content_rect(w, ResizeFit::Preferred);
    let dc = ui.dimension_converter().clone();
    if let Some(a) = node.get("WindowInfo>size").and_then(|n| n.value.as_array().map(|a| a.to_vec())) {
        if a.len() == 2 {
            r.w = dc.px_from_value(&a[0], r.w);
            r.h = dc.px_from_value(&a[1], r.h);
        }
    }
    match node.get("WindowInfo>position").and_then(|n| n.value.as_array().map(|a| a.to_vec())) {
        Some(a) if a.len() == 2 => {
            r.x = dc.px_from_value(&a[0], r.x);
            r.y = dc.px_from_value(&a[1], r.y);
        }
        _ => r = r.center_in(&parent),
    }
    let r = r.move_in(&parent).clip(&parent);
    ui.set_rect(w, r);
    ui.ensure_focus(w);
    Some(w)
}

/// The demo's main window (with its message sink).
pub fn open_main_window(ui: &mut Ui) -> Option<WidgetId> {
    let w = open_resource_window(ui, "demo/main_window.tb.txt")?;
    let sink = ui.add(w, MessageSink);
    ui.set_id(sink, tbid!("msg-sink"));
    ui.set_visibility(sink, Visibility::Gone);
    Some(w)
}

/// The demo state: a `Ui` with the skin, fonts and language loaded and the main
/// window open.
pub struct Demo {
    pub ui: Ui,
    pub main: Option<WidgetId>,
}

impl Demo {
    /// TB: Demo01.cpp DemoApplication::Init
    pub fn new(resource_root: &str, viewport: Size, dpi: i32) -> Demo {
        let mut ui = Ui::new(Config::for_dpi(dpi), Services::with_loader(FsLoader::new(resource_root)));
        if let Err(e) = ui.load_language("language/lng_en.tb.txt") {
            eprintln!("demo: language: {e}");
        }
        if let Err(e) = ui.load_skin("default_skin/skin.tb.txt", Some("demo01/skin/skin.tb.txt")) {
            eprintln!("demo: skin: {e}");
        }
        ui.add_font("Segoe", "default_font/segoe_white_with_shadow.tb.txt");
        ui.add_font("Neon", "demo_fonts/neon.tb.txt");
        ui.add_font("Orangutang", "demo_fonts/orangutang.tb.txt");
        ui.add_font("Orange", "demo_fonts/orange.tb.txt");
        let size = ui.dimension_converter().dp_to_px(14).clamp(1, 255) as u16;
        if !ui.set_default_font(FontDesc::new(tbid!("Segoe"), size)) {
            eprintln!("demo: could not load the default font");
        }
        ui.set_viewport(viewport);
        // The demo background (a tile from the demo skin).
        let root = ui.root();
        ui.set_skin(root, tbid!("background"));
        ui.add_listener(Box::new(DemoListener));
        ui.create_bound_value(tbid!("continous-repaint"), ValueKind::Int);
        let main = open_main_window(&mut ui);
        Demo { ui, main }
    }

    /// One frame of host work: time, update, and whether to repaint.
    pub fn frame(&mut self, now_ms: u64) -> bool {
        self.ui.set_time(now_ms);
        self.ui.update();
        if self.ui.bound_value(tbid!("continous-repaint")).is_some_and(|v| v.as_int() != 0) {
            self.ui.invalidate();
        }
        self.ui.take_repaint()
    }
}
