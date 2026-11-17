//! Hand-rolled PNG decoder (feature `png`). Placeholder until the decoder lands.

use crate::services::{DecodeError, Image, ImageDecoder};

#[derive(Default)]
pub struct PngDecoder;

impl ImageDecoder for PngDecoder {
    fn decode(&mut self, _bytes: &[u8]) -> Result<Image, DecodeError> {
        Err(DecodeError::Unsupported("png decoder not implemented yet"))
    }
}
