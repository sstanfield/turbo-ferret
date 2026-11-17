//! `EditField`: a one line or multi line text field, editable or read-only.
//! TB: tb_editfield.cpp (TBEditField, TBEditFieldScrollRoot,
//! TBEditFieldContentFactory, TBTextFragmentContentWidget)
//!
//! TB's edit field was the listener of its TBStyleEdit and got called back in the
//! middle of edits. Here every call into the [`StyleEdit`] goes through `with_edit`,
//! which lends it an environment (font metrics, embedded widgets) and afterwards
//! applies the effects it collected (repaint, scroll bars, `Changed`, caret blink).

use alloc::rc::Rc;
use alloc::string::String;
use core::cell::OnceCell;

use crate::arena::WidgetId;
use crate::edit::{Blink, CaretPos, Content, EditEnv, EditPainter, StyleEdit};
use crate::event::{Event, EventKind};
use crate::font::FontDesc;
use crate::geom::{Color, Point, Rect, Size};
use crate::id::TbId;
use crate::layout::size::{PreferredSize, SizeConstraints, SizeDep};
use crate::msg::Message;
use crate::paint::PaintCtx;
use crate::res::ResNode;
use crate::skin::Condition;
use crate::skin::condition::CondProp;
use crate::ui::{Ctx, NodeRef};
use crate::value::Value;
use crate::widget::{Axis, HitStatus, ScrollInfo, Widget, Z};

use super::Plain;
use super::popup::PopupAlignment;
use super::scroll::ScrollBar;
use super::select_item::{SourceRef, StringItem, StringItemSource};
use super::text::{TextAlign, paint_text_line};

const CARET_BLINK_TIME: u32 = 500;
const SELECTION_SCROLL_DELAY: u32 = 1000 / 30;
const BLINK: TbId = crate::tbid!("blink");
const SELSCROLL: TbId = crate::tbid!("selscroll");

/// What an edit field is for. It does not restrict input; it is a hint for virtual
/// keyboards and skins (`search` gets an icon), and `Password` hides the text.
/// TB: EDIT_TYPE
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum EditType {
    #[default]
    Text,
    Search,
    Password,
    Email,
    Phone,
    Url,
    Number,
}

impl EditType {
    /// TB: TBEditField::OnInflate ("type", matched anywhere, ignoring case)
    pub fn parse(s: &str) -> Option<EditType> {
        let s = s.to_ascii_lowercase();
        [
            ("text", EditType::Text),
            ("search", EditType::Search),
            ("password", EditType::Password),
            ("email", EditType::Email),
            ("phone", EditType::Phone),
            ("url", EditType::Url),
            ("number", EditType::Number),
        ]
        .into_iter()
        .find(|(name, _)| s.contains(name))
        .map(|(_, t)| t)
    }

    fn name(self) -> &'static str {
        match self {
            EditType::Text => "text",
            EditType::Search => "search",
            EditType::Password => "password",
            EditType::Email => "email",
            EditType::Phone => "phone",
            EditType::Url => "url",
            EditType::Number => "number",
        }
    }
}

/// How far to scroll when the pointer is dragged outside `min..max`.
/// TB: GetSelectionScrollSpeed
fn selection_scroll_speed(pointer: i32, min: i32, max: i32) -> i32 {
    let d = if pointer < min {
        pointer - min
    } else if pointer > max {
        pointer - max
    } else {
        0
    };
    let speed = d * d / 40;
    if pointer < min { -speed } else { speed }
}

/// The scrolled, clipped parent of widgets embedded in the text. Only its children
/// take input. TB: TBEditFieldScrollRoot
#[derive(Default)]
pub struct EditFieldScrollRoot {
    pos: Point,
    children: u32,
}

impl Widget for EditFieldScrollRoot {
    fn type_name(&self) -> &'static str {
        "EditFieldScrollRoot"
    }
    fn on_child_added(&mut self, _cx: &mut Ctx, _child: WidgetId) {
        self.children += 1;
    }
    fn on_child_removed(&mut self, _cx: &mut Ctx, _child: WidgetId) {
        self.children = self.children.saturating_sub(1);
    }
    /// TB: TBEditFieldScrollRoot::GetChildTranslation
    fn child_translation(&self) -> Point {
        Point::new(-self.pos.x, -self.pos.y)
    }
    /// No clip (which ends a batch) when there is nothing to paint.
    /// TB: TBEditFieldScrollRoot::OnPaintChildren
    fn children_clip(&self, padding: Rect) -> Option<Rect> {
        (self.children > 0).then_some(padding)
    }
    /// No hit on this widget, but maybe on a child. TB: GetHitStatus
    fn hit_test(&self, this: NodeRef<'_>, p: Point) -> HitStatus {
        if HitStatus::default_for(this, p) == HitStatus::NoHit {
            return HitStatus::NoHit;
        }
        let p = p + self.pos;
        if this.child_refs().any(|c| c.rect().contains(p)) { HitStatus::Hit } else { HitStatus::NoHit }
    }
}

