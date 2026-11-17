//! `MessageWindow`: a window with a message and buttons.
//! TB: tb_message_window.cpp (TBMessageWindow, TBMessageWindowSettings)
//!

use alloc::boxed::Box;

use crate::arena::WidgetId;
use crate::event::{Event, EventKind, SpecialKey};
use crate::geom::Rect;
use crate::id::TbId;
use crate::layout::size::Gravity;
use crate::layout::stack::{LayoutDistribution, LayoutDistributionPosition, LayoutSize};
use crate::ui::{Ctx, ListenerId, NodeRef, Ui};
use crate::widget::{Axis, DieAction, Widget, Z};

use super::popup::TargetWatcher;
use super::window::{ResizeFit, Window, forward_to_window};
use super::{Button, Dimmer, EditField, Layout, SkinImage};

/// Which buttons a message window has. TB: TB_MSG
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MessageButtons {
    #[default]
    Ok,
    OkCancel,
    YesNo,
}

/// TB: TBMessageWindowSettings
#[derive(Copy, Clone, Debug, Default)]
pub struct MessageSettings {
    pub buttons: MessageButtons,
    /// Skin of an icon next to the message (none by default).
    pub icon_skin: TbId,
    /// Dim the widgets behind the window with a `Dimmer`.
    pub dimmer: bool,
    /// The message has styling tags (`<color #rrggbb>..</color>`).
    pub styling: bool,
}

impl MessageSettings {
    pub fn new(buttons: MessageButtons, icon_skin: TbId) -> MessageSettings {
        MessageSettings { buttons, icon_skin, ..Default::default() }
    }
}

/// A window that shows a message. A click on one of its buttons sends a `Click` with
/// the window's id through the target widget, with the button's id as `ref_id`
/// ("TBMessageWindow.ok", ".cancel", ".yes", ".no", or "TBWindow.close"), and closes
/// the window. It also closes when its target goes away. Show one with
/// [`Ui::show_message`]. TB: TBMessageWindow
pub struct MessageWindow {
    window: Window,
    target: WidgetId,
    dimmer: Option<WidgetId>,
    watcher: Option<ListenerId>,
}

impl MessageWindow {
    pub fn new(target: WidgetId) -> MessageWindow {
        MessageWindow { window: Window::default(), target, dimmer: None, watcher: None }
    }

    pub fn target(&self) -> WidgetId {
        self.target
    }

    /// Send the click for button `ref_id` through the target, then close.
    fn answer(&mut self, cx: &mut Ctx, ref_id: TbId) {
        let me = cx.this();
        let mut click = Event::new(EventKind::Click, me);
        click.ref_id = ref_id;
        cx.emit(click);
        cx.close_window(me);
    }
}

impl Widget for MessageWindow {
    fn type_name(&self) -> &'static str {
        "MessageWindow"
    }

    /// TB: TBMessageWindow::TBMessageWindow
    fn on_create(&mut self, cx: &mut Ctx) {
        self.window.on_create(cx);
        let me = cx.this();
        let watcher = TargetWatcher { window: me, target: self.target, outside_closes: false };
        self.watcher = Some(cx.add_listener(Box::new(watcher)));
    }

    /// TB: TBMessageWindow::~TBMessageWindow
    fn on_removed(&mut self, cx: &mut Ctx) {
        if let Some(w) = self.watcher.take() {
            cx.remove_listener(w);
        }
        if let Some(d) = self.dimmer.take() {
            cx.remove(d);
        }
    }

    /// TB: TBMessageWindow::GetEventDestination
    fn event_destination(&self, _this: NodeRef<'_>) -> Option<WidgetId> {
        Some(self.target)
    }

    /// TB: TBMessageWindow::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        if ev.kind == EventKind::Click && cx.get::<Button>(ev.target).is_some() {
            let id = cx.node(ev.target).map_or(TbId::NONE, |n| n.id());
            self.answer(cx, id);
            return true;
        }
        if ev.kind == EventKind::KeyDown && ev.special == SpecialKey::Esc {
            // As if the close button was clicked.
            self.answer(cx, crate::tbid!("TBWindow.close"));
            return true;
        }
        self.window.on_event(cx, ev)
    }

    /// TB: TBMessageWindow::OnDie
    fn on_die(&mut self, cx: &mut Ctx) -> DieAction {
        if let Some(d) = self.dimmer.filter(|&d| cx.exists(d)) {
            cx.die(d);
        }
        self.window.on_die(cx)
    }

    forward_to_window!(window);
}

