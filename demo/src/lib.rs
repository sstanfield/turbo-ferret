//! The Turbo Ferret demo: Turbo Badger's demo01 windows, built from its resources.
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author. The demo font is rasterized from a copyrighted
//! font (see resources/default_font/license.txt); it is for the demo only.

pub mod resource_edit;
pub mod sources;

use std::cell::Cell;
use std::rc::Rc;

use turbo_ferret::anim::{ALMOST_ZERO_OPACITY, AnimSpec, Curve};
use turbo_ferret::font::BackgroundFont;
use turbo_ferret::layout::stack::LayoutPosition;
use turbo_ferret::widgets::{
    Button, EditField, ImageWidget, Layout, MessageButtons, MessageSettings, PopupAlignment, ResizeFit,
    ScrollContainer, ScrollMode, SelectDropdown, SelectList, SkinImage, SourceRef, StringItem, StringItemSource,
    TabContainer, TextAlign, Window,
};
use turbo_ferret::*;

use sources::Sources;

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
        let me = cx.this();
        let Some(win) = cx.parent_window(me) else { return };
        if msg.id == tbid!("new button") {
            // data: [the parent's id, the button number]
            if let Some([parent, i]) = msg.data.as_array() {
                let root = cx.root();
                if let Some(target) = cx.find(root, TbId(parent.as_int() as u32)) {
                    add_remove_button(cx, target, i.as_int());
                }
            }
        } else if msg.id == tbid!("instantmsg") {
            message(cx, win, tbid!("test_dialog"), "Message window", "Instant message received!");
        } else if msg.id == tbid!("busy") {
            // Keep the message queue busy by posting another "busy" message.
            cx.post_message(me, tbid!("busy"), Value::Null);
        } else if msg.id == tbid!("delayedmsg") {
            // data: the intended fire time
            let late = cx.now() as i64 - msg.data.as_int() as i64;
            let text = format!("Delayed message received!\n\nIt was received {late} ms after its intended fire time.");
            message(cx, win, TbId::NONE, "Message window", &text);
        }
    }
}

/// Keeps the text window's undo and redo buttons and its caret info up to date.
/// TB: EditWindow::OnProcessStates
#[derive(Default)]
struct EditWindowState;

impl Widget for EditWindowState {
    fn type_name(&self) -> &'static str {
        "EditWindowState"
    }
    fn on_process(&mut self, cx: &mut Ctx) {
        let Some(win) = cx.parent_window(cx.this()) else { return };
        let Some(edit) = cx.find_as::<EditField>(win, tbid!("editfield")) else { return };
        let Some((can_undo, can_redo, caret)) =
            cx.get::<EditField>(edit).map(|e| (e.can_undo(), e.can_redo(), e.style_edit().caret_global_ofs()))
        else {
            return;
        };
        if let Some(b) = cx.find(win, tbid!("undo")) {
            cx.set_state(b, WidgetState::DISABLED, !can_undo);
        }
        if let Some(b) = cx.find(win, tbid!("redo")) {
            cx.set_state(b, WidgetState::DISABLED, !can_redo);
        }
        if let Some(info) = cx.find(win, tbid!("info")) {
            let text = format!("Caret ofs: {caret}");
            if cx.text(info) != Some(text.as_str()) {
                cx.set_text(info, &text);
            }
        }
    }
}

/// Draws the frame number (and the frame rate while "Continuous repaint" is on)
/// in the corner of the root. TB: DemoApplication::RenderFrame
#[derive(Default)]
struct FrameCounter {
    continuous: bool,
    /// Frames painted in all, frames since `since`, and the last rate worked out.
    total: Cell<u32>,
    frames: Cell<u32>,
    since: Cell<u64>,
    fps: Cell<u32>,
}

impl Widget for FrameCounter {
    fn type_name(&self) -> &'static str {
        "FrameCounter"
    }

    /// Cover the root.
    fn on_process(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        if let Some(p) = cx.parent(me) {
            let size = cx.rect(p).size();
            cx.set_rect(me, Rect::from_size(size));
        }
    }

    fn paint(&self, p: &mut PaintCtx) {
        self.total.set(self.total.get() + 1);
        self.frames.set(self.frames.get() + 1);
        // Update the FPS counter
        let (now, since) = (p.now(), self.since.get());
        if now > since + 1000 {
            self.fps.set((u64::from(self.frames.get()) * 1000 / (now - since)) as u32);
            self.since.set(now);
            self.frames.set(0);
        }
        let text = if self.continuous {
            format!("FPS: {} Frame {}", self.fps.get(), self.total.get())
        } else {
            format!("Frame {}", self.total.get())
        };
        p.draw_text(Point::new(5, 5), Color::rgb(255, 255, 255), &text);
    }
}

