//! Integer geometry and colors.
//!
//! TB: tb_geometry.h, tb_geometry.cpp, tb_color.h, tb_color.cpp

use alloc::vec::Vec;
use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0, y: 0 };
    pub const fn new(x: i32, y: i32) -> Point {
        Point { x, y }
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}
impl AddAssign for Point {
    fn add_assign(&mut self, o: Point) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl SubAssign for Point {
    fn sub_assign(&mut self, o: Point) {
        self.x -= o.x;
        self.y -= o.y;
    }
}
impl Neg for Point {
    type Output = Point;
    fn neg(self) -> Point {
        Point::new(-self.x, -self.y)
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

impl Size {
    pub const ZERO: Size = Size { w: 0, h: 0 };
    pub const fn new(w: i32, h: i32) -> Size {
        Size { w, h }
    }
    pub fn max(self, o: Size) -> Size {
        Size::new(self.w.max(o.w), self.h.max(o.h))
    }
    pub fn min(self, o: Size) -> Size {
        Size::new(self.w.min(o.w), self.h.min(o.h))
    }
}

/// Integer rectangle: origin plus size. TB: TBRect
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const ZERO: Rect = Rect { x: 0, y: 0, w: 0, h: 0 };

    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
    pub const fn from_size(s: Size) -> Rect {
        Rect::new(0, 0, s.w, s.h)
    }
    pub const fn from_origin_size(p: Point, s: Size) -> Rect {
        Rect::new(p.x, p.y, s.w, s.h)
    }
    pub const fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }
    pub const fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }
    pub const fn right(&self) -> i32 {
        self.x + self.w
    }
    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    pub const fn is_inside_out(&self) -> bool {
        self.w < 0 || self.h < 0
    }
    pub const fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.w && p.y < self.y + self.h
    }
    pub const fn shrink(&self, l: i32, t: i32, r: i32, b: i32) -> Rect {
        Rect::new(self.x + l, self.y + t, self.w - l - r, self.h - t - b)
    }
    pub const fn shrink_edges(&self, e: Edges) -> Rect {
        self.shrink(e.left, e.top, e.right, e.bottom)
    }
    pub const fn expand(&self, l: i32, t: i32, r: i32, b: i32) -> Rect {
        self.shrink(-l, -t, -r, -b)
    }
    pub const fn shrink_xy(&self, tx: i32, ty: i32) -> Rect {
        Rect::new(self.x + tx, self.y + ty, self.w - tx * 2, self.h - ty * 2)
    }
    pub const fn expand_xy(&self, tx: i32, ty: i32) -> Rect {
        self.shrink_xy(-tx, -ty)
    }
    pub const fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
    pub const fn offset_by(&self, p: Point) -> Rect {
        self.offset(p.x, p.y)
    }

    /// TB: TBRect::Intersects
    pub const fn intersects(&self, r: &Rect) -> bool {
        if self.is_empty() || r.is_empty() {
            return false;
        }
        self.x + self.w > r.x && self.x < r.x + r.w && self.y + self.h > r.y && self.y < r.y + r.h
    }

    /// Move inside `b` (clamped; a rect larger than `b` is aligned to its top left).
    /// TB: TBRect::MoveIn
    pub fn move_in(&self, b: &Rect) -> Rect {
        let clamp = |v: i32, lo: i32, hi: i32| if v > hi { hi.max(lo) } else { v.max(lo) };
        // TB's ClampClipMax: clamp to [min, max] but min wins.
        Rect::new(clamp(self.x, b.x, b.x + b.w - self.w), clamp(self.y, b.y, b.y + b.h - self.h), self.w, self.h)
    }

    /// TB: TBRect::CenterIn
    pub const fn center_in(&self, b: &Rect) -> Rect {
        Rect::new(b.x + (b.w - self.w) / 2, b.y + (b.h - self.h) / 2, self.w, self.h)
    }

    /// TB: TBRect::Union
    pub fn union(&self, r: &Rect) -> Rect {
        if self.is_empty() {
            return *r;
        }
        if r.is_empty() {
            return *self;
        }
        let minx = self.x.min(r.x);
        let miny = self.y.min(r.y);
        let maxx = self.right().max(r.right());
        let maxy = self.bottom().max(r.bottom());
        Rect::new(minx, miny, maxx - minx, maxy - miny)
    }

    /// Intersection (empty `Rect::ZERO` if they do not intersect). TB: TBRect::Clip
    pub fn clip(&self, c: &Rect) -> Rect {
        if !self.intersects(c) {
            return Rect::ZERO;
        }
        let x = self.x.max(c.x);
        let y = self.y.max(c.y);
        Rect::new(x, y, self.right().min(c.right()) - x, self.bottom().min(c.bottom()) - y)
    }
}