/// The environment of the style edit while a hook of the edit field runs.
struct Env<'a, 'b> {
    cx: &'a mut Ctx<'b>,
    root: Option<WidgetId>,
}

impl EditEnv for Env<'_, '_> {
    fn string_width(&mut self, font: FontDesc, text: &str) -> i32 {
        self.cx.text_width(font, text)
    }
    fn font_height(&mut self, font: FontDesc) -> i32 {
        self.cx.font_height(font)
    }
    fn font_ascent(&mut self, font: FontDesc) -> i32 {
        self.cx.font_ascent(font)
    }

    /// The built-in content, and `<widget ...>`: any widget from an inline resource
    /// string, for example `<widget TBButton: text: "Hello" id: "hello">`.
    /// TB: TBEditFieldContentFactory::CreateFragmentContent
    fn create_content(&mut self, tag: &str) -> Option<Content> {
        let (Some(source), Some(root)) = (tag.strip_prefix("<widget "), self.root) else {
            return Content::from_tag(tag);
        };
        let source = source.strip_suffix('>').unwrap_or(source);
        // A wrapper whose size follows what is inflated into it.
        let wrapper = self.cx.add(root, Plain);
        if let Err(e) = self.cx.inflate(wrapper, source) {
            self.cx.warn(format_args!("EditField: <widget>: {e}"));
        }
        Some(Content::Widget(wrapper))
    }

    /// TB: TBTextFragmentContentWidget::GetWidth / GetHeight
    fn content_size(&mut self, content: &Content) -> Size {
        let Content::Widget(w) = *content else { return Size::ZERO };
        let rect = self.cx.rect(w).size();
        if rect.w != 0 && rect.h != 0 {
            return rect;
        }
        let pref = self.cx.preferred_size(w, SizeConstraints::NONE).pref;
        Size::new(if rect.w != 0 { rect.w } else { pref.w }, if rect.h != 0 { rect.h } else { pref.h })
    }

    /// TB: TBTextFragmentContentWidget::UpdatePos
    fn content_moved(&mut self, content: &Content, pos: Point) {
        if let Content::Widget(w) = *content {
            let size = self.content_size(content);
            self.cx.set_rect(w, Rect::from_origin_size(pos, size));
        }
    }

    /// TB: TBTextFragmentContentWidget::~TBTextFragmentContentWidget
    fn content_removed(&mut self, content: &Content) {
        if let Content::Widget(w) = *content {
            self.cx.remove(w);
        }
    }
}

/// Draws the style edit through a `PaintCtx`.
struct Painter<'p, 'a, 'r> {
    p: &'p mut PaintCtx<'a, 'r>,
    caret: bool,
}

impl EditPainter for Painter<'_, '_, '_> {
    fn string_width(&mut self, font: FontDesc, text: &str) -> i32 {
        self.p.text_width_with(font, text)
    }
    fn draw_string(&mut self, pos: Point, font: FontDesc, color: Color, text: &str) {
        self.p.draw_text_with(font, pos, color, text);
    }
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.p.fill_rect(rect, color);
    }
    /// TB: TBEditField::DrawTextSelectionBg
    fn draw_selection(&mut self, rect: Rect) {
        let state = self.p.state();
        self.p.draw_skin(rect, crate::tbid!("TBEditField.selection"), state);
    }
    /// TB: TBEditField::DrawCaret
    fn draw_caret(&mut self, rect: Rect) {
        if self.caret {
            self.draw_selection(rect);
        }
    }
}

/// A text field: one line or many, editable or read-only, optionally a password
/// field (`EditType::Password`) or styled text with embedded content (`styling`).
/// Its value is its text. TB: TBEditField
pub struct EditField {
    edit: StyleEdit,
    placeholder: String,
    edit_type: EditType,
    adapt_to_content: bool,
    virtual_width: i32,
    scrollbar_x: Option<WidgetId>,
    scrollbar_y: Option<WidgetId>,
    root: Option<WidgetId>,
    /// Scroll position and limits (the scroll bars show the same).
    info: ScrollInfo,
    /// The whole text, built when asked for and dropped on a change.
    text_cache: OnceCell<String>,
}