impl Ui {
    /// Show a message window for `target`, centered in the root. The answer comes as
    /// a `Click` event with `id` through `target` (see [`MessageWindow`]).
    /// TB: new TBMessageWindow(target, id) + Show
    pub fn show_message(
        &mut self,
        target: WidgetId,
        id: TbId,
        title: &str,
        message: &str,
        settings: &MessageSettings,
    ) -> Option<WidgetId> {
        if !self.exists(target) {
            return None;
        }
        let root = self.parent_root(target);
        let win = self.create(MessageWindow::new(target));
        self.set_id(win, id);

        // Icon and text, then the buttons.
        let mut col = Layout::new(Axis::Y);
        col.distribution = LayoutDistribution::Available;
        let col = self.add(win, col);
        self.set_gravity(col, Gravity::ALL);
        // Into the window's padding rect (below the title), as inflation would.
        self.place_child(win, col);
        let mut row = Layout::default();
        row.distribution = LayoutDistribution::Available;
        row.size = LayoutSize::Available;
        let row = self.add(col, row);
        self.set_gravity(row, Gravity::ALL);
        let icon = self.add(row, SkinImage);
        self.set_skin(icon, settings.icon_skin);
        let text = self.add(row, EditField::default());
        self.with_widget::<EditField, _>(text, |e, cx| {
            e.set_multiline(cx, true);
            e.set_read_only(cx, true);
            e.set_styling(cx, settings.styling);
            e.set_text(cx, message);
        });
        self.set_skin(text, TbId::NONE);
        self.set_gravity(text, Gravity::ALL);
        let mut buttons = Layout::default();
        buttons.distribution_position = LayoutDistributionPosition::RightBottom;
        let buttons = self.add(col, buttons);
        self.set_gravity(buttons, Gravity::ALL);
        self.set_text(win, title);

        let ids: &[TbId] = match settings.buttons {
            MessageButtons::Ok => &[crate::tbid!("TBMessageWindow.ok")],
            MessageButtons::OkCancel => &[crate::tbid!("TBMessageWindow.ok"), crate::tbid!("TBMessageWindow.cancel")],
            MessageButtons::YesNo => &[crate::tbid!("TBMessageWindow.yes"), crate::tbid!("TBMessageWindow.no")],
        };
        let mut first = None;
        for &bid in ids {
            let label = self.lang().get(bid).into_owned();
            let b = self.add(buttons, Button::new(&label));
            self.set_id(b, bid);
            first.get_or_insert(b);
        }

        if settings.dimmer {
            let dimmer = self.add(root, Dimmer);
            if let Some(w) = self.get_mut::<MessageWindow>(win) {
                w.dimmer = Some(dimmer);
            }
        }
        // Size to the content (the text field's default size), then grow by what
        // the text overflows at that width, to show all of it.
        self.resize_to_fit_content(win, ResizeFit::Preferred);
        let mut r = self.rect(win);
        r.h += self.get::<EditField>(text).map_or(0, |e| e.overflow_y());
        let bounds = Rect::from_size(self.rect(root).size());
        let r = r.center_in(&bounds).move_in(&bounds).clip(&bounds);
        self.set_rect(win, r);
        self.attach(root, win, Z::Top);
        if let Some(b) = first {
            self.set_focus(b);
        }
        Some(win)
    }
}
