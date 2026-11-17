//! Size negotiation types. TB: tb_widgets.h (SizeConstraints, PreferredSize,
//! LayoutParams, WIDGET_GRAVITY, WIDGET_VISIBILITY, SIZE_DEP).

use crate::flags::flags;
use crate::geom::Size;

/// "No restriction" for available sizes and max sizes (as TB).
pub const NO_RESTRICTION: i32 = 10_000;

flags! {
    /// TB: SIZE_DEP
    pub struct SizeDep: u8 {
        const WIDTH_DEPEND_ON_HEIGHT = 1;
        const HEIGHT_DEPEND_ON_WIDTH = 2;
        const BOTH = 3;
    }
}

/// Available space during measurement. TB: SizeConstraints
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SizeConstraints {
    pub available: Size,
}

impl Default for SizeConstraints {
    fn default() -> Self {
        SizeConstraints::NONE
    }
}

impl SizeConstraints {
    pub const NONE: SizeConstraints = SizeConstraints { available: Size::new(NO_RESTRICTION, NO_RESTRICTION) };

    pub const fn new(w: i32, h: i32) -> Self {
        SizeConstraints { available: Size::new(w, h) }
    }

    /// TB: SizeConstraints::ConstrainByPadding
    pub fn constrain_by_padding(self, horizontal: i32, vertical: i32) -> Self {
        let f = |a: i32, p: i32| if a == NO_RESTRICTION { NO_RESTRICTION } else { a - p };
        SizeConstraints::new(f(self.available.w, horizontal), f(self.available.h, vertical))
    }

    /// TB: SizeConstraints::ConstrainByLayoutParams
    pub fn constrain_by_layout_params(self, lp: &LayoutParams) -> Self {
        fn by(c: i32, min: Option<i32>, max: Option<i32>) -> i32 {
            if c == NO_RESTRICTION {
                return max.unwrap_or(NO_RESTRICTION);
            }
            let mut r = c;
            if let Some(m) = min {
                r = r.max(m);
            }
            if let Some(m) = max {
                r = r.min(m);
            }
            r
        }
        SizeConstraints::new(by(self.available.w, lp.min_w, lp.max_w), by(self.available.h, lp.min_h, lp.max_h))
    }
}

/// Size preferences computed during measure. TB: PreferredSize
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PreferredSize {
    pub min: Size,
    pub max: Size,
    pub pref: Size,
    pub dependency: SizeDep,
}

impl Default for PreferredSize {
    fn default() -> Self {
        PreferredSize {
            min: Size::ZERO,
            max: Size::new(NO_RESTRICTION, NO_RESTRICTION),
            pref: Size::ZERO,
            dependency: SizeDep::NONE,
        }
    }
}

impl PreferredSize {
    /// Fixed size (min = max = pref). TB: PreferredSize(w, h)
    pub const fn fixed(w: i32, h: i32) -> Self {
        PreferredSize { min: Size::new(w, h), max: Size::new(w, h), pref: Size::new(w, h), dependency: SizeDep::NONE }
    }
    pub const ZERO: PreferredSize = PreferredSize::fixed(0, 0);
}

/// Per-widget overrides of the preferred size. TB: LayoutParams (UNSPECIFIED = None)
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct LayoutParams {
    pub min_w: Option<i32>,
    pub min_h: Option<i32>,
    pub max_w: Option<i32>,
    pub max_h: Option<i32>,
    pub pref_w: Option<i32>,
    pub pref_h: Option<i32>,
}

impl LayoutParams {
    pub fn fixed(w: i32, h: i32) -> Self {
        let mut lp = LayoutParams::default();
        lp.set_width(w);
        lp.set_height(h);
        lp
    }
    pub fn set_width(&mut self, w: i32) {
        self.min_w = Some(w);
        self.max_w = Some(w);
        self.pref_w = Some(w);
    }
    pub fn set_height(&mut self, h: i32) {
        self.min_h = Some(h);
        self.max_h = Some(h);
        self.pref_h = Some(h);
    }
    /// Apply the overrides to a computed size. TB: TBWidget::GetPreferredSize LP_OVERRIDE
    pub fn apply(&self, ps: &mut PreferredSize) {
        if let Some(v) = self.min_w {
            ps.min.w = v;
        }
        if let Some(v) = self.min_h {
            ps.min.h = v;
        }
        if let Some(v) = self.max_w {
            ps.max.w = v;
        }
        if let Some(v) = self.max_h {
            ps.max.h = v;
        }
        if let Some(v) = self.pref_w {
            ps.pref.w = v;
        }
        if let Some(v) = self.pref_h {
            ps.pref.h = v;
        }
        ps.max.w = ps.max.w.max(ps.min.w);
        ps.max.h = ps.max.h.max(ps.min.h);
        ps.pref.w = ps.pref.w.max(ps.min.w);
        ps.pref.h = ps.pref.h.max(ps.min.h);
    }
}

flags! {
    /// Which parent edges a child follows on resize. TB: WIDGET_GRAVITY
    pub struct Gravity: u8 {
        const LEFT = 1;
        const RIGHT = 2;
        const TOP = 4;
        const BOTTOM = 8;
        const LEFT_RIGHT = 3;
        const TOP_BOTTOM = 12;
        const ALL = 15;
    }
}

impl Gravity {
    /// TB: WIDGET_GRAVITY_DEFAULT
    pub const DEFAULT: Gravity = Gravity(1 | 4);

    /// Parse TB's resource syntax: "left right", "all", "top bottom" ...
    /// TB: tb_widgets_reader.cpp StringToGravity
    pub fn parse(s: &str) -> Gravity {
        let mut g = Gravity::NONE;
        if s.contains("left") {
            g |= Gravity::LEFT;
        }
        if s.contains("top") {
            g |= Gravity::TOP;
        }
        if s.contains("right") {
            g |= Gravity::RIGHT;
        }
        if s.contains("bottom") {
            g |= Gravity::BOTTOM;
        }
        if s.contains("all") {
            g |= Gravity::ALL;
        }
        if !g.intersects(Gravity::LEFT_RIGHT) {
            g |= Gravity::LEFT;
        }
        if !g.intersects(Gravity::TOP_BOTTOM) {
            g |= Gravity::TOP;
        }
        g
    }
    pub fn fills_x(self) -> bool {
        self.contains(Gravity::LEFT_RIGHT)
    }
    pub fn fills_y(self) -> bool {
        self.contains(Gravity::TOP_BOTTOM)
    }
}

/// TB: WIDGET_VISIBILITY
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Visibility {
    #[default]
    Visible,
    /// Invisible but laid out. No interaction.
    Invisible,
    /// Invisible and not laid out. No interaction.
    Gone,
}

impl Visibility {
    pub fn parse(s: &str) -> Visibility {
        match s {
            "invisible" => Visibility::Invisible,
            "gone" => Visibility::Gone,
            _ => Visibility::Visible,
        }
    }
}
