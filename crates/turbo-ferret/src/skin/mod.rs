//! Skin: elements, states, conditions, loading and painting (port-design.md section 13).
//!
//! TB: tb_skin.cpp / tb_skin.h

pub mod condition;
pub mod element;
pub mod load;
pub mod paint;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

pub use condition::{CondProp, CondTarget, Condition, Test};
pub use element::{SkinElement, SkinKind, SkinSizes, StateElement};

use crate::arena::WidgetId;
use crate::geom::{Color, Size};
use crate::id::TbId;
use crate::layout::dim::DimensionConverter;
use crate::render::atlas::Atlas;
use crate::res::ParseError;
use crate::ui::View;
use crate::widget::SkinState;

use element::{MatchRule, find_state_element};

/// Override chains stop resolving at this depth (M-07: replaces TB's is_painting /
/// is_getting flags, so reads stay reads).
pub const MAX_DEPTH: u32 = 8;

/// TB: TBSkin's defaults section.
#[derive(Clone, Debug)]
pub struct SkinDefaults {
    pub text_color: Color,
    pub disabled_opacity: f32,
    pub placeholder_opacity: f32,
    pub spacing: i32,
}

impl Default for SkinDefaults {
    fn default() -> Self {
        SkinDefaults { text_color: Color::BLACK, disabled_opacity: 0.3, placeholder_opacity: 0.2, spacing: 0 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkinError {
    Parse(ParseError),
    /// The skin loaded, but these bitmaps could not be loaded or decoded.
    MissingBitmaps(Vec<String>),
}

impl fmt::Display for SkinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkinError::Parse(e) => write!(f, "{e}"),
            SkinError::MissingBitmaps(v) => write!(f, "missing skin bitmaps: {}", v.join(", ")),
        }
    }
}

impl From<ParseError> for SkinError {
    fn from(e: ParseError) -> Self {
        SkinError::Parse(e)
    }
}

/// TB: TBSkin
pub struct Skin {
    pub(crate) elements: BTreeMap<TbId, SkinElement>,
    pub(crate) atlas: Atlas,
    pub(crate) dim: DimensionConverter,
    pub defaults: SkinDefaults,
    /// Bitmap file -> fragment, so files used by several elements load once.
    pub(crate) files: BTreeMap<String, crate::render::atlas::FragmentId>,
}

impl Default for Skin {
    fn default() -> Self {
        Skin {
            elements: BTreeMap::new(),
            atlas: Atlas::new(Size::new(1024, 1024)),
            dim: DimensionConverter::default(),
            defaults: SkinDefaults::default(),
            files: BTreeMap::new(),
        }
    }
}

impl Skin {
    pub fn element(&self, id: TbId) -> Option<&SkinElement> {
        if id.is_none() {
            return None;
        }
        self.elements.get(&id)
    }

    pub fn element_mut(&mut self, id: TbId) -> Option<&mut SkinElement> {
        self.elements.get_mut(&id)
    }

    /// Add or replace an element (for code-built skins and tests).
    pub fn insert(&mut self, e: SkinElement) {
        self.elements.insert(e.id, e);
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub fn elements(&self) -> impl Iterator<Item = &SkinElement> {
        self.elements.values()
    }

    pub fn dimension_converter(&self) -> &DimensionConverter {
        &self.dim
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// The element for `id`, following strong overrides that match `state`.
    /// TB: TBSkin::GetSkinElementStrongOverride
    pub(crate) fn strong_override(
        &self,
        id: TbId,
        state: SkinState,
        v: &View<'_>,
        w: WidgetId,
    ) -> Option<&SkinElement> {
        self.strong_override_depth(id, state, v, w, 0)
    }

    fn strong_override_depth(
        &self,
        id: TbId,
        state: SkinState,
        v: &View<'_>,
        w: WidgetId,
        depth: u32,
    ) -> Option<&SkinElement> {
        let e = self.element(id)?;
        if depth >= MAX_DEPTH {
            return None;
        }
        if let Some(o) = find_state_element(&e.strong_overrides, state, v, w, MatchRule::Default) {
            if let Some(oe) = self.strong_override_depth(o.element, state, v, w, depth + 1) {
                return Some(oe);
            }
        }
        Some(e)
    }
}