/// Padding, expansion and similar per-side values.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Edges {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Edges {
    pub const ZERO: Edges = Edges { left: 0, top: 0, right: 0, bottom: 0 };
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Edges {
        Edges { left, top, right, bottom }
    }
    pub const fn all(v: i32) -> Edges {
        Edges::new(v, v, v, v)
    }
    pub const fn horizontal(&self) -> i32 {
        self.left + self.right
    }
    pub const fn vertical(&self) -> i32 {
        self.top + self.bottom
    }
}

/// A set of non-overlapping rectangles. TB: TBRegion
#[derive(Clone, Debug, Default)]
pub struct Region {
    rects: Vec<Rect>,
}

impl Region {
    pub fn new() -> Region {
        Region::default()
    }
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }
    pub fn len(&self) -> usize {
        self.rects.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
    pub fn clear(&mut self) {
        self.rects.clear();
    }
    /// TB: TBRegion::Set
    pub fn set(&mut self, r: Rect) {
        self.rects.clear();
        self.add_rect(r, false);
    }

    /// TB: TBRegion::AddRect
    pub fn add_rect(&mut self, rect: Rect, coalesce: bool) {
        if coalesce {
            for i in (0..self.rects.len()).rev() {
                let e = self.rects[i];
                let vert = rect.x == e.x && rect.w == e.w && (rect.y == e.bottom() || rect.bottom() == e.y);
                let horiz = rect.y == e.y && rect.h == e.h && (rect.x == e.right() || rect.right() == e.x);
                if vert || horiz {
                    let u = e.union(&rect);
                    self.rects.swap_remove(i); // TB: RemoveRectFast
                    self.add_rect(u, true);
                    return;
                }
            }
        }
        self.rects.push(rect);
    }

    /// Add the area of `r`, keeping rects non-overlapping. TB: TBRegion::IncludeRect
    pub fn include_rect(&mut self, r: Rect) {
        for i in 0..self.rects.len() {
            if r.intersects(&self.rects[i]) {
                let mut parts = Region::new();
                parts.add_excluding_rects(r, self.rects[i], false);
                for p in parts.rects {
                    self.include_rect(p);
                }
                return;
            }
        }
        self.add_rect(r, true);
    }

    /// Remove the area of `r`. TB: TBRegion::ExcludeRect
    pub fn exclude_rect(&mut self, ex: Rect) {
        let mut to_check = self.rects.len();
        let mut i = 0;
        while i < to_check {
            if self.rects[i].intersects(&ex) {
                let r = self.rects.remove(i);
                to_check -= 1;
                self.add_excluding_rects(r, ex, true);
            } else {
                i += 1;
            }
        }
    }

    /// TB: TBRegion::AddExcludingRects
    fn add_excluding_rects(&mut self, rect: Rect, ex: Rect, coalesce: bool) {
        let rm = ex.clip(&rect);
        if rm.y > rect.y {
            self.add_rect(Rect::new(rect.x, rect.y, rect.w, rm.y - rect.y), coalesce);
        }
        if rm.x > rect.x {
            self.add_rect(Rect::new(rect.x, rm.y, rm.x - rect.x, rm.h), coalesce);
        }
        if rm.right() < rect.right() {
            self.add_rect(Rect::new(rm.right(), rm.y, rect.right() - rm.right(), rm.h), coalesce);
        }
        if rm.bottom() < rect.bottom() {
            self.add_rect(Rect::new(rect.x, rm.bottom(), rect.w, rect.bottom() - rm.bottom()), coalesce);
        }
    }
}

