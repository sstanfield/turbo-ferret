//! Hand-rolled PNG decoder (feature `png`): [`PngDecoder`], the default [`ImageDecoder`].
//!
//! [`inflate`] is a zlib/DEFLATE decompressor and [`png`] turns PNG files into
//! straight-alpha RGBA8. Both need only `alloc`, use no `unsafe`, and never panic on
//! malformed input; [`Limits`] bounds the memory a crafted file can make them allocate.
//! See port-design.md section 18.3 for the supported subset.
//!
//! TB: tb_image_loader_stb.cpp (TB decodes through the vendored stb_image instead).

pub mod inflate;
pub mod png;

use crate::services::{DecodeError, Image, ImageDecoder};

/// Bounds on what one decode may allocate. Exceeding any of them gives
/// [`DecodeError::TooLarge`] before the pixel buffers are allocated.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum image width in pixels (at most `i32::MAX` is ever accepted).
    pub max_width: u32,
    /// Maximum image height in pixels (at most `i32::MAX` is ever accepted).
    pub max_height: u32,
    /// Maximum size in bytes of the decompressed scanlines, and of the RGBA8 output.
    pub max_bytes: usize,
}

impl Limits {
    /// 16384 x 16384 pixels and 256 MiB.
    pub const DEFAULT: Limits = Limits { max_width: 16384, max_height: 16384, max_bytes: 256 << 20 };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}

/// Decodes PNG files to RGBA8 within [`Limits`].
#[derive(Clone, Debug, Default)]
pub struct PngDecoder {
    pub limits: Limits,
}

impl PngDecoder {
    /// A decoder with the default limits.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limits(limits: Limits) -> Self {
        PngDecoder { limits }
    }
}

impl ImageDecoder for PngDecoder {
    fn decode(&mut self, bytes: &[u8]) -> Result<Image, DecodeError> {
        png::decode(bytes, &self.limits)
    }
}
