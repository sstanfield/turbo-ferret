//! `TextField` and the text-with-alignment helper. TB: tb_widgets_common.cpp
//! (TBWidgetString, TBTextField)

use alloc::string::String;

use crate::geom::{Color, Point, Rect};
use crate::layout::size::{PreferredSize, SizeConstraints};
use crate::paint::PaintCtx;
use crate::res::ResNode;
use crate::ui::Ctx;
use crate::widget::Widget;

/// TB: TB_TEXT_ALIGN
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum TextAlign {
    Left,
    Right,
    #[default]
    Center,
}

impl TextAlign {
    pub fn parse(s: &str) -> TextAlign {
        match s {
            "left" => TextAlign::Left,
            "right" => TextAlign::Right,
            _ => TextAlign::Center,
        }
    }
}

/// Paint one line of text in `rect` with alignment, vertically centered, and "..."
/// when it does not fit. TB: TBWidgetString::Paint
pub fn paint_text_line(p: &mut PaintCtx, rect: Rect, color: Color, text: &str, align: TextAlign) {
    let w = p.text_width(text);
    let h = p.font_height();
    let mut x = rect.x;
    match align {
        TextAlign::Right => x += rect.w - w,
        TextAlign::Center => x += ((rect.w - w) / 2).max(0),
        TextAlign::Left => {}
    }
    let y = rect.y + (rect.h - h) / 2;
    if w <= rect.w {
        p.draw_text(Point::new(x, y), color, text);
        return;
    }
    let end = "...";
    let endw = p.text_width(end);
    let mut startw = 0;
    let mut cut = 0;
    for (i, _) in text.char_indices().skip(1).chain(core::iter::once((text.len(), ' '))) {
        let nw = p.text_width(&text[..i]);
        if nw + endw > rect.w {
            break;
        }
        startw = nw;
        cut = i;
    }
    p.draw_text(Point::new(x, y), color, &text[..cut]);
    p.draw_text(Point::new(x + startw, y), color, end);
}

/// A single line of text. TB: TBTextField
#[derive(Default)]
pub struct TextField {
    pub text: String,
    pub align: TextAlign,
    /// May be narrower than its text (shows "..."). TB: SetSqueezable
    pub squeezable: bool,
}

impl TextField {
    pub fn new(text: &str) -> TextField {
        TextField { text: String::from(text), ..Default::default() }
    }
}

impl Widget for TextField {
    fn type_name(&self) -> &'static str {
        "TextField"
    }

    fn on_create(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.set_skin_quiet(me, crate::tbid!("TBTextField"));
    }

    fn on_inflate(&mut self, _cx: &mut Ctx, res: &ResNode) {
        if let Some(a) = res.get("text-align") {
            self.align = TextAlign::parse(&a.value.as_str());
        }
    }

    /// TB: TBTextField::OnCalculatePreferredContentSize
    fn measure_content(&mut self, cx: &mut Ctx, _sc: SizeConstraints) -> PreferredSize {
        let me = cx.this();
        let font = cx.calculated_font(me);
        let mut ps = PreferredSize::default();
        ps.pref.w = cx.text_width(font, &self.text);
        ps.pref.h = cx.font_height(font);
        ps.min.h = ps.pref.h;
        if !cx.gravity(me).fills_y() {
            ps.max.h = ps.pref.h;
        }
        if !self.squeezable {
            ps.min.w = ps.pref.w;
        }
        ps
    }

    fn on_font_changed(&mut self, cx: &mut Ctx) {
        let me = cx.this();
        cx.invalidate_layout(me);
    }

    fn paint(&self, p: &mut PaintCtx) {
        let r = p.padding_rect();
        let c = p.text_color();
        paint_text_line(p, r, c, &self.text, self.align);
    }

    fn text(&self) -> Option<&str> {
        Some(&self.text)
    }

    fn set_text(&mut self, cx: &mut Ctx, text: &str) {
        if self.text != text {
            self.text = String::from(text);
            let me = cx.this();
            cx.invalidate_layout(me);
        }
    }

    fn value(&self) -> crate::value::Value {
        crate::value::Value::str(&self.text)
    }
}
