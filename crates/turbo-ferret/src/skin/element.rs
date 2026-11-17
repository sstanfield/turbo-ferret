//! Skin elements and state elements. TB: tb_skin.h (TBSkinElement, TBSkinElementState)

use alloc::string::String;
use alloc::vec::Vec;

use crate::arena::WidgetId;
use crate::geom::{Color, Edges, Point};
use crate::id::TbId;
use crate::render::atlas::FragmentId;
use crate::ui::View;
use crate::value::Value;
use crate::widget::SkinState;

use super::condition::Condition;

/// TB: SKIN_ELEMENT_TYPE
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum SkinKind {
    #[default]
    StretchBox,
    StretchBorder,
    StretchImage,
    Tile,
    Image,
}

impl SkinKind {
    /// TB: tb_skin.cpp StringToType
    pub fn parse(s: &str) -> SkinKind {
        match s {
            "Image" => SkinKind::Image,
            "Stretch Image" => SkinKind::StretchImage,
            "Tile" => SkinKind::Tile,
            "StretchBorder" => SkinKind::StretchBorder,
            _ => SkinKind::StretchBox,
        }
    }
}

/// An element to use when the state and conditions match. TB: TBSkinElementState
#[derive(Clone, Debug)]
pub struct StateElement {
    pub element: TbId,
    pub state: SkinState,
    pub conditions: Vec<Condition>,
}

/// TB: TBSkinElementState::MATCH_RULE
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MatchRule {
    Default,
    OnlySpecificState,
}

impl StateElement {
    fn conditions_hold(&self, v: &View<'_>, id: WidgetId) -> bool {
        self.conditions.iter().all(|c| c.eval(v, id))
    }

    /// TB: TBSkinElementState::IsMatch
    pub(crate) fn is_match(&self, state: SkinState, v: &View<'_>, id: WidgetId, rule: MatchRule) -> bool {
        if rule == MatchRule::OnlySpecificState && self.state == SkinState::ALL {
            return false;
        }
        (state.intersects(self.state) || self.state == SkinState::ALL) && self.conditions_hold(v, id)
    }

    /// TB: TBSkinElementState::IsExactMatch
    pub(crate) fn is_exact_match(&self, state: SkinState, v: &View<'_>, id: WidgetId, rule: MatchRule) -> bool {
        if rule == MatchRule::OnlySpecificState && self.state == SkinState::ALL {
            return false;
        }
        (state == self.state || self.state == SkinState::ALL) && self.conditions_hold(v, id)
    }
}

/// TB: TBSkinElementStateList::GetStateElement (exact match first, then any match).
pub(crate) fn find_state_element<'a>(
    list: &'a [StateElement],
    state: SkinState,
    v: &View<'_>,
    id: WidgetId,
    rule: MatchRule,
) -> Option<&'a StateElement> {
    list.iter()
        .find(|e| e.is_exact_match(state, v, id, rule))
        .or_else(|| list.iter().find(|e| e.is_match(state, v, id, rule)))
}

/// Sizes from the skin. `None` = not specified (TB: SKIN_VALUE_NOT_SPECIFIED).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct SkinSizes {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub pref_width: Option<i32>,
    pub pref_height: Option<i32>,
    pub min_width: Option<i32>,
    pub min_height: Option<i32>,
    pub max_width: Option<i32>,
    pub max_height: Option<i32>,
}

/// A skin element: a bitmap and how to paint it, plus padding and size settings.
/// TB: TBSkinElement
#[derive(Clone, Debug)]
pub struct SkinElement {
    pub id: TbId,
    pub name: String,
    /// Resolved path of the bitmap file (may be empty).
    pub bitmap_file: String,
    pub bitmap: Option<FragmentId>,
    pub kind: SkinKind,
    pub cut: i32,
    pub expand: i32,
    pub padding: Edges,
    pub sizes: SkinSizes,
    pub spacing: Option<i32>,
    pub content_ofs: Point,
    pub img_ofs: Point,
    /// 0..100 (left to right, top to bottom).
    pub img_position: Point,
    pub flip_x: bool,
    pub flip_y: bool,
    pub opacity: f32,
    pub text_color: Option<Color>,
    pub bg_color: Option<Color>,
    /// DPI the bitmap was loaded at (0 = not yet).
    pub bitmap_dpi: i32,
    pub tag: Value,
    pub overrides: Vec<StateElement>,
    pub strong_overrides: Vec<StateElement>,
    pub children: Vec<StateElement>,
    pub overlays: Vec<StateElement>,
}

impl SkinElement {
    pub fn new(name: &str) -> SkinElement {
        SkinElement {
            id: TbId::new(name),
            name: String::from(name),
            bitmap_file: String::new(),
            bitmap: None,
            kind: SkinKind::StretchBox,
            cut: 0,
            expand: 0,
            padding: Edges::ZERO,
            sizes: SkinSizes::default(),
            spacing: None,
            content_ofs: Point::ZERO,
            img_ofs: Point::ZERO,
            img_position: Point::new(50, 50),
            flip_x: false,
            flip_y: false,
            opacity: 1.0,
            text_color: None,
            bg_color: None,
            bitmap_dpi: 0,
            tag: Value::Null,
            overrides: Vec::new(),
            strong_overrides: Vec::new(),
            children: Vec::new(),
            overlays: Vec::new(),
        }
    }

    fn bitmap_size(&self) -> Option<(i32, i32)> {
        self.bitmap.map(|f| (f.rect.w, f.rect.h))
    }

    /// TB: GetIntrinsicMinWidth
    pub fn intrinsic_min_width(&self) -> i32 {
        match self.bitmap_size() {
            Some((w, _)) if self.kind == SkinKind::Image => w - self.expand * 2,
            _ => self.cut * 2 - self.expand * 2,
        }
    }

    /// TB: GetIntrinsicMinHeight
    pub fn intrinsic_min_height(&self) -> i32 {
        match self.bitmap_size() {
            Some((_, h)) if self.kind == SkinKind::Image => h - self.expand * 2,
            _ => self.cut * 2 - self.expand * 2,
        }
    }

    /// TB: GetIntrinsicWidth
    pub fn intrinsic_width(&self) -> Option<i32> {
        self.sizes.width.or_else(|| self.bitmap_size().map(|(w, _)| w - self.expand * 2))
    }

    /// TB: GetIntrinsicHeight
    pub fn intrinsic_height(&self) -> Option<i32> {
        self.sizes.height.or_else(|| self.bitmap_size().map(|(_, h)| h - self.expand * 2))
    }

    pub fn has_overlays(&self) -> bool {
        !self.overlays.is_empty()
    }

    /// An exact or partial match for `state` in the override, child or overlay lists,
    /// ignoring "all" entries. TB: TBSkinElement::HasState
    pub(crate) fn has_state(&self, state: SkinState, v: &View<'_>, id: WidgetId) -> bool {
        let r = MatchRule::OnlySpecificState;
        find_state_element(&self.overrides, state, v, id, r).is_some()
            || find_state_element(&self.children, state, v, id, r).is_some()
            || find_state_element(&self.overlays, state, v, id, r).is_some()
    }
}
