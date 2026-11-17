//! Text measurement and radio groups for widgets. Font metrics come from the font
//! manager (wired when fonts land; until then a fixed-width fallback).

use crate::arena::WidgetId;
use crate::font::{FontDesc, FontManager, LoadCtx};
use crate::geom::Point;
use crate::id::TbId;
use crate::value::Value;

use super::Ui;
use super::node::NodeFlags;

impl Ui {
    /// Width of `text` in `font`. TB: TBFontFace::GetStringWidth
    pub fn text_width(&self, font: FontDesc, text: &str) -> i32 {
        self.fonts.face(font).string_width(text)
    }

    /// Line height of `font`. TB: TBFontFace::GetHeight
    pub fn font_height(&self, font: FontDesc) -> i32 {
        self.fonts.face(font).height()
    }

    pub fn fonts(&self) -> &FontManager {
        &self.fonts
    }

    pub fn fonts_mut(&mut self) -> &mut FontManager {
        &mut self.fonts
    }

    /// Register a font file under a name. TB: TBFontManager::AddFontInfo
    pub fn add_font(&mut self, name: &str, path: &str) -> TbId {
        self.fonts.add_font(name, path)
    }

    /// Load a face (font + size) through the services. TB: CreateFontFace
    pub fn create_font_face(&mut self, desc: FontDesc) -> bool {
        let mut cx = LoadCtx::new(&mut *self.services.loader, &mut *self.services.images);
        self.fonts.create_face(desc, &mut cx)
    }

    /// Create the face and make it the default for widgets without a font.
    pub fn set_default_font(&mut self, desc: FontDesc) -> bool {
        let ok = self.create_font_face(desc);
        self.fonts.set_default(desc);
        self.default_font = desc;
        let root = self.root();
        self.invoke_font_changed(root);
        ok
    }

    /// The text on the clipboard, if any. TB: TBClipboard::GetText
    pub fn clipboard_text(&mut self) -> Option<alloc::string::String> {
        self.services.clipboard.text()
    }

    /// TB: TBClipboard::SetText
    pub fn set_clipboard_text(&mut self, text: &str) {
        self.services.clipboard.set_text(text);
    }

    /// Ascent of a font (baseline from the top of a line).
    pub fn font_ascent(&self, font: FontDesc) -> i32 {
        self.fonts.face(font).ascent()
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
