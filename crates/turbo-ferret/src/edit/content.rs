//! Embedded (non-text) content of styled text.
//! TB: tb_style_edit_content.cpp (TBTextFragmentContent and its subclasses,
//! TBTextFragmentContentFactory)

use crate::arena::WidgetId;
use crate::geom::Color;

/// The content of a fragment that is not text. TB: TBTextFragmentContent
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// A horizontal line (`<hr>`). TB: TBTextFragmentContentHR
    Hr { width_percent: i32, height: i32 },
    /// Underline what follows (`<u>`). TB: TBTextFragmentContentUnderline
    Underline,
    /// Color what follows (`<color #rrggbb>`). TB: TBTextFragmentContentTextColor
    TextColor(Color),
    /// End the last style change (`</...>`). TB: TBTextFragmentContentStylePop
    StylePop,
    /// A widget made by the environment (`<widget ...>` in an edit field).
    /// TB: TBTextFragmentContentWidget
    Widget(WidgetId),
}

impl Content {
    /// The length of the tag at the start of `text` ("<" up to ">"), or 0.
    /// TB: TBTextFragmentContentFactory::GetContent
    pub fn tag_len(text: &str) -> usize {
        let b = text.as_bytes();
        if b.first() != Some(&b'<') {
            return 0;
        }
        match b.iter().position(|&c| c == b'>' || c < 32) {
            Some(i) if b[i] == b'>' => i + 1,
            _ => 0,
        }
    }

    /// The built-in content for a tag. TB: TBTextFragmentContentFactory::CreateFragmentContent
    pub fn from_tag(tag: &str) -> Option<Content> {
        if tag == "<hr>" {
            Some(Content::Hr { width_percent: 100, height: 2 })
        } else if tag == "<u>" {
            Some(Content::Underline)
        } else if let Some(rest) = tag.strip_prefix("<color ") {
            let color = rest.strip_suffix('>').unwrap_or(rest).trim();
            Some(Content::TextColor(Color::parse(color)))
        } else if tag.starts_with("</") {
            Some(Content::StylePop)
        } else {
            None
        }
    }

    /// TB: GetAllowBreakBefore
    pub(crate) fn allow_break_before(&self) -> bool {
        !matches!(self, Content::StylePop)
    }

    /// TB: GetAllowBreakAfter
    pub(crate) fn allow_break_after(&self) -> bool {
        !matches!(self, Content::Underline | Content::TextColor(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        assert_eq!(Content::tag_len("<u>x"), 3);
        assert_eq!(Content::tag_len("<color #ff0000>x"), 15);
        assert_eq!(Content::tag_len("a<u>"), 0);
        assert_eq!(Content::tag_len("<not closed"), 0);
        assert_eq!(Content::tag_len("<broken\n>"), 0);
        assert_eq!(Content::from_tag("<hr>"), Some(Content::Hr { width_percent: 100, height: 2 }));
        assert_eq!(Content::from_tag("<u>"), Some(Content::Underline));
        assert_eq!(Content::from_tag("</u>"), Some(Content::StylePop));
        assert_eq!(Content::from_tag("</color>"), Some(Content::StylePop));
        assert_eq!(Content::from_tag("<color #0794f8>"), Some(Content::TextColor(Color::rgb(0x07, 0x94, 0xf8))));
        assert_eq!(Content::from_tag("<b>"), None);
    }
}