impl Default for EditField {
    fn default() -> Self {
        EditField {
            edit: StyleEdit::new(),
            placeholder: String::new(),
            edit_type: EditType::Text,
            adapt_to_content: false,
            virtual_width: 250,
            scrollbar_x: None,
            scrollbar_y: None,
            root: None,
            info: ScrollInfo::default(),
            text_cache: OnceCell::new(),
        }
    }
}

impl EditField {
    /// Read access to the text model (blocks, caret, selection). Changes go through
    /// the edit field's own methods, which keep the widget in step.
    pub fn style_edit(&self) -> &StyleEdit {
        &self.edit
    }

    /// Call into the style edit with its environment, then do what it asks for.
    fn with_edit<R>(&mut self, cx: &mut Ctx, f: impl FnOnce(&mut StyleEdit, &mut dyn EditEnv) -> R) -> R {
        let r = f(&mut self.edit, &mut Env { cx: &mut *cx, root: self.root });
        self.after_edit(cx);
        r
    }

    /// Apply the style edit's effects. TB: the TBStyleEditListener callbacks
    fn after_edit(&mut self, cx: &mut Ctx) {
        let fx = self.edit.take_effects();
        let me = cx.this();
        if fx.invalidate {
            cx.invalidate();
        }
        if fx.update_scrollbars {
            self.update_scrollbars(cx);
        }
        if fx.scrolled {
            self.sync_scroll(cx);
        }
        match fx.blink {
            // TB: CaretBlinkStart
            Some(Blink::Start) => {
                if !cx.has_message(me, BLINK) {
                    cx.post_message_delayed(me, BLINK, Value::Null, CARET_BLINK_TIME);
                }
            }
            // TB: CaretBlinkStop
            Some(Blink::Stop) => cx.cancel_messages(me, Some(BLINK)),
            Some(Blink::Restart) => {
                cx.cancel_messages(me, Some(BLINK));
                cx.post_message_delayed(me, BLINK, Value::Null, CARET_BLINK_TIME);
            }
            None => {}
        }
        if fx.changed {
            // TB: TBEditField::OnChange
            self.text_cache = OnceCell::new();
            if self.adapt_to_content {
                cx.invalidate_layout(me);
            }
            let text = Value::Str(Rc::from(self.text().unwrap_or("")));
            cx.emit_changed(text);
        }
    }

    /// TB: TBEditField::UpdateScrollbars
    fn update_scrollbars(&mut self, cx: &mut Ctx) {
        let size = self.edit.layout_size();
        let max = Point::new(self.edit.overflow_x(), self.edit.overflow_y());
        if let Some(sb) = self.scrollbar_x {
            cx.with_widget::<ScrollBar, _>(sb, |w, cx| w.set_limits_quiet(cx, 0.0, max.x as f64, size.w as f64));
        }
        if let Some(sb) = self.scrollbar_y {
            cx.with_widget::<ScrollBar, _>(sb, |w, cx| w.set_limits_quiet(cx, 0.0, max.y as f64, size.h as f64));
        }
        self.info.max = max;
        self.sync_scroll(cx);
    }

    /// Show the scroll position in the scroll bars and move embedded widgets.
    /// TB: TBEditField::Scroll
    fn sync_scroll(&mut self, cx: &mut Ctx) {
        let pos = self.edit.scroll();
        self.info.pos = pos;
        if let Some(sb) = self.scrollbar_x {
            cx.with_widget::<ScrollBar, _>(sb, |w, cx| w.set_value_quiet(cx, pos.x as f64));
        }
        if let Some(sb) = self.scrollbar_y {
            cx.with_widget::<ScrollBar, _>(sb, |w, cx| w.set_value_quiet(cx, pos.y as f64));
        }
        if let Some(r) = self.root.and_then(|r| cx.get_mut::<EditFieldScrollRoot>(r)) {
            r.pos = pos;
        }
        cx.invalidate();
    }

