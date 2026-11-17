//! The Turbo Ferret demo: Turbo Badger's demo01 windows, built from its resources.
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author. The demo font is rasterized from a copyrighted
//! font (see resources/default_font/license.txt); it is for the demo only.

use std::rc::Rc;

use turbo_ferret::anim::{ALMOST_ZERO_OPACITY, AnimSpec, Curve};
use turbo_ferret::widgets::{Button, ResizeFit, SkinImage, TabContainer, Window};
use turbo_ferret::*;

/// Where the demo's resources live (the repository's `resources/` folder).
pub fn default_resource_root() -> String {
    format!("{}/../resources", env!("CARGO_MANIFEST_DIR"))
}

/// Receives the demo's messages (an app widget: messages are delivered to widgets
/// only). It lives in the main window. TB: MainWindow::OnMessageReceived and
/// ScrollContainerWindow::OnMessageReceived
#[derive(Default)]
struct MessageSink;

impl Widget for MessageSink {
    fn type_name(&self) -> &'static str {
        "MessageSink"
    }
    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {
        if msg.id == tbid!("new button") {
            // data: [the parent's id, the button number]
            if let Some([parent, i]) = msg.data.as_array() {
                let root = cx.root();
                if let Some(target) = cx.find(root, TbId(parent.as_int() as u32)) {
                    add_remove_button(cx, target, i.as_int());
                }
            }
            return;
        }
        let text = if msg.id == tbid!("instantmsg") {
            "Got an instant message".to_string()
        } else {
            format!("Got a delayed message at {} ms", cx.now())
        };
        show_status(cx, &text);
    }
}

/// Show a status text in the main window's title. (TB opens a TBMessageWindow, which
/// comes with a later milestone.)
fn show_status(ui: &mut Ui, text: &str) {
    let root = ui.root();
    let Some(sink) = ui.find_as::<MessageSink>(root, tbid!("msg-sink")) else { return };
    if let Some(win) = ui.parent_window(sink) {
        ui.set_text(win, &format!("Turbo Ferret - {text}"));
    }
}

/// TB: ScrollContainerWindow ("new buttons")
fn add_remove_button(ui: &mut Ui, parent: WidgetId, i: i32) {
    let b = ui.add(parent, Button::new(&format!("Remove {i}")));
    ui.set_id(b, tbid!("remove button"));
}

/// Handles clicks in the demo windows (a global listener sees every event before
/// it is dispatched). TB: the OnEvent of MainWindow, ScrollContainerWindow,
/// TabContainerWindow, ConnectionWindow and AnimationsWindow
struct DemoListener {
    tab_axis: Axis,
}