/// The text window's menu. TB: EditWindow::OnEvent ("menu")
fn text_menu_source() -> SourceRef {
    let source = StringItemSource::shared();
    {
        let mut s = source.borrow_mut();
        for (text, id) in [
            ("Default font", "default font"),
            ("Default font (larger)", "large font"),
            ("RGB font (Neon)", "rgb font Neon"),
            ("RGB font (Orangutang)", "rgb font Orangutang"),
            ("RGB font (Orange)", "rgb font Orange"),
            // Not in TB's menu: its demo is built with the bitmap font renderer only.
            ("TrueType font (DejaVu Sans)", "ttf font DejaVu"),
            ("TrueType font with a glow", "ttf font DejaVuGlow"),
            ("-", ""),
            ("Glyph cache stresstest (CJK)", "CJK"),
            ("-", ""),
            ("Toggle wrapping", "toggle wrapping"),
            ("-", ""),
            ("Align left", "align left"),
            ("Align center", "align center"),
            ("Align right", "align right"),
        ] {
            s.add(StringItem::with_id(text, TbId::new(id)));
        }
    }
    source
}

/// A choice in the text window's menu. TB: EditWindow::OnEvent ("popup_menu")
fn text_menu_choice(ui: &mut Ui, edit: WidgetId, choice: TbId) {
    let with_font_id = |ui: &mut Ui, name: &str| {
        let mut fd = ui.calculated_font(edit);
        fd.id = TbId::new(name);
        ui.set_font(edit, Some(fd));
    };
    if choice == tbid!("default font") {
        ui.set_font(edit, None);
    } else if choice == tbid!("large font") {
        let mut fd = ui.default_font();
        fd.size = ui.dimension_converter().dp_to_px(28).clamp(1, 255) as u16;
        ui.set_font(edit, Some(fd));
    } else if choice == tbid!("rgb font Neon") {
        with_font_id(ui, "Neon");
    } else if choice == tbid!("rgb font Orangutang") {
        with_font_id(ui, "Orangutang");
    } else if choice == tbid!("rgb font Orange") {
        with_font_id(ui, "Orange");
    } else if choice == tbid!("ttf font DejaVu") {
        with_font_id(ui, "DejaVu");
    } else if choice == tbid!("ttf font DejaVuGlow") {
        // A face with a blurred copy of itself behind it. Both come from the same
        // file, registered under two names (see `Demo::new`).
        let mut fd = ui.default_font();
        fd.id = tbid!("DejaVuGlowText");
        fd.size = ui.dimension_converter().dp_to_px(20).clamp(1, 255) as u16;
        let glow = FontDesc::new(tbid!("DejaVuGlow"), fd.size);
        ui.create_font_face(fd);
        ui.create_font_face(glow);
        let radius = ui.dimension_converter().dp_to_px(3).clamp(1, 64) as u8;
        ui.fonts_mut().set_blur_radius(glow, radius);
        let background = BackgroundFont { desc: glow, color: Color::rgb(0, 150, 255), offset: Point::ZERO };
        ui.fonts_mut().set_background_font(fd, Some(background));
        ui.set_font(edit, Some(fd));
    } else if choice == tbid!("CJK") {
        let mut text = String::new();
        for (i, cp) in (0x4E00u32..=0x9FCC).enumerate() {
            text.extend(char::from_u32(cp));
            if i % 64 == 63 {
                text.push('\n');
            }
        }
        ui.set_text(edit, &text);
    } else if choice == tbid!("toggle wrapping") {
        ui.with_widget::<EditField, _>(edit, |e, cx| {
            let wrap = !e.is_wrapping();
            e.set_wrapping(cx, wrap);
        });
    } else {
        let align = if choice == tbid!("align left") {
            TextAlign::Left
        } else if choice == tbid!("align center") {
            TextAlign::Center
        } else if choice == tbid!("align right") {
            TextAlign::Right
        } else {
            return;
        };
        ui.with_widget::<EditField, _>(edit, |e, cx| e.set_text_align(cx, align));
    }
}

/// A message window with an Ok button.
fn message(ui: &mut Ui, target: WidgetId, id: TbId, title: &str, text: &str) {
    ui.show_message(target, id, title, text, &MessageSettings::default());
}