    /// The area of the text: the padding rect less the visible scroll bars.
    /// TB: GetVisibleRect
    pub fn visible_rect(&self, cx: &Ctx) -> Rect {
        let mut r = cx.padding_rect(cx.this());
        if let Some(sb) = self.scrollbar_y.filter(|&s| cx.opacity(s) != 0.0) {
            r.w -= cx.rect(sb).w;
        }
        if let Some(sb) = self.scrollbar_x.filter(|&s| cx.opacity(s) != 0.0) {
            r.h -= cx.rect(sb).h;
        }
        r
    }

    /// TB: UpdateScrollbarVisibility
    fn update_scrollbar_visibility(&mut self, cx: &mut Ctx, multiline: bool) {
        let vertical = multiline && !self.adapt_to_content;
        if let Some(sb) = self.scrollbar_y {
            cx.set_opacity(sb, if vertical { 1.0 } else { 0.0 });
        }
        self.layout_parts(cx);
    }

    /// Place the scroll bars and the root, and give the style edit its size.
    fn layout_parts(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        let size = cx.rect(me).size();
        let (Some(sbx), Some(sby)) = (self.scrollbar_x, self.scrollbar_y) else { return };
        let sb_y_w = cx.preferred_size(sby, SizeConstraints::NONE).pref.w;
        let sb_x_h = cx.preferred_size(sbx, SizeConstraints::NONE).pref.h;
        cx.set_rect(sbx, Rect::new(0, size.h - sb_x_h, size.w - sb_y_w, sb_x_h));
        cx.set_rect(sby, Rect::new(size.w - sb_y_w, 0, sb_y_w, size.h));
        let visible = self.visible_rect(cx);
        if let Some(r) = self.root {
            cx.set_rect(r, visible);
        }
        self.with_edit(cx, |e, env| e.set_layout_size(env, visible.w, visible.h, false));
        self.update_scrollbars(cx);
    }

    pub fn is_multiline(&self) -> bool {
        self.edit.is_multiline()
    }

    /// Allow several lines (which also turns wrapping on or off). TB: SetMultiline
    pub fn set_multiline(&mut self, cx: &mut Ctx, multiline: bool) {
        if multiline == self.edit.is_multiline() {
            return;
        }
        self.edit.set_multiline(multiline);
        self.update_scrollbar_visibility(cx, multiline);
        self.set_wrapping(cx, multiline);
        cx.publish(self);
        cx.invalidate_skin_states();
    }

    pub fn is_styling(&self) -> bool {
        self.edit.is_styling()
    }

    /// Show tags in the text as styles and embedded content (see
    /// [`Content`]). TB: SetStyling
    pub fn set_styling(&mut self, cx: &mut Ctx, styling: bool) {
        if styling != self.edit.is_styling() {
            self.edit.set_styling(styling);
            self.with_edit(cx, |e, env| e.reformat(env, true));
        }
    }

    pub fn is_read_only(&self) -> bool {
        self.edit.is_read_only()
    }

    /// In read-only mode the text cannot be edited and the caret is hidden; it can
    /// still be focused, selected and copied. TB: SetReadOnly
    pub fn set_read_only(&mut self, cx: &mut Ctx, read_only: bool) {
        if read_only != self.edit.is_read_only() {
            self.edit.set_read_only(read_only);
            cx.invalidate_skin_states();
        }
    }

    pub fn is_wrapping(&self) -> bool {
        self.edit.is_wrapping()
    }

    /// TB: SetWrapping
    pub fn set_wrapping(&mut self, cx: &mut Ctx, wrapping: bool) {
        if wrapping == self.edit.is_wrapping() {
            return;
        }
        self.with_edit(cx, |e, env| e.set_wrapping(env, wrapping));
        if self.adapt_to_content {
            let me = cx.this();
            cx.invalidate_layout(me);
        }
    }

    pub fn adapt_to_content_size(&self) -> bool {
        self.adapt_to_content
    }

    /// Whether the preferred size follows the content (off by default). With
    /// wrapping, the result depends on the virtual width. TB: SetAdaptToContentSize
    pub fn set_adapt_to_content_size(&mut self, cx: &mut Ctx, adapt: bool) {
        if adapt != self.adapt_to_content {
            self.adapt_to_content = adapt;
            let multiline = self.edit.is_multiline();
            self.update_scrollbar_visibility(cx, multiline);
            let me = cx.this();
            cx.invalidate_layout(me);
        }
    }

    pub fn virtual_width(&self) -> i32 {
        self.virtual_width
    }

