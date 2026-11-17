//! Text measurement and radio groups for widgets. Font metrics come from the font
//! manager (wired when fonts land; until then a fixed-width fallback).

use crate::arena::WidgetId;
use crate::font::FontDesc;
use crate::geom::Point;
use crate::value::Value;

use super::Ui;
use super::node::NodeFlags;

impl Ui {
    /// Width of `text` in `font`. TB: TBFontFace::GetStringWidth
    pub fn text_width(&self, font: FontDesc, text: &str) -> i32 {
        let _ = font;
        text.chars().count() as i32 * 8
    }

    /// Line height of `font`. TB: TBFontFace::GetHeight
    pub fn font_height(&self, font: FontDesc) -> i32 {
        let _ = font;
        16
    }

    /// Where the current press started, in the captured widget's coordinates.
    pub fn pointer_down_pos(&self) -> Point {
        self.input.down_pos
    }

    /// Last pointer position, in the captured (or hovered) widget's coordinates.
    pub fn pointer_move_pos(&self) -> Point {
        self.input.move_pos
    }

    pub fn is_click_cancelled(&self) -> bool {
        self.input.cancel_click
    }

    /// Set value 0 on every other widget with `leader`'s group id under the nearest
    /// group root. TB: TBRadioCheckBox::UpdateGroupWidgets
    pub fn update_group_widgets(&mut self, leader: WidgetId) {
        let Some(n) = self.tree.live(leader) else { return };
        let gid = n.group_id;
        if gid.is_none() {
            return;
        }
        let mut group = leader;
        while !self.has_flag(group, NodeFlags::GROUP_ROOT) {
            match self.parent(group) {
                Some(p) => group = p,
                None => break,
            }
        }
        let mut cur = Some(group);
        let mut members = alloc::vec::Vec::new();
        while let Some(c) = cur {
            if c != leader && self.tree.live(c).is_some_and(|n| n.group_id == gid) {
                members.push(c);
            }
            cur = self.tree.next_deep(c, Some(group));
        }
        for m in members {
            self.set_value(m, Value::Int(0));
        }
    }
}
