//! Dimension conversion: `dp` (default), `px` and `mm`. TB: tb_dimension.cpp

use alloc::string::String;

use crate::math::round;
use crate::value::{Value, atof, atoi};

/// TB: TB_INVALID_DIMENSION
pub const INVALID_DIMENSION: i32 = -5555;

/// Converts resource dimensions to pixels. TB: TBDimensionConverter
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimensionConverter {
    /// Source DPI (the skin's `base-dpi`).
    pub src_dpi: i32,
    /// Destination DPI (the supported skin DPI nearest the screen DPI).
    pub dst_dpi: i32,
    /// Screen DPI, for `mm`.
    pub screen_dpi: i32,
}

impl Default for DimensionConverter {
    fn default() -> Self {
        DimensionConverter { src_dpi: 100, dst_dpi: 100, screen_dpi: 96 }
    }
}

impl DimensionConverter {
    pub fn new(src_dpi: i32, dst_dpi: i32, screen_dpi: i32) -> Self {
        DimensionConverter { src_dpi, dst_dpi, screen_dpi }
    }

    pub fn need_conversion(&self) -> bool {
        self.src_dpi != self.dst_dpi
    }

    /// File name suffix for bitmaps at the destination DPI ("@192"), empty if none.
    pub fn dst_dpi_suffix(&self) -> String {
        if self.need_conversion() { alloc::format!("@{}", self.dst_dpi) } else { String::new() }
    }

    /// "foo.png" -> "foo@192.png". TB: GetDstDPIFilename
    pub fn dst_dpi_filename(&self, name: &str) -> String {
        let suffix = self.dst_dpi_suffix();
        match name.rfind('.') {
            Some(dot) if dot > 0 => alloc::format!("{}{}{}", &name[..dot], suffix, &name[dot..]),
            _ => alloc::format!("{name}{suffix}"),
        }
    }

    pub fn dp_to_px_f(&self, dp: f32) -> f32 {
        if dp <= INVALID_DIMENSION as f32 || dp == 0.0 || !self.need_conversion() {
            return dp;
        }
        dp * self.dst_dpi as f32 / self.src_dpi as f32
    }

    pub fn dp_to_px(&self, dp: i32) -> i32 {
        round(self.dp_to_px_f(dp as f32)) as i32
    }

    pub fn mm_to_px_f(&self, mm: f32) -> f32 {
        if mm <= INVALID_DIMENSION as f32 || mm == 0.0 {
            return mm;
        }
        mm * self.screen_dpi as f32 / 25.4
    }

    pub fn mm_to_px(&self, mm: i32) -> i32 {
        round(self.mm_to_px_f(mm as f32)) as i32
    }

    /// "10", "10dp", "10px", "3mm". TB: GetPxFromString
    pub fn px_from_str(&self, s: &str, default: i32) -> i32 {
        if !starts_number(s) {
            return default;
        }
        let v = atoi(s);
        if s.len() > 2 && s.ends_with("px") {
            v
        } else if s.len() > 2 && s.ends_with("mm") {
            self.mm_to_px(v)
        } else {
            self.dp_to_px(v)
        }
    }

    pub fn px_from_str_f(&self, s: &str, default: f32) -> f32 {
        if !starts_number(s) {
            return default;
        }
        let v = atof(s);
        if s.len() > 2 && s.ends_with("px") {
            v
        } else if s.len() > 2 && s.ends_with("mm") {
            self.mm_to_px_f(v)
        } else {
            self.dp_to_px_f(v)
        }
    }

    /// TB: GetPxFromValue
    pub fn px_from_value(&self, v: &Value, default: i32) -> i32 {
        match v {
            Value::Int(i) => self.dp_to_px(*i),
            Value::Float(f) => round(self.dp_to_px_f(*f)) as i32,
            Value::Str(s) => self.px_from_str(s, default),
            _ => default,
        }
    }

    pub fn px_from_value_f(&self, v: &Value, default: f32) -> f32 {
        match v {
            Value::Int(i) => self.dp_to_px_f(*i as f32),
            Value::Float(f) => self.dp_to_px_f(*f),
            Value::Str(s) => self.px_from_str_f(s, default),
            _ => default,
        }
    }
}

/// TB: tb_value.cpp is_start_of_number
fn starts_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
    }
    b.get(i).is_some_and(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    // TB: tests/test_tb_dimension.cpp
    #[test]
    fn dimension_converter() {
        let dc = DimensionConverter::new(100, 200, 96);
        assert_eq!(dc.px_from_str("50", 0), 100);
        assert_eq!(dc.px_from_str("50px", 0), 50);
        assert_eq!(dc.px_from_str("50dp", 0), 100);
        assert_eq!(dc.px_from_str("abc", 7), 7);
        assert_eq!(dc.px_from_str("-3", 0), -6);
        assert_eq!(dc.px_from_value(&Value::Int(4), 0), 8);
        assert_eq!(dc.dst_dpi_filename("skin/foo.png"), "skin/foo@200.png");
        let same = DimensionConverter::new(96, 96, 96);
        assert_eq!(same.dst_dpi_filename("foo.png"), "foo.png");
        assert_eq!(same.px_from_str("25mm", 0), 94);
    }
}