    /// The width a wrapping field that adapts to its content lays its text out in
    /// to find its preferred size (250 by default). TB: SetVirtualWidth
    pub fn set_virtual_width(&mut self, cx: &mut Ctx, width: i32) {
        if width != self.virtual_width {
            self.virtual_width = width;
            if self.adapt_to_content && self.edit.is_wrapping() {
                let me = cx.this();
                cx.invalidate_layout(me);
            }
        }
    }

    pub fn edit_type(&self) -> EditType {
        self.edit_type
    }

    /// TB: SetEditType
    pub fn set_edit_type(&mut self, cx: &mut Ctx, t: EditType) {
        if t != self.edit_type {
            self.edit_type = t;
            self.with_edit(cx, |e, env| e.set_password(env, t == EditType::Password));
            cx.invalidate_skin_states();
        }
    }

    pub fn text_align(&self) -> TextAlign {
        self.edit.align()
    }

    /// The alignment of new lines, and of the selected lines (or the caret's).
    /// TB: SetTextAlign
    pub fn set_text_align(&mut self, cx: &mut Ctx, align: TextAlign) {
        self.with_edit(cx, |e, env| e.set_align(env, align));
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// The text shown, faded, while the field is empty. TB: SetPlaceholderText
    pub fn set_placeholder(&mut self, cx: &mut Ctx, text: &str) {
        self.placeholder = String::from(text);
        cx.invalidate();
    }

    /// Set the text and put the caret at its beginning or end. TB: SetText(text, pos)
    pub fn set_text_and_caret(&mut self, cx: &mut Ctx, text: &str, pos: CaretPos) {
        self.text_cache = OnceCell::new();
        self.with_edit(cx, |e, env| e.set_text(env, text, pos));
    }

    /// Add text after the last character. TB: TBStyleEdit::AppendText
    pub fn append_text(&mut self, cx: &mut Ctx, text: &str, clear_undo_redo: bool) {
        self.text_cache = OnceCell::new();
        self.with_edit(cx, |e, env| e.append_text(env, text, clear_undo_redo));
    }

    /// Run an editing command: "cut", "copy", "paste", "delete", "undo", "redo" or
    /// "selectall" (the ids of `Shortcut` events and of the context menu's items).
    /// Returns false for anything else. TB: TBEditField::OnEvent (shortcut part)
    pub fn command(&mut self, cx: &mut Ctx, id: TbId) -> bool {
        let editable = !self.edit.is_read_only();
        if id == crate::tbid!("cut") {
            if editable {
                if let Some(text) = self.with_edit(cx, |e, env| e.cut(env)) {
                    cx.set_clipboard_text(&text);
                }
            }
        } else if id == crate::tbid!("copy") {
            if let Some(text) = self.edit.copy() {
                cx.set_clipboard_text(&text);
            }
        } else if id == crate::tbid!("paste") {
            if editable {
                if let Some(text) = cx.clipboard_text() {
                    self.with_edit(cx, |e, env| e.paste(env, &text));
                }
            }
        } else if id == crate::tbid!("delete") {
            if editable {
                self.with_edit(cx, |e, env| e.delete(env));
            }
        } else if id == crate::tbid!("undo") {
            if editable {
                self.with_edit(cx, |e, env| e.undo(env));
            }
        } else if id == crate::tbid!("redo") {
            if editable {
                self.with_edit(cx, |e, env| e.redo(env));
            }
        } else if id == crate::tbid!("selectall") {
            self.with_edit(cx, |e, _| e.select_all());
        } else {
            return false;
        }
        true
    }

    /// Select text by offsets into the whole text.
    pub fn select(&mut self, cx: &mut Ctx, from: usize, to: usize) {
        self.with_edit(cx, |e, _| e.select_global(from, to));
    }

    /// How much taller the text is than the field. TB: TBStyleEdit::GetOverflowY
    pub fn overflow_y(&self) -> i32 {
        self.edit.overflow_y()
    }

    pub fn can_undo(&self) -> bool {
        self.edit.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.edit.can_redo()
    }

    /// The context menu with the editing commands. TB: TBEditField::OnEvent (context menu)
    fn show_context_menu(&mut self, cx: &mut Ctx, pos: Point) {
        let me = cx.this();
        let source = StringItemSource::shared();
        {
            let mut s = source.borrow_mut();
            for name in ["cut", "copy", "paste", "delete"] {
                s.add(StringItem::with_id(&cx.lang().get_key(name), TbId::new(name)));
            }
            s.add(StringItem::new("-"));
            s.add(StringItem::with_id(&cx.lang().get_key("selectall"), crate::tbid!("selectall")));
        }
        let pos_in_root = cx.to_root(me, pos);
        let source: SourceRef = source;
        cx.show_menu(
            me,
            crate::tbid!("popupmenu"),
            source,
            &PopupAlignment::at(pos_in_root, crate::widget::Align::Bottom),
            -1,
        );
    }
}

impl Widget for EditField {
    fn type_name(&self) -> &'static str {
        "EditField"
    }

