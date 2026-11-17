//! A window that edits a resource file and shows the widgets it makes, live.
//! TB: ResourceEditWindow.cpp
//!
//! TB's window was a TBWindow subclass and a global listener in one. Here a
//! controller widget inside the window keeps the state, paints the selection and
//! receives the update message, and a global listener forwards what it sees.

use std::cell::RefCell;
use std::rc::Rc;

use turbo_ferret::geom::Color;
use turbo_ferret::widgets::{EditField, ScrollContainer, SelectList, SourceRef, StringItem, StringItemSource, Window};
use turbo_ferret::*;

/// The state of one resource edit window. It covers the window (without taking
/// input) so it can paint the outline of the selected widget.
struct Controller {
    list: WidgetId,
    edit: WidgetId,
    /// Where the edited resource's widgets are built.
    build: WidgetId,
    source: Rc<RefCell<StringItemSource>>,
    /// The widget of each list item.
    widgets: Vec<WidgetId>,
    selected: Option<WidgetId>,
    /// The selected widget's rect in this widget's coordinates.
    selection: Option<Rect>,
    /// The global listener that forwards to this controller.
    forwarder: Option<ListenerId>,
}

impl Controller {
    /// Build the widgets again from the source text. TB: RefreshFromSource
    fn refresh_from_source(&mut self, cx: &mut Ctx, text: &str) {
        for c in cx.children(self.build).to_vec() {
            cx.remove(c);
        }
        // A text that does not parse (yet) just builds nothing.
        let _ = cx.inflate(self.build, text);
        // The forwarder may be the caller (it does not hear about widgets added
        // while it is being called), so ask for the list update here.
        let me = cx.this();
        if !cx.has_message(me, tbid!("update_widget_list")) {
            cx.post_message(me, tbid!("update_widget_list"), Value::Null);
        }
        // Back to the editor, in case the resource has `autofocus`.
        cx.set_focus(self.edit);
    }

    /// TB: UpdateWidgetList(true) + AddWidgetListItemsRecursive
    fn update_widget_list(&mut self, cx: &mut Ctx) {
        fn add(ui: &Ui, w: WidgetId, depth: usize, out: &mut Vec<(WidgetId, String)>) {
            if depth > 0 {
                let name = ui.node(w).map_or("?", |n| n.type_name());
                out.push((w, format!("{}{name}", " ".repeat(depth - 1))));
            }
            for &c in ui.children(w) {
                add(ui, c, depth + 1, out);
            }
        }
        let mut items = Vec::new();
        add(cx, self.build, 0, &mut items);
        let mut source = self.source.borrow_mut();
        source.clear();
        self.widgets.clear();
        for (w, name) in items {
            source.add(StringItem::new(&name));
            self.widgets.push(w);
        }
    }

    /// TB: SetSelectedWidget
    fn select(&mut self, cx: &mut Ctx, widget: WidgetId) {
        self.selected = Some(widget);
        if let Some(i) = self.widgets.iter().position(|&w| w == widget) {
            cx.set_value(self.list, i as i32);
        }
    }
}

impl Widget for Controller {
    fn type_name(&self) -> &'static str {
        "ResourceEditController"
    }

    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {
        if msg.id == tbid!("update_widget_list") {
            self.update_widget_list(cx);
        }
    }

    /// The window is gone: stop listening.
    fn on_free(&mut self, ui: &mut Ui) {
        if let Some(l) = self.forwarder.take() {
            ui.remove_listener(l);
        }
    }

    /// Cover the window, and follow the selected widget.
    fn on_process(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        if let Some(p) = cx.parent(me) {
            let size = cx.rect(p).size();
            cx.set_rect(me, Rect::from_size(size));
        }
        let selection = self.selected.filter(|&w| cx.exists(w)).map(|w| {
            let origin = cx.to_root(w, Point::ZERO) - cx.to_root(me, Point::ZERO);
            Rect::from_origin_size(origin, cx.rect(w).size())
        });
        if selection != self.selection {
            self.selection = selection;
            cx.invalidate();
        }
    }

    /// TB: ResourceEditWindow::OnPaintChildren
    fn paint(&self, p: &mut PaintCtx) {
        if let Some(r) = self.selection {
            p.draw_rect(r, Color::rgb(255, 205, 0));
        }
    }
}

/// Forwards the events of one resource edit window to its controller.
struct Forwarder {
    win: WidgetId,
    ctl: WidgetId,
    build: WidgetId,
    list: WidgetId,
    edit: WidgetId,
    scroll_container: WidgetId,
}

impl Forwarder {
    fn ctl<R>(&self, ui: &mut Ui, f: impl FnOnce(&mut Controller, &mut Ctx) -> R) -> Option<R> {
        ui.with_widget::<Controller, _>(self.ctl, f)
    }

    fn list_changed(&self, ui: &mut Ui) {
        if !ui.has_message(self.ctl, tbid!("update_widget_list")) {
            ui.post_message(self.ctl, tbid!("update_widget_list"), Value::Null);
        }
    }
}

