//! `TbId`: a 32-bit name hash (FNV-1a), the "name" of widgets, skin elements,
//! fonts, messages and language strings.
//!
//! TB: tb_id.h, tb_hash.h

use core::fmt;

const FNV_BASIS: u32 = 2_166_136_261;
const FNV_PRIME: u32 = 16_777_619;

/// FNV-1a 32 over bytes, with TB's basis and prime. The empty string hashes to 0
/// (TB: tb_hash.h TBGetHash), so `TbId::new("") == TbId::NONE`.
///
/// TB hashed signed `char`s, so non-ASCII names hash differently there. The port
/// hashes unsigned bytes (compatibility is not required, decision 8).
pub const fn fnv1a(bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return 0;
    }
    let mut h = FNV_BASIS;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// A name hash. `TbId(0)` means "none". (H-20: always 4 bytes, in every build.)
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub struct TbId(pub u32);

impl TbId {
    pub const NONE: TbId = TbId(0);

    /// Hash a name. Usable in `const` context (H-21); see also [`tbid!`](crate::tbid).
    pub const fn new(s: &str) -> TbId {
        TbId(fnv1a(s.as_bytes()))
    }

    pub const fn from_u32(v: u32) -> TbId {
        TbId(v)
    }

    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    pub const fn is_some(self) -> bool {
        self.0 != 0
    }
}

impl From<&str> for TbId {
    fn from(s: &str) -> Self {
        TbId::new(s)
    }
}

impl From<u32> for TbId {
    fn from(v: u32) -> Self {
        TbId(v)
    }
}

impl fmt::Debug for TbId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TbId({:#010x})", self.0)
    }
}

/// `tbid!("name")` hashes at compile time.
#[macro_export]
macro_rules! tbid {
    ($s:literal) => {{
        const ID: $crate::TbId = $crate::TbId::new($s);
        ID
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_known_values() {
        // Reference values for FNV-1a 32.
        assert_eq!(fnv1a(b""), 0); // TB: empty string is 0, not the basis
        assert_eq!(fnv1a(b"a"), 0xe40c292c);
        assert_eq!(fnv1a(b"foobar"), 0xbf9cf968);
        assert_eq!(TbId::new("a"), TbId(0xe40c292c));
    }

    #[test]
    fn const_macro() {
        const OK: TbId = tbid!("ok");
        assert_eq!(OK, TbId::new("ok"));
        assert_eq!(tbid!(""), TbId::NONE);
        assert!(TbId::NONE.is_none());
        assert_ne!(tbid!("ok"), tbid!("cancel"));
    }
}
