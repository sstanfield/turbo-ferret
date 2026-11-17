//! Skin conditions. TB: tb_skin.h TBSkinCondition

use crate::id::TbId;

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