impl UiListener for DemoListener {
    fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool {
        if ev.kind != EventKind::Click {
            return false;
        }
        let Some(n) = ui.node(target) else { return false };
        let id = n.id();
        let data = n.data().clone();
        let parent = n.parent();
        let win = ui.parent_window(target);
        let res = |name: &str| format!("demo01/ui_resources/{name}");

        // == Main window ==
        if id == tbid!("test-layout") {
            open_resource_window(ui, &res(&data.as_str()));
        } else if id == tbid!("test-skin-conditions") {
            open_resource_window(ui, &res("test_skin_conditions01.tb.txt"));
            open_resource_window(ui, &res("test_skin_conditions02.tb.txt"));
        } else if id == tbid!("test-animations") {
            open_resource_window(ui, &res("test_animations.tb.txt"));
        } else if id == tbid!("test-connections") {
            open_resource_window(ui, &res("test_connections.tb.txt"));
        } else if id == tbid!("test-scroll-container") {
            open_resource_window(ui, &res("test_scrollcontainer.tb.txt"));
        } else if id == tbid!("test-page") {
            open_page_window(ui);
        } else if id == tbid!("test context lost") || id == tbid!("reload skin bitmaps") {
            ui.renderer_lost();
            show_status(ui, "bitmaps dropped and recreated");
        } else if id == tbid!("msg") || id == tbid!("delayedmsg") {
            let root = ui.root();
            if let Some(sink) = ui.find_as::<MessageSink>(root, tbid!("msg-sink")) {
                if id == tbid!("msg") {
                    ui.post_message(sink, tbid!("instantmsg"), Value::Null);
                } else {
                    ui.post_message_delayed(sink, tbid!("delayedmsg"), Value::Null, 2000);
                    show_status(ui, "delayed message posted (2 s)...");
                }
            }
        // == Animations window ==
        } else if id == tbid!("Animate!") {
            if let Some(w) = win {
                animate_window(ui, w);
            }
        // == Connections window ==
        } else if id == tbid!("reset-master-volume") {
            ui.set_bound_value(tbid!("master-volume"), Value::Int(50));
        } else if id == tbid!("reset-user-name") {
            ui.set_bound_value(tbid!("user-name"), Value::str(""));
        // == Scroll container window ==
        } else if id == tbid!("add img") {
            let content = ui.content_root(target);
            let img = ui.add_at(content, SkinImage, Z::Bottom);
            ui.set_skin(img, tbid!("Icon16"));
        } else if id == tbid!("new buttons") {
            if let Some(p) = parent {
                for i in 0..data.as_int() {
                    add_remove_button(ui, p, i);
                }
            }
        } else if id == tbid!("new buttons delayed") {
            let root = ui.root();
            let sink = ui.find_as::<MessageSink>(root, tbid!("msg-sink"));
            let parent_id = parent.and_then(|p| ui.node(p)).map(|p| p.id());
            if let (Some(sink), Some(parent_id)) = (sink, parent_id) {
                for i in 0..data.as_int() {
                    let d = Value::Array(Rc::from([Value::Int(parent_id.0 as i32), Value::Int(i)]));
                    ui.post_message_delayed(sink, tbid!("new button"), d, 100 + i as u32 * 500);
                }
            }
        } else if id == tbid!("remove button") {
            ui.remove(target);
        // == Tab container window ==
        } else if id == tbid!("set_align") {
            let align = match data.as_int() {
                0 => Align::Left,
                1 => Align::Top,
                2 => Align::Right,
                _ => Align::Bottom,
            };
            if let Some((w, tc)) = win.and_then(|w| Some((w, ui.find_as::<TabContainer>(w, tbid!("tabcontainer"))?))) {
                ui.with_widget::<TabContainer, _>(tc, |t, cx| t.set_alignment(cx, align));
                ui.resize_to_fit_content(w, ResizeFit::CurrentOrNeeded);
            }
            // The radio button still has to take the click.
            return false;
        } else if id == tbid!("toggle_tab_axis") {
            self.tab_axis = self.tab_axis.other();
            let axis = self.tab_axis;
            if let Some((w, tc)) = win.and_then(|w| Some((w, ui.find_as::<TabContainer>(w, tbid!("tabcontainer"))?))) {
                let tabs = ui.get::<TabContainer>(tc).and_then(|t| t.tab_layout());
                for b in tabs.map(|t| ui.children(t).to_vec()).unwrap_or_default() {
                    ui.with_widget::<Button, _>(b, |b, cx| b.set_axis(cx, axis));
                }
                ui.resize_to_fit_content(w, ResizeFit::CurrentOrNeeded);
            }
        } else if id == tbid!("start_spinner") || id == tbid!("stop_spinner") {
            if let Some(sp) = win.and_then(|w| ui.find(w, tbid!("spinner"))) {
                ui.set_value(sp, (id == tbid!("start_spinner")) as i32);
            }
        } else {
            return false;
        }
        true
    }
}

/// A scroll container that snaps to its pages. TB: PageWindow
pub fn open_page_window(ui: &mut Ui) -> Option<WidgetId> {
    let w = open_resource_window(ui, "demo01/ui_resources/test_scroller_snap.tb.txt")?;
    if let Some(pager) = ui.find(w, tbid!("page-scroller")) {
        // TB: PageWindow::OnScrollSnap
        ui.set_scroll_snap(
            pager,
            Some(Rc::new(|ui: &Ui, target, to: Point| {
                let page_w = ui.padding_rect(target).w.max(1);
                Point::new((to.x + page_w / 2) / page_w * page_w, to.y)
            })),
        );
    }
    Some(w)
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
    // WindowInfo is metadata for this function, not a widget.
    let mut content = node.clone();
    content.children.retain(|c| &*c.name != "WindowInfo");
    if let Err(e) = ui.inflate_node(w, &content) {
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

/// The demo's main window: TB's, with the message sink added. TB: MainWindow
pub fn open_main_window(ui: &mut Ui) -> Option<WidgetId> {
    let w = open_resource_window(ui, "demo01/ui_resources/test_ui.tb.txt")?;
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
        ui.add_listener(Box::new(DemoListener { tab_axis: Axis::X }));
        ui.create_bound_value(tbid!("continous-repaint"), ValueKind::Int);
        let main = open_main_window(&mut ui);
        // TB's demo starts with the tab container window open too.
        open_resource_window(&mut ui, "demo01/ui_resources/test_tabcontainer01.tb.txt");
        if let Some(m) = main {
            ui.activate_window(m);
        }
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