/// Straight (non-premultiplied) RGBA color. TB: TBColor
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Default for Color {
    fn default() -> Self {
        Color::BLACK
    }
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// Parse `#rrggbbaa`, `#rrggbb`, `#rgba` or `#rgb`. Anything else gives opaque black,
    /// as TB does. TB: TBColor::SetFromString
    pub fn parse(s: &str) -> Color {
        Self::try_parse(s).unwrap_or(Color::BLACK)
    }

    pub fn try_parse(s: &str) -> Option<Color> {
        let s = s.trim();
        let hex = s.strip_prefix('#')?;
        let b = hex.as_bytes();
        let nib = |i: usize| -> Option<u8> { (b[i] as char).to_digit(16).map(|v| v as u8) };
        let byte = |i: usize| -> Option<u8> { Some(nib(i)? << 4 | nib(i + 1)?) };
        let dup = |i: usize| -> Option<u8> { nib(i).map(|v| v << 4 | v) };
        match b.len() {
            8 => Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            6 => Some(Color::rgb(byte(0)?, byte(2)?, byte(4)?)),
            4 => Some(Color::rgba(dup(0)?, dup(1)?, dup(2)?, dup(3)?)),
            3 => Some(Color::rgb(dup(0)?, dup(1)?, dup(2)?)),
            _ => None,
        }
    }

    /// Multiply alpha by `opacity` (0..1).
    pub fn with_opacity(self, opacity: f32) -> Color {
        let a = (self.a as f32 * opacity.clamp(0.0, 1.0) + 0.5) as u8;
        Color { a, ..self }
    }

    /// Premultiplied copy (for quads and pixel data).
    pub fn premultiplied(self) -> Color {
        let m = |c: u8| ((c as u32 * self.a as u32 + 127) / 255) as u8;
        Color::rgba(m(self.r), m(self.g), m(self.b), self.a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TB: tests/test_tb_color.cpp
    #[test]
    fn color_from_string() {
        assert_eq!(Color::parse("#11223344"), Color::rgba(0x11, 0x22, 0x33, 0x44));
        assert_eq!(Color::parse("#112233"), Color::rgba(0x11, 0x22, 0x33, 0xff));
        assert_eq!(Color::parse("#1234"), Color::rgba(0x11, 0x22, 0x33, 0x44));
        assert_eq!(Color::parse("#123"), Color::rgba(0x11, 0x22, 0x33, 0xff));
        assert_eq!(Color::parse("123"), Color::rgba(0, 0, 0, 0xff));
        assert_eq!(Color::parse("#12345"), Color::BLACK);
        assert_eq!(Color::parse("#zz2233"), Color::BLACK);
    }

    // TB: tests/test_tb_geometry.cpp
    #[test]
    fn region_include() {
        let mut r = Region::new();
        r.include_rect(Rect::new(10, 10, 100, 100));
        r.include_rect(Rect::new(50, 50, 100, 100));
        assert_eq!(r.rects(), &[Rect::new(10, 10, 100, 100), Rect::new(110, 50, 40, 60), Rect::new(50, 110, 100, 40)]);
    }

    #[test]
    fn region_include_adjacent_coalesce() {
        let mut r = Region::new();
        r.include_rect(Rect::new(10, 10, 10, 10));
        r.include_rect(Rect::new(20, 10, 10, 10));
        assert_eq!(r.rects(), &[Rect::new(10, 10, 20, 10)]);
        r.include_rect(Rect::new(10, 20, 20, 10));
        assert_eq!(r.rects(), &[Rect::new(10, 10, 20, 20)]);
        r.include_rect(Rect::new(0, 10, 10, 20));
        assert_eq!(r.rects(), &[Rect::new(0, 10, 30, 20)]);
        r.include_rect(Rect::new(0, 0, 30, 10));
        assert_eq!(r.rects(), &[Rect::new(0, 0, 30, 30)]);
    }

    #[test]
    fn region_include_overlap_coalesce() {
        let mut r = Region::new();
        r.include_rect(Rect::new(10, 10, 10, 10));
        r.include_rect(Rect::new(0, 10, 30, 10));
        assert_eq!(r.rects(), &[Rect::new(0, 10, 30, 10)]);
    }

    #[test]
    fn region_include_overlap_multi_coalesce() {
        let mut r = Region::new();
        r.include_rect(Rect::new(10, 10, 10, 10));
        r.include_rect(Rect::new(0, 0, 100, 100));
        assert_eq!(r.rects(), &[Rect::new(0, 0, 100, 100)]);
    }

    #[test]
    fn region_exclude() {
        let mut r = Region::new();
        r.set(Rect::new(100, 100, 100, 100));
        r.exclude_rect(Rect::new(140, 140, 20, 20));
        assert_eq!(
            r.rects(),
            &[
                Rect::new(100, 100, 100, 40),
                Rect::new(100, 140, 40, 20),
                Rect::new(160, 140, 40, 20),
                Rect::new(100, 160, 100, 40)
            ]
        );
        r.exclude_rect(Rect::new(130, 130, 40, 40));
        assert_eq!(
            r.rects(),
            &[
                Rect::new(100, 100, 100, 30),
                Rect::new(100, 130, 30, 40),
                Rect::new(170, 130, 30, 40),
                Rect::new(100, 170, 100, 30)
            ]
        );
    }

    #[test]
    fn rect_ops() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert!(a.intersects(&b));
        assert_eq!(a.clip(&b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert!(!a.intersects(&Rect::new(10, 0, 5, 5)));
        assert_eq!(Rect::new(50, 50, 10, 10).move_in(&a), Rect::new(0, 0, 10, 10));
        assert_eq!(Rect::new(0, 0, 4, 4).center_in(&a), Rect::new(3, 3, 4, 4));
    }
}
