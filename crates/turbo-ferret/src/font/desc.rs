//! TB: tb_font_desc.h (TBFontDescription; the unused bold/italic bits are dropped).

use crate::id::TbId;

/// A font face key: font id plus pixel size.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FontDesc {
    pub id: TbId,
    pub size: u16,
}

impl FontDesc {
    pub const fn new(id: TbId, size: u16) -> FontDesc {
        FontDesc { id, size }
    }
}
