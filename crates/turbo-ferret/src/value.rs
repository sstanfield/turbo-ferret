//! `Value`: the dynamic value used by resources, widget values, messages and events.
//!
//! TB: tb_value.h, tb_value.cpp. Getters never mutate (H-13); strings and arrays are
//! `Rc`-shared so clones are a refcount bump.

use alloc::borrow::Cow;
use alloc::format;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt;

#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Null,
    Int(i32),
    Float(f32),
    Str(Rc<str>),
    Array(Rc<[Value]>),
    Object(Rc<dyn Any>),
}

/// The kind of a value, without its payload.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum ValueKind {
    #[default]
    Null,
    Int,
    Float,
    Str,
    Array,
    Object,
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "Null"),
            Value::Int(v) => write!(f, "Int({v})"),
            Value::Float(v) => write!(f, "Float({v})"),
            Value::Str(s) => write!(f, "Str({s:?})"),
            Value::Array(a) => f.debug_list().entries(a.iter()).finish(),
            Value::Object(_) => write!(f, "Object(..)"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, o: &Value) -> bool {
        match (self, o) {
            (Value::Null, Value::Null) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Null => ValueKind::Null,
            Value::Int(_) => ValueKind::Int,
            Value::Float(_) => ValueKind::Float,
            Value::Str(_) => ValueKind::Str,
            Value::Array(_) => ValueKind::Array,
            Value::Object(_) => ValueKind::Object,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// TB: TBValue::GetInt (strings parse like `atoi`, floats truncate).
    pub fn as_int(&self) -> i32 {
        match self {
            Value::Int(v) => *v,
            Value::Float(v) => *v as i32,
            Value::Str(s) => atoi(s),
            _ => 0,
        }
    }

    /// TB: TBValue::GetFloat (strings parse like `atof`).
    pub fn as_float(&self) -> f32 {
        match self {
            Value::Int(v) => *v as f32,
            Value::Float(v) => *v,
            Value::Str(s) => atof(s),
            _ => 0.0,
        }
    }

    /// String form. Borrowed for strings, formatted for numbers, empty otherwise.
    /// TB: TBValue::GetString (which mutated the value; this does not).
    pub fn as_str(&self) -> Cow<'_, str> {
        match self {
            Value::Str(s) => Cow::Borrowed(s),
            Value::Int(v) => Cow::Owned(format!("{v}")),
            Value::Float(v) => Cow::Owned(format!("{v}")),
            _ => Cow::Borrowed(""),
        }
    }

    /// The string, only if this is a `Str`.
    pub fn get_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object<T: Any>(&self) -> Option<&T> {
        match self {
            Value::Object(o) => o.downcast_ref::<T>(),
            _ => None,
        }
    }

    /// Convert to the given kind (used by value binding). TB: TBWidget::SetValue/SetText paths.
    pub fn to_kind(&self, kind: ValueKind) -> Value {
        match kind {
            ValueKind::Int => Value::Int(self.as_int()),
            ValueKind::Float => Value::Float(self.as_float()),
            ValueKind::Str => match self {
                Value::Str(_) => self.clone(),
                _ => Value::str(&self.as_str()),
            },
            _ => self.clone(),
        }
    }

    /// Interpret a string the way TB does: an int, a float, a list of numbers
    /// (`"6 8"`, `"0, 0, 100, 50"`), otherwise a string. TB: TBValue::SetFromStringAuto
    pub fn parse_auto(s: &str) -> Value {
        if is_number_only(s) {
            if s.contains('.') { Value::Float(atof(s)) } else { Value::Int(atoi(s)) }
        } else if is_start_of_number(s) && contains_non_trailing_space(s) {
            let items: Vec<Value> = s.split([' ', ',']).filter(|t| !t.is_empty()).map(Value::parse_auto).collect();
            Value::Array(Rc::from(items))
        } else {
            Value::str(s)
        }
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::str(v)
    }
}
impl From<Rc<str>> for Value {
    fn from(v: Rc<str>) -> Self {
        Value::Str(v)
    }
}

// ---- C-like number scanning (TB used atoi/atof/strtod) ----

/// Length of the longest prefix of `s` that `strtod` would accept (decimal only), after
/// leading whitespace. Returns (start, end) byte offsets of the number, or None.
fn scan_number(s: &str) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t' || b[i] == b'\n' || b[i] == b'\r') {
        i += 1;
    }
    let start = i;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    // exponent
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    Some((start, i))
}

/// `atoi`: leading whitespace, optional sign, digits; 0 if none.
pub fn atoi(s: &str) -> i32 {
    let b = s.trim_start().as_bytes();
    let (neg, mut i) = match b.first() {
        Some(b'-') => (true, 1),
        Some(b'+') => (false, 1),
        _ => (false, 0),
    };
    let mut v: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = (v * 10 + (b[i] - b'0') as i64).min(i32::MAX as i64 + 1);
        i += 1;
    }
    let v = if neg { -v } else { v };
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// `atof`: the longest numeric prefix; 0.0 if none.
pub fn atof(s: &str) -> f32 {
    match scan_number(s) {
        Some((a, b)) => s[a..b].parse::<f32>().unwrap_or(0.0),
        None => 0.0,
    }
}

/// TB: tb_value.cpp is_number_only
fn is_number_only(s: &str) -> bool {
    if s.is_empty() || s.starts_with(' ') {
        return false;
    }
    match scan_number(s) {
        Some((_, end)) => s[end..].bytes().all(|c| c == b' '),
        None => false,
    }
}

/// TB: tb_value.cpp is_start_of_number
fn is_start_of_number(s: &str) -> bool {
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

/// TB: tb_value.cpp contains_non_trailing_space
fn contains_non_trailing_space(s: &str) -> bool {
    match s.find(' ') {
        Some(p) => s[p..].bytes().any(|c| c != b' '),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_auto_kinds() {
        assert_eq!(Value::parse_auto("42"), Value::Int(42));
        assert_eq!(Value::parse_auto("-7"), Value::Int(-7));
        assert_eq!(Value::parse_auto("1.5"), Value::Float(1.5));
        assert_eq!(Value::parse_auto("42  "), Value::Int(42));
        assert_eq!(Value::parse_auto("hello"), Value::str("hello"));
        assert_eq!(Value::parse_auto("10px"), Value::str("10px"));
        assert_eq!(Value::parse_auto(" 42"), Value::str(" 42"));
        assert_eq!(Value::parse_auto("6 8"), Value::Array(Rc::from([Value::Int(6), Value::Int(8)])));
        let a = Value::parse_auto("0, 0, 100, 50");
        assert_eq!(a.as_array().unwrap().len(), 4);
        assert_eq!(a.as_array().unwrap()[2], Value::Int(100));
        let m = Value::parse_auto("10 -4 3.5");
        assert_eq!(m.as_array().unwrap()[2], Value::Float(3.5));
    }

    #[test]
    fn getters_convert_without_mutating() {
        let v = Value::Int(5);
        assert_eq!(v.as_str(), "5");
        assert_eq!(v, Value::Int(5));
        assert_eq!(Value::str("12abc").as_int(), 12);
        assert_eq!(Value::str("2.5x").as_float(), 2.5);
        assert_eq!(Value::Float(3.9).as_int(), 3);
        assert_eq!(Value::Null.as_int(), 0);
        assert_eq!(Value::Null.as_str(), "");
    }

    #[test]
    fn clone_is_shared() {
        let v = Value::str("shared");
        let w = v.clone();
        if let (Value::Str(a), Value::Str(b)) = (&v, &w) {
            assert!(Rc::ptr_eq(a, b));
        } else {
            panic!();
        }
    }
}