/// TB: ScrollContainerWindow ("new buttons")
fn add_remove_button(ui: &mut Ui, parent: WidgetId, i: i32) {
    let b = ui.add(parent, Button::new(&format!("Remove {i}")));
    ui.set_id(b, tbid!("remove button"));
}

/// The row of a list item that `w` is in, with the list and the item's index.
fn list_item_of(ui: &Ui, w: WidgetId) -> Option<(WidgetId, usize)> {
    let mut cur = w;
    while let Some(parent) = ui.parent(cur) {
        if let (Some(list), Value::Int(index)) = (list_of_layout(ui, parent), ui.data(cur)) {
            return usize::try_from(index).ok().map(|i| (list, i));
        }
        cur = parent;
    }
    None
}

/// The `SelectList` whose item layout is `layout`.
fn list_of_layout(ui: &Ui, layout: WidgetId) -> Option<WidgetId> {
    let mut cur = ui.parent(layout);
    while let Some(c) = cur {
        if let Some(l) = ui.get::<SelectList>(c) {
            return (l.item_layout() == Some(layout)).then_some(c);
        }
        cur = ui.parent(c);
    }
    None
}

/// Handles the events of the demo windows (a global listener sees every event before
/// it is dispatched). TB: the OnEvent of MainWindow, ScrollContainerWindow,
/// TabContainerWindow, ConnectionWindow, AnimationsWindow, AdvancedListWindow and
/// AdvancedItemWidget
struct DemoListener {
    tab_axis: Axis,
    /// The axis the layout test window's "Toggle axis" sets next.
    layout_axis: Axis,
    sources: Sources,
}

impl DemoListener {
    /// TB: AdvancedItemWidget::OnEvent and the busy check box
    fn on_changed(&mut self, ui: &mut Ui, target: WidgetId, id: TbId, value: &Value) -> bool {
        let text = value.as_str();
        let value = value.as_int();
        if id == tbid!("check") {
            if let Some((list, index)) = list_item_of(ui, target) {
                self.sources.advanced.borrow_mut().set_checked(index, value != 0, Some(list));
            }
            return false;
        }
        if id == tbid!("select position") {
            // The layout test window: the position of its three layouts.
            // TB: LayoutWindow::OnEvent
            let pos = match value {
                1 => LayoutPosition::LeftTop,
                2 => LayoutPosition::RightBottom,
                3 => LayoutPosition::Gravity,
                _ => LayoutPosition::Center,
            };
            if let Some(win) = ui.parent_window(target) {
                let layouts: Vec<WidgetId> = (1..=3).filter_map(|i| ui.find_as::<Layout>(win, TbId(i))).collect();
                for layout in layouts {
                    ui.with_widget::<Layout, _>(layout, |l, cx| l.configure(cx, |l| l.position = pos));
                }
            }
            return true;
        }
        if id == tbid!("filter") {
            // TB: ListWindow::OnEvent and AdvancedListWindow::OnEvent
            // (The edit field is busy sending this event: its text comes with it.)
            let filter = text.into_owned();
            let list = ui.parent_window(target).and_then(|w| ui.find_as::<SelectList>(w, tbid!("list")));
            if let Some(list) = list {
                ui.with_widget::<SelectList, _>(list, |l, cx| l.set_filter(cx, &filter));
            }
            return true;
        }
        if id == tbid!("busymsg") {
            let root = ui.root();
            let Some(sink) = ui.find_as::<MessageSink>(root, tbid!("msg-sink")) else { return false };
            if value != 0 {
                if !ui.has_message(sink, tbid!("busy")) {
                    ui.post_message(sink, tbid!("busy"), Value::Null);
                    if let Some(win) = ui.parent_window(target) {
                        let text = "The message loop is now constantly busy with messages to process.\n\n\
                                    The main thread should be working hard, but input & animations should still \
                                    be running smoothly.";
                        message(ui, win, tbid!("test_dialog"), "Message window", text);
                    }
                }
            } else {
                ui.cancel_messages(sink, Some(tbid!("busy")));
            }
        }
        false
    }
}