impl UiListener for Forwarder {
    /// TB: OnWidgetAdded
    fn on_added(&mut self, ui: &mut Ui, _parent: WidgetId, child: WidgetId) {
        if ui.is_ancestor_of(self.build, child) {
            self.list_changed(ui);
        }
    }

    /// TB: OnWidgetRemove
    fn on_removing(&mut self, ui: &mut Ui, _parent: Option<WidgetId>, child: WidgetId) {
        if child == self.win {
            // The window goes: stop listening.
            return;
        }
        if ui.is_ancestor_of(self.build, child) {
            self.list_changed(ui);
        }
    }

    /// TB: OnWidgetInvokeEvent + ResourceEditWindow::OnEvent
    fn on_event(&mut self, ui: &mut Ui, target: WidgetId, ev: &Event) -> bool {
        if !ui.exists(self.win) {
            return false;
        }
        // Events to the built widgets stop here, so the layout under edit does not
        // act on clicks. With Alt they go through, to try it out in place.
        if target != self.build && ui.is_ancestor_of(self.build, target) {
            if ev.mods.contains(Modifiers::ALT) {
                return false;
            }
            if ev.kind == EventKind::PointerDown {
                self.ctl(ui, |c, cx| c.select(cx, target));
            }
            return true;
        }
        if !ui.is_ancestor_of(self.win, target) {
            return false;
        }
        let id = ui.node(target).map_or(TbId::NONE, |n| n.id());
        let value = ev.value.clone().unwrap_or(Value::Null);
        match ev.kind {
            EventKind::Changed if id == tbid!("widget_list_search") => {
                let filter = value.as_str().into_owned();
                ui.with_widget::<SelectList, _>(self.list, |l, cx| l.set_filter(cx, &filter));
                true
            }
            EventKind::Changed if target == self.list => {
                let index = value.as_int();
                self.ctl(ui, |c, _| {
                    if let Some(&w) = usize::try_from(index).ok().and_then(|i| c.widgets.get(i)) {
                        c.selected = Some(w);
                    }
                });
                false
            }
            EventKind::Changed if target == self.edit => {
                // The editor is busy sending this event: its text comes with it.
                let text = value.as_str().into_owned();
                self.ctl(ui, |c, cx| c.refresh_from_source(cx, &text));
                true
            }
            EventKind::Changed if id == tbid!("constrained") => {
                let adapt = value.as_int() != 0;
                ui.with_widget::<ScrollContainer, _>(self.scroll_container, |s, cx| {
                    s.set_adapt_content_size(cx, adapt)
                });
                false
            }
            EventKind::Click if id == tbid!("test") => {
                // A window with the current layout, sized to it and centered.
                let root = ui.root();
                let text = ui.text(self.edit).unwrap_or("").to_string();
                let win = ui.add(root, Window::new("Test window"));
                let _ = ui.inflate(win, &text);
                let bounds = Rect::from_size(ui.rect(root).size());
                let r = ui.fit_content_rect(win, widgets::ResizeFit::Preferred);
                ui.set_rect(win, r.center_in(&bounds).move_in(&bounds).clip(&bounds));
                true
            }
            _ => false,
        }
    }
}

/// Open a resource edit window on a resource file. TB: new ResourceEditWindow + Load
pub fn open_resource_edit_window(ui: &mut Ui, path: &str) -> Option<WidgetId> {
    let root = ui.root();
    let win = ui.add(root, Window::new(path));
    if let Err(e) = ui.inflate_file(win, "demo01/ui_resources/resource_edit_window.tb.txt") {
        eprintln!("demo: resource edit window: {e}");
    }
    let scroll_container = ui.find_as::<ScrollContainer>(win, tbid!("scroll_container"))?;
    let edit = ui.find_as::<EditField>(win, tbid!("source_edit"))?;
    let list = ui.find_as::<SelectList>(win, tbid!("widget_list"))?;
    let build = ui.content_root(scroll_container);
    let source = StringItemSource::shared();
    let shared: SourceRef = source.clone();
    ui.with_widget::<SelectList, _>(list, |l, cx| l.set_source(cx, Some(shared)));

    let ctl = ui.add(
        win,
        Controller { list, edit, build, source, widgets: Vec::new(), selected: None, selection: None, forwarder: None },
    );
    ui.set_ignore_input(ctl, true);
    let forwarder = ui.add_listener(Box::new(Forwarder { win, ctl, build, list, edit, scroll_container }));
    ui.with_widget::<Controller, _>(ctl, |c, _| c.forwarder = Some(forwarder));
    ui.set_rect(win, Rect::new(100, 50, 900, 600));

    // Load the file into the editor.
    let text = match ui.services_mut().loader.load(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => {
            ui.show_message(root, TbId::NONE, "Error loading resource", &format!("{e}"), &Default::default());
            String::new()
        }
    };
    // Its Changed event builds the widgets.
    ui.set_text(edit, &text);
    Some(win)
}
