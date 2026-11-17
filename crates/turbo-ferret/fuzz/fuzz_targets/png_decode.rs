//! The PNG decoder must return `Ok` or `Err` on any input and never panic; a decoded
//! image is always consistent with its size.
#![no_main]

use libfuzzer_sys::fuzz_target;
use turbo_ferret::ImageDecoder;
use turbo_ferret::image::{Limits, PngDecoder};

fuzz_target!(|data: &[u8]| {
    // Limits small enough that legitimate-but-huge headers do not slow the fuzzer down.
    let limits = Limits { max_width: 4096, max_height: 4096, max_bytes: 32 << 20 };
    if let Ok(image) = PngDecoder::with_limits(limits).decode(data) {
        assert!(image.size.w > 0 && image.size.h > 0);
        assert!(image.size.w as u32 <= limits.max_width && image.size.h as u32 <= limits.max_height);
        assert_eq!(image.rgba.len(), image.size.w as usize * image.size.h as usize * 4);
    }
});
