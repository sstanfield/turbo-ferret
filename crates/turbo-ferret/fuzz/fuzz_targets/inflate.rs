//! The zlib and raw DEFLATE decoders must return `Ok` or `Err` on any input, never panic,
//! and never produce more than `max_out` bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use turbo_ferret::image::inflate;

const MAX_OUT: usize = 1 << 22;

fuzz_target!(|data: &[u8]| {
    if let Ok(out) = inflate::zlib_decompress(data, MAX_OUT) {
        assert!(out.len() <= MAX_OUT);
        // A pre-sized output and an exact limit give the same result.
        assert_eq!(inflate::zlib_decompress_with_capacity(data, out.len(), out.len()), Ok(out));
    }
    if let Ok(out) = inflate::deflate_decompress(data, MAX_OUT) {
        assert!(out.len() <= MAX_OUT);
        // A tighter limit fails cleanly.
        if let Some(limit) = out.len().checked_sub(1) {
            assert_eq!(inflate::deflate_decompress(data, limit), Err(inflate::InflateError::OutputLimit));
        }
    }
});