impl UiListener for DemoListener {
    fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool {
        let Some(n) = ui.node(target) else { return false };
        let id = n.id();
        if ev.kind == EventKind::Changed {
            let value = ev.value.clone().unwrap_or(Value::Null);
            return self.on_changed(ui, target, id, &value);
        }
        if ev.kind != EventKind::Click {
            return false;
        }
        let data = n.data().clone();
        let parent = n.parent();
        let win = ui.parent_window(target);
        let root = ui.root();
        let sink = ui.find_as::<MessageSink>(root, tbid!("msg-sink"));
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
        } else if id == tbid!("test-image") {
            open_resource_window(ui, &res("test_image_widget.tb.txt"));
        } else if id == tbid!("remove") && parent.is_some_and(|p| ui.get::<ImageWidget>(p).is_some()) {
            // The image window: the button on an image removes it. Its image is
            // unloaded when no other widget shows it. TB: ImageWindow::OnEvent
            if let Some(image) = parent {
                ui.remove(image);
            }
        } else if id == tbid!("test-scroll-container") {
            open_scroll_container_window(ui, &self.sources);
        } else if id == tbid!("test-list") {
            open_advanced_list_window(ui, &self.sources);
        } else if id == tbid!("test-page") {
            open_page_window(ui);
        } else if id == tbid!("test-resource-edit") {
            resource_edit::open_resource_edit_window(ui, &res("resource_edit_test.tb.txt"));
        } else if id == tbid!("TBWindow.close") && win.is_some_and(|w| ui.find(w, tbid!("msg-sink")).is_some()) {
            // The main window asks before it closes (the click does not reach the
            // window, so it stays open).
            let mut settings = MessageSettings::new(MessageButtons::YesNo, tbid!("Icon48"));
            settings.dimmer = true;
            settings.styling = true;
            let text = "Really <color #0794f8>close</color> the window?";
            ui.show_message(win.unwrap_or(root), tbid!("confirm_close_dialog"), "Are you sure?", text, &settings);
        } else if id == tbid!("confirm_close_dialog") {
            if ev.ref_id == tbid!("TBMessageWindow.yes") {
                // The message window's events go on through its target: the window
                // that asked.
                if let Some(main) = ui.event_destination(target) {
                    ui.close_window(main);
                }
            }
        } else if id == tbid!("reload skin bitmaps") {
            // TB: MainWindow::OnEvent ("reload skin bitmaps")
            let reload_count = 10;
            let start = std::time::Instant::now();
            let mut failed = None;
            for _ in 0..reload_count {
                if let Err(e) = ui.reload_skin_bitmaps() {
                    failed = Some(e);
                }
            }
            let ms = start.elapsed().as_millis();
            let text = match failed {
                None => format!("Reloading the skin graphics {reload_count} times took {ms}ms"),
                Some(e) => format!("Reloading the skin graphics failed: {e}"),
            };
            message(ui, target, TbId::NONE, "GFX load performance", &text);
        } else if id == tbid!("test context lost") {
            ui.renderer_lost();
            let text = "Dropped every bitmap as after a lost context.\n\nDoes everything look fine?";
            message(ui, target, TbId::NONE, "Context lost & restore", text);
        } else if id == tbid!("msg") {
            if let Some(sink) = sink {
                ui.post_message(sink, tbid!("instantmsg"), Value::Null);
            }
        } else if id == tbid!("delayedmsg") {
            if let Some(sink) = sink {
                let at = ui.now() + 2000;
                ui.post_message_delayed(sink, tbid!("delayedmsg"), Value::Int(at as i32), 2000);
            }
        } else if id == tbid!("debug settings") {
            // TB: ShowDebugInfoSettingsWindow
            ui.show_debug_window();
        // == Text window ==
        } else if id == tbid!("clear") || id == tbid!("undo") || id == tbid!("redo") {
            if let Some(edit) = win.and_then(|w| ui.find_as::<EditField>(w, tbid!("editfield"))) {
                if id == tbid!("clear") {
                    ui.set_text(edit, "");
                } else {
                    ui.with_widget::<EditField, _>(edit, |e, cx| e.command(cx, id));
                }
            }
        } else if id == tbid!("menu") {
            ui.show_menu(target, tbid!("popup_menu"), text_menu_source(), &PopupAlignment::default(), -1);
        } else if id == tbid!("popup_menu") {
            // The menu's target is the menu button, in the text window.
            let edit = ui
                .popup_target(target)
                .and_then(|b| ui.parent_window(b))
                .and_then(|w| ui.find_as::<EditField>(w, tbid!("editfield")));
            if let Some(edit) = edit {
                text_menu_choice(ui, edit, ev.ref_id);
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
            let parent_id = parent.and_then(|p| ui.node(p)).map(|p| p.id());
            if let (Some(sink), Some(parent_id)) = (sink, parent_id) {
                for i in 0..data.as_int() {
                    let d = Value::Array(Rc::from([Value::Int(parent_id.0 as i32), Value::Int(i)]));
                    ui.post_message_delayed(sink, tbid!("new button"), d, 100 + i as u32 * 500);
                }
            }
        } else if id == tbid!("remove button") {
            ui.remove(target);
        } else if id == tbid!("showpopupmenu1") {
            let source: SourceRef = self.sources.popup_menu.clone();
            ui.show_menu(target, tbid!("popupmenu1"), source, &PopupAlignment::default(), -1);
        } else if id == tbid!("popupmenu1") {
            // An item of the popup menu (or of one of its sub menus) was chosen.
            let owner = ui.popup_target(target).and_then(|t| ui.parent_window(t)).unwrap_or(root);
            let text = format!("Menu event received!\nref_id: {}", ev.ref_id.0);
            message(ui, owner, tbid!("popup_dialog"), "Info", &text);
        // == Customized list window ==
        } else if id == tbid!("delete") {
            if let Some((_, index)) = list_item_of(ui, target) {
                self.sources.advanced.borrow_mut().remove(index);
            }
        } else if id == tbid!("add") {
            let name = win.and_then(|w| ui.text_by_name(w, tbid!("add_name"))).unwrap_or("").to_string();
            if !name.is_empty() {
                self.sources.advanced.borrow_mut().add(&name, tbid!("boy_item"), true);
            }
        } else if id == tbid!("delete all") {
            self.sources.advanced.borrow_mut().clear();
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
        } else if id == tbid!("toggle axis") {
            // The layout test window: its three layouts get the axis, and the layout
            // that holds them the other one. TB: LayoutWindow::OnEvent
            if let Some(w) = win {
                let axis = self.layout_axis;
                let set = |ui: &mut Ui, layout: WidgetId, axis: Axis| {
                    ui.with_widget::<Layout, _>(layout, |l, cx| l.configure(cx, |l| l.axis = axis));
                };
                let layouts: Vec<WidgetId> = (1..=3).filter_map(|i| ui.find_as::<Layout>(w, TbId(i))).collect();
                for layout in layouts {
                    set(ui, layout, axis);
                }
                self.layout_axis = axis.other();
                if let Some(outer) = ui.find_as::<Layout>(w, tbid!("switch_layout")) {
                    set(ui, outer, self.layout_axis);
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

/// Styled, editable text with undo, fonts and alignment. TB: EditWindow
pub fn open_text_window(ui: &mut Ui) -> Option<WidgetId> {
    let w = open_resource_window(ui, "demo01/ui_resources/test_textwindow.tb.txt")?;
    let state = ui.add(w, EditWindowState);
    ui.set_visibility(state, Visibility::Gone);
    Some(w)
}

/// A list of names with a filter field. TB: ListWindow
pub fn open_list_window(ui: &mut Ui, sources: &Sources) -> Option<WidgetId> {
    let w = open_resource_window(ui, "demo01/ui_resources/test_select.tb.txt")?;
    if let Some(list) = ui.find_as::<SelectList>(w, tbid!("list")) {
        let source: SourceRef = sources.names.clone();
        ui.with_widget::<SelectList, _>(list, |l, cx| l.set_source(cx, Some(source)));
    }
    Some(w)
}

/// A list with customized item widgets. TB: AdvancedListWindow
pub fn open_advanced_list_window(ui: &mut Ui, sources: &Sources) -> Option<WidgetId> {
    let w = open_resource_window(ui, "demo01/ui_resources/test_select_advanced.tb.txt")?;
    if let Some(list) = ui.find_as::<SelectList>(w, tbid!("list")) {
        let source: SourceRef = sources.advanced.clone();
        ui.with_widget::<SelectList, _>(list, |l, cx| l.set_source(cx, Some(source)));
        if let Some(sc) = ui.get::<SelectList>(list).and_then(|l| l.scroll_container()) {
            ui.with_widget::<ScrollContainer, _>(sc, |c, cx| c.set_scroll_mode(cx, ScrollMode::Auto));
        }
    }
    Some(w)
}

/// A scroll container with many kinds of widgets. TB: ScrollContainerWindow
pub fn open_scroll_container_window(ui: &mut Ui, sources: &Sources) -> Option<WidgetId> {
    let w = open_resource_window(ui, "demo01/ui_resources/test_scrollcontainer.tb.txt")?;
    if let Some(d) = ui.find_as::<SelectDropdown>(w, tbid!("name dropdown")) {
        let source: SourceRef = sources.names.clone();
        ui.with_widget::<SelectDropdown, _>(d, |d, cx| d.set_source(cx, Some(source)));
    }
    if let Some(d) = ui.find_as::<SelectDropdown>(w, tbid!("advanced dropdown")) {
        let source: SourceRef = sources.advanced.clone();
        ui.with_widget::<SelectDropdown, _>(d, |d, cx| d.set_source(cx, Some(source)));
    }
    Some(w)
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
    pub sources: Sources,
    frame_counter: WidgetId,
    /// The mouse button went down as a finger (see `finger_down`).
    touching: bool,
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
        // TrueType: DejaVu Sans (the font TB ships as vera.ttf), under three names so the text
        // window can show it plain and with a blurred copy behind it.
        // TB: Demo01.cpp (register_stb_font_renderer, AddFontInfo("resources/vera.ttf", "Vera"))
        ui.fonts_mut().add_backend(Box::new(turbo_ferret_ttf::TtfBackend::new()));
        for name in ["DejaVu", "DejaVuGlowText", "DejaVuGlow"] {
            ui.add_font(name, "DejaVuSans.ttf");
        }
        let size = ui.dimension_converter().dp_to_px(14).clamp(1, 255) as u16;
        if !ui.set_default_font(FontDesc::new(tbid!("Segoe"), size)) {
            eprintln!("demo: could not load the default font");
        }
        ui.set_viewport(viewport);
        // The demo background (a tile from the demo skin).
        let root = ui.root();
        ui.set_skin(root, tbid!("background"));
        let frame_counter = ui.add(root, FrameCounter::default());
        ui.set_ignore_input(frame_counter, true);
        let sources = Sources::new();
        ui.add_listener(Box::new(DemoListener { tab_axis: Axis::X, layout_axis: Axis::Y, sources: sources.clone() }));
        ui.create_bound_value(tbid!("continous-repaint"), ValueKind::Int);
        let main = open_main_window(&mut ui);
        // TB's demo starts with these windows open too.
        open_text_window(&mut ui);
        open_list_window(&mut ui, &sources);
        open_advanced_list_window(&mut ui, &sources);
        open_resource_window(&mut ui, "demo01/ui_resources/test_tabcontainer01.tb.txt");
        if let Some(m) = main {
            ui.activate_window(m);
        }
        Demo { ui, main, sources, frame_counter, touching: false }
    }

    /// One frame of host work: time, update, and whether to repaint.
    pub fn frame(&mut self, now_ms: u64) -> bool {
        self.ui.set_time(now_ms);
        self.ui.update();
        let continuous = self.ui.bound_value(tbid!("continous-repaint")).is_some_and(|v| v.as_int() != 0);
        if continuous {
            self.ui.invalidate();
        }
        self.ui.with_widget::<FrameCounter, _>(self.frame_counter, |c, _| c.continuous = continuous);
        self.ui.take_repaint()
    }

    /// TB's demo treats the mouse as a finger while shift, ctrl or alt is held
    /// down, to try touch input without a touch screen. TB: port_glfw.cpp
    /// ShouldEmulateTouchEvent
    pub fn emulates_touch(mods: Modifiers) -> bool {
        mods.intersects(Modifiers::SHIFT.union(Modifiers::CTRL).union(Modifiers::ALT))
    }

    /// The mouse button went down while `emulates_touch`: a finger touches.
    pub fn finger_down(&mut self, p: Point, clicks: u32, mods: Modifiers) -> bool {
        self.touching = true;
        self.ui.touch_down(0, p, clicks, mods)
    }

    /// The mouse moved. True if this was handled as a finger: it moves while it is
    /// down, and a finger that is not down does not hover.
    pub fn finger_move(&mut self, p: Point, mods: Modifiers) -> bool {
        if self.touching {
            self.ui.touch_move(0, p, mods);
            true
        } else {
            Demo::emulates_touch(mods)
        }
    }

    /// The mouse button went up. True if it had gone down as a finger.
    pub fn finger_up(&mut self, p: Point, mods: Modifiers) -> bool {
        if !self.touching {
            return false;
        }
        self.touching = false;
        self.ui.touch_up(0, p, mods);
        true
    }
}