    /// TB: TBEditField::TBEditField
    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_focusable(me, true);
        cx.set_want_long_click(me, true);
        let sbx = cx.add(me, ScrollBar::default());
        let sby = cx.add(me, ScrollBar::default());
        cx.with_widget::<ScrollBar, _>(sby, |w, cx| w.set_axis(cx, Axis::Y));
        cx.set_opacity(sbx, 0.0);
        cx.set_opacity(sby, 0.0);
        let root = cx.add_at(me, EditFieldScrollRoot::default(), Z::Top);
        cx.set_skin_quiet(me, crate::tbid!("TBEditField"));
        self.scrollbar_x = Some(sbx);
        self.scrollbar_y = Some(sby);
        self.root = Some(root);
        let font = cx.calculated_font(me);
        self.with_edit(cx, |e, env| {
            e.set_font(env, font);
            e.reformat(env, true);
        });
    }

    /// TB: TBEditField::OnInflate
    fn on_inflate(&mut self, cx: &mut Ctx, res: &ResNode) {
        self.set_multiline(cx, res.int("multiline", 0) != 0);
        self.set_styling(cx, res.int("styling", 0) != 0);
        self.set_read_only(cx, res.int("readonly", 0) != 0);
        let wrap = res.int("wrap", self.edit.is_wrapping() as i32) != 0;
        self.set_wrapping(cx, wrap);
        let adapt = res.int("adapt-to-content", self.adapt_to_content as i32) != 0;
        self.set_adapt_to_content_size(cx, adapt);
        if let Some(n) = res.get("virtual-width") {
            let w = cx.px(&n.value.as_str(), self.virtual_width);
            self.set_virtual_width(cx, w);
        }
        if let Some(n) = res.get("placeholder") {
            let text = cx.lang().resolve(&n.value.as_str()).into_owned();
            self.set_placeholder(cx, &text);
        }
        if let Some(n) = res.get("text-align") {
            let align = TextAlign::parse(&n.value.as_str());
            self.set_text_align(cx, align);
        }
        if let Some(t) = res.get("type").and_then(|n| EditType::parse(&n.value.as_str())) {
            self.set_edit_type(cx, t);
        }
    }

    /// TB: TBEditField::OnAdded
    fn on_added(&mut self, cx: &mut Ctx) {
        self.on_font_changed(cx);
    }

    /// TB: TBEditField::OnFontChanged
    fn on_font_changed(&mut self, cx: &mut Ctx) {
        let font = cx.calculated_font(cx.this());
        self.with_edit(cx, |e, env| e.set_font(env, font));
    }

    fn on_skin_changed(&mut self, cx: &mut Ctx) {
        self.layout_parts(cx);
    }

    /// TB: TBEditField::OnFocusChanged
    fn on_focus_changed(&mut self, cx: &mut Ctx, focused: bool) {
        self.with_edit(cx, |e, _| e.focus(focused));
    }

    /// TB: TBEditField::OnResized
    fn arrange(&mut self, cx: &mut Ctx, _old: Size, _new: Size) {
        self.layout_parts(cx);
    }

    /// TB: TBEditField::OnCalculatePreferredContentSize
    fn measure_content(&mut self, cx: &mut Ctx, sc: SizeConstraints) -> PreferredSize {
        let me = cx.this();
        let font_height = cx.font_height(cx.calculated_font(me));
        let mut ps = PreferredSize::default();
        if !self.adapt_to_content {
            ps.pref.h = font_height;
            ps.min.h = font_height;
            if self.edit.is_multiline() {
                ps.pref.w = font_height * 10;
                ps.pref.h = font_height * 5;
            } else {
                ps.max.h = ps.pref.h;
            }
            return ps;
        }
        let old = self.edit.layout_size();
        let wrapping = self.edit.is_wrapping();
        if wrapping {
            // Lay the text out in a virtual width to get the same content size every
            // time, whatever size the field has now.
            let width =
                if sc.available.w != crate::layout::size::NO_RESTRICTION { sc.available.w } else { self.virtual_width };
            self.edit.set_layout_size(&mut Env { cx: &mut *cx, root: self.root }, width, old.h, true);
            ps.dependency = SizeDep::HEIGHT_DEPEND_ON_WIDTH;
        }
        let width = self.edit.content_width();
        let height = self.edit.content_height().max(font_height);
        if wrapping {
            self.edit.set_layout_size(&mut Env { cx: &mut *cx, root: self.root }, old.w, old.h, true);
            // The trial layout is not something to scroll or repaint for.
            self.edit.take_effects();
        }
        ps.min.w = width;
        ps.pref.w = width;
        ps.min.h = height;
        ps.pref.h = height;
        ps.max.h = height;
        ps
    }

    /// TB: TBEditField::OnPaint
    fn paint(&self, p: &mut PaintCtx) {
        let mut visible = p.padding_rect();
        let bar = |s: Option<WidgetId>| s.and_then(|s| p.node(s)).filter(|n| n.opacity() != 0.0).map(|n| n.rect());
        visible.w -= bar(self.scrollbar_y).map_or(0, |r| r.w);
        visible.h -= bar(self.scrollbar_x).map_or(0, |r| r.h);
        let color = p.text_color();
        let font = p.font();
        let caret = p.is_focused() && !self.edit.is_read_only();
        let placeholder_opacity = p.skin().defaults.placeholder_opacity;
        let draw = |p: &mut PaintCtx| {
            p.with_offset(visible.origin(), |p| {
                let area = Rect::from_size(visible.size());
                self.edit.paint(&mut Painter { p, caret }, area, color);
                // The placeholder, faded, while the field is empty.
                if self.edit.is_empty() && !self.placeholder.is_empty() {
                    let h = p.fonts().face(font).height();
                    p.with_opacity(placeholder_opacity, |p| {
                        paint_text_line(p, Rect::new(0, 0, area.w, h), color, &self.placeholder, TextAlign::Left);
                    });
                }
            });
        };
        // Clip only if something can be scrolled out.
        if self.info.can_scroll_x() || self.info.can_scroll_y() {
            p.with_clip(visible, draw);
        } else {
            draw(p);
        }
    }

    /// Fade out the edges that text is scrolled out over (if the skin has
    /// "TBEditField.fadeout_x" or "_y"). TB: TBEditField::OnPaintChildren
    fn paint_after_children(&self, p: &mut PaintCtx) {
        let i = self.info;
        if i.max == Point::ZERO {
            return;
        }
        let mut visible = p.padding_rect();
        let bar = |s: Option<WidgetId>| s.and_then(|s| p.node(s)).filter(|n| n.opacity() != 0.0).map(|n| n.rect());
        visible.w -= bar(self.scrollbar_y).map_or(0, |r| r.w);
        visible.h -= bar(self.scrollbar_x).map_or(0, |r| r.h);
        p.draw_edge_fadeout(
            visible,
            crate::tbid!("TBEditField.fadeout_x"),
            crate::tbid!("TBEditField.fadeout_y"),
            i.pos.x,
            i.pos.y,
            i.max.x - i.pos.x,
            i.max.y - i.pos.y,
        );
    }

    fn content_root(&self) -> Option<WidgetId> {
        self.root
    }

    fn scroll_root(&self) -> Option<WidgetId> {
        self.root
    }

    /// TB: TBEditField::GetScrollInfo
    fn scroll_info(&self) -> Option<ScrollInfo> {
        Some(self.info)
    }

    /// TB: TBEditField::ScrollTo
    fn scroll_to(&mut self, cx: &mut Ctx, p: Point) {
        self.with_edit(cx, |e, _| e.set_scroll_pos(p.x, p.y));
    }

    /// The text. TB: GetText
    fn text(&self) -> Option<&str> {
        Some(self.text_cache.get_or_init(|| self.edit.text()))
    }

    /// TB: SetText (the caret goes to the beginning)
    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        self.set_text_and_caret(cx, text, CaretPos::Beginning);
    }

    fn value(&self) -> Value {
        Value::Str(Rc::from(self.text().unwrap_or("")))
    }

    fn set_value(&mut self, cx: &mut Ctx, v: &Value) {
        let text = v.as_str().into_owned();
        self.set_text(cx, &text);
    }

    /// Skins can test "edit-type" (the type's name), "multiline" and "readonly"
    /// (0 or 1). TB: GetCustomSkinCondition
    fn custom_skin_condition(&self, cond: &Condition) -> bool {
        let CondProp::Custom(prop) = cond.prop else { return false };
        if prop == crate::tbid!("edit-type") {
            cond.value == TbId::new(self.edit_type.name())
        } else if prop == crate::tbid!("multiline") {
            (cond.value != TbId(0) && cond.value != crate::tbid!("0")) == self.edit.is_multiline()
        } else if prop == crate::tbid!("readonly") {
            (cond.value != TbId(0) && cond.value != crate::tbid!("0")) == self.edit.is_read_only()
        } else {
            false
        }
    }

    /// TB: TBEditField::OnMessageReceived
    fn on_message(&mut self, cx: &mut Ctx, msg: &Message) {
        let me = cx.this();
        if msg.id == BLINK {
            self.with_edit(cx, |e, _| e.blink());
            cx.post_message_delayed(me, BLINK, Value::Null, CARET_BLINK_TIME);
        } else if msg.id == SELSCROLL && cx.captured() == Some(me) {
            // Scroll while the pointer is dragged outside, faster the further out.
            let area = self.visible_rect(cx).shrink_xy(2, 2);
            let pointer = cx.pointer_move_pos();
            let d = Point::new(
                selection_scroll_speed(pointer.x, area.x, area.right()),
                selection_scroll_speed(pointer.y, area.y, area.bottom()),
            );
            if d != Point::ZERO {
                let origin = cx.padding_rect(me).origin();
                self.with_edit(cx, |e, env| {
                    let to = e.scroll() + d;
                    e.set_scroll_pos(to.x, to.y);
                    // The selection follows at the new scroll position.
                    e.mouse_move(env, pointer - origin);
                });
            }
            if self.edit.is_selecting() {
                cx.post_message_delayed(me, SELSCROLL, Value::Null, SELECTION_SCROLL_DELAY);
            }
        }
    }

    /// TB: TBEditField::OnEvent
    fn on_event(&mut self, cx: &mut Ctx, ev: &Event) -> bool {
        let me = cx.this();
        let own = ev.target == me;
        match ev.kind {
            // The user moved a scroll bar (it is busy: the value comes with the event).
            EventKind::Changed if Some(ev.target) == self.scrollbar_x || Some(ev.target) == self.scrollbar_y => {
                if let Some(v) = ev.value.as_ref().map(|v| v.as_float() as i32) {
                    let horizontal = Some(ev.target) == self.scrollbar_x;
                    self.with_edit(cx, |e, _| {
                        let s = e.scroll();
                        if horizontal { e.set_scroll_pos(v, s.y) } else { e.set_scroll_pos(s.x, v) }
                    });
                }
                true
            }
            EventKind::Wheel if ev.mods.is_empty() => {
                let old = self.edit.scroll();
                let dy = ev.delta.y * cx.config().pixels_per_line;
                self.with_edit(cx, |e, _| e.set_scroll_pos(old.x, old.y + dy));
                self.edit.scroll() != old
            }
            EventKind::PointerDown if own => {
                let p = ev.pos - cx.padding_rect(me).origin();
                if self.with_edit(cx, |e, env| e.mouse_down(env, p, ev.count, ev.touch)) {
                    // Start the selection scroll.
                    cx.post_message_delayed(me, SELSCROLL, Value::Null, SELECTION_SCROLL_DELAY);
                    return true;
                }
                false
            }
            EventKind::PointerMove if own => {
                let p = ev.pos - cx.padding_rect(me).origin();
                self.with_edit(cx, |e, env| e.mouse_move(env, p))
            }
            EventKind::PointerUp if own => {
                let cancelled = cx.is_click_cancelled();
                self.with_edit(cx, |e, env| e.mouse_up(env, ev.touch, cancelled))
            }
            EventKind::KeyDown => self.with_edit(cx, |e, env| e.key_down(env, ev.key, ev.special, ev.mods)),
            EventKind::KeyUp => true,
            EventKind::Shortcut => self.command(cx, ev.ref_id),
            // An item of the context menu.
            EventKind::Click if cx.node(ev.target).is_some_and(|n| n.id() == crate::tbid!("popupmenu")) => {
                self.command(cx, ev.ref_id)
            }
            EventKind::ContextMenu if own => {
                self.show_context_menu(cx, ev.pos);
                true
            }
            _ => false,
        }
    }
}
