//! Skin conditions and their evaluation against the tree (port-design.md section 13).
//!
//! TB: tb_skin.h (TBSkinCondition), tb_widget_skin_condition_context.cpp.

use crate::arena::WidgetId;
use crate::id::TbId;
use crate::ui::View;
use crate::widget::{Align, Axis, WidgetState};

/// What a condition looks at. TB: TBSkinCondition::TARGET
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CondTarget {
    This,
    Parent,
    Ancestors,
    PrevSibling,
    NextSibling,
}

/// Which property a condition tests. TB: TBSkinCondition::PROPERTY
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CondProp {
    Skin,
    WindowActive,
    Axis,
    Align,
    Id,
    State,
    Value,
    Hover,
    Capture,
    Focus,
    /// App-defined; `Widget::custom_skin_condition` answers it.
    Custom(TbId),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Test {
    Equal,
    NotEqual,
}

/// TB: TBSkinCondition
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Condition {
    pub target: CondTarget,
    pub prop: CondProp,
    pub value: TbId,
    pub test: Test,
}

impl CondTarget {
    /// TB: tb_skin.cpp StringToTarget
    pub fn parse(s: &str) -> CondTarget {
        match s {
            "parent" => CondTarget::Parent,
            "ancestors" => CondTarget::Ancestors,
            "prev sibling" => CondTarget::PrevSibling,
            "next sibling" => CondTarget::NextSibling,
            _ => CondTarget::This,
        }
    }
}

impl CondProp {
    /// TB: tb_skin.cpp StringToProperty
    pub fn parse(s: &str) -> CondProp {
        match s {
            "skin" => CondProp::Skin,
            "window active" => CondProp::WindowActive,
            "axis" => CondProp::Axis,
            "align" => CondProp::Align,
            "id" => CondProp::Id,
            "state" => CondProp::State,
            "value" => CondProp::Value,
            "hover" => CondProp::Hover,
            "capture" => CondProp::Capture,
            "focus" => CondProp::Focus,
            other => CondProp::Custom(TbId::new(other)),
        }
    }
}

impl Condition {
    /// TB: TBSkinCondition::GetCondition
    pub(crate) fn eval(&self, v: &View<'_>, id: WidgetId) -> bool {
        let equal = match self.target {
            CondTarget::This => self.prop_on(v, id),
            CondTarget::Parent => v.tree.parent(id).is_some_and(|p| self.prop_on(v, p)),
            // TB fell through to PREV_SIBLING after a failed ancestor search (missing
            // break); the port checks ancestors only.
            CondTarget::Ancestors => {
                let mut cur = v.tree.parent(id);
                let mut hit = false;
                while let Some(p) = cur {
                    if self.prop_on(v, p) {
                        hit = true;
                        break;
                    }
                    cur = v.tree.parent(p);
                }
                hit
            }
            CondTarget::PrevSibling => v.sibling(id, -1).is_some_and(|s| self.prop_on(v, s)),
            CondTarget::NextSibling => v.sibling(id, 1).is_some_and(|s| self.prop_on(v, s)),
        };
        equal == (self.test == Test::Equal)
    }

    /// TB: TBWidgetSkinConditionContext::GetCondition(widget, info)
    fn prop_on(&self, v: &View<'_>, w: WidgetId) -> bool {
        let Some(n) = v.tree.node_ref(w) else { return false };
        match self.prop {
            CondProp::Skin => n.skin() == self.value,
            CondProp::WindowActive => {
                v.tree.parent_window(w).is_some_and(|win| v.tree.node_ref(win).is_some_and(|x| x.is_active_window()))
            }
            CondProp::Axis => {
                // A busy widget (its hook is running) answers from its node's copy.
                let axis = n.behaviour().map(|b| b.axis()).unwrap_or(n.node.axis);
                TbId::new(if axis == Axis::X { "x" } else { "y" }) == self.value
            }
            CondProp::Align => {
                let Some(a) = n.behaviour().map(|b| b.align()).unwrap_or(n.node.align) else { return false };
                let s = match a {
                    Align::Left => "left",
                    Align::Top => "top",
                    Align::Right => "right",
                    Align::Bottom => "bottom",
                    Align::Center => "center",
                };
                TbId::new(s) == self.value
            }
            CondProp::Id => n.id() == self.value,
            CondProp::State => v.auto_state(w).bits() as u32 & self.value.0 != 0,
            CondProp::Value => n.behaviour().map(|b| b.value().as_int()).unwrap_or(0) == self.value.0 as i32,
            CondProp::Hover => v.input.hovered.is_some_and(|h| v.tree.is_ancestor_of(w, h)),
            CondProp::Capture => v.input.captured.is_some_and(|h| v.tree.is_ancestor_of(w, h)),
            CondProp::Focus => v.input.focused.is_some_and(|h| v.tree.is_ancestor_of(w, h)),
            CondProp::Custom(_) => n.behaviour().is_some_and(|b| b.custom_skin_condition(self)),
        }
    }
}

/// State value for `state` conditions: the state bits as an id. TB: value.Set(StringToState(..))
pub fn state_value(s: &str) -> TbId {
    TbId(WidgetState::parse(s).bits() as u32)
}
