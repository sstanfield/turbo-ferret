//! Corrupt input: truncated and bit-flipped real PNGs, and random zlib streams, must give
//! `Ok` or `Err` and never panic (run in debug, so arithmetic overflow would panic too).

mod common;

use common::Rng;
use turbo_ferret::image::inflate;
use turbo_ferret::image::{Limits, PngDecoder};
use turbo_ferret::{DecodeError, Image, ImageDecoder};

/// Small limits keep mutated headers from asking for big (if legitimate) buffers.
fn decode(bytes: &[u8]) -> Result<Image, DecodeError> {
    let result =
        PngDecoder::with_limits(Limits { max_width: 4096, max_height: 4096, max_bytes: 16 << 20 }).decode(bytes);
    if let Ok(image) = &result {
        assert!(image.size.w > 0 && image.size.h > 0);
        assert_eq!(image.rgba.len(), image.size.w as usize * image.size.h as usize * 4);
    }
    result
}

/// For each color type present, the resource PNG closest to 8 KB.
fn samples() -> Vec<(String, Vec<u8>)> {
    let mut by_color: Vec<(u8, String, Vec<u8>)> = Vec::new();
    let distance = |n: usize| n.abs_diff(8000);
    for path in common::png_files(&common::resources_dir()) {
        let bytes = std::fs::read(&path).unwrap();
        let color = bytes[25];
        match by_color.iter_mut().find(|(c, ..)| *c == color) {
            Some(entry) if distance(entry.2.len()) <= distance(bytes.len()) => {}
            Some(entry) => *entry = (color, path.display().to_string(), bytes),
            None => by_color.push((color, path.display().to_string(), bytes)),
        }
    }
    assert!(by_color.len() >= 4, "expected several color types in resources/");
    by_color.into_iter().map(|(_, name, bytes)| (name, bytes)).collect()
}

#[test]
fn truncated_files() {
    for (name, bytes) in samples() {
        assert!(decode(&bytes).is_ok(), "{name}");
        // Where the last IDAT body ends: CRCs are not checked and a missing IEND is
        // tolerated, so only cuts before this point lose image data.
        let mut data_end = 8;
        let mut pos = 8;
        while pos + 8 <= bytes.len() {
            let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
            if &bytes[pos + 4..pos + 8] == b"IDAT" {
                data_end = pos + 8 + len;
            }
            pos += 12 + len;
        }
        let step = (bytes.len() / 400).max(1);
        for n in (0..bytes.len()).filter(|&n| n < 64 || n % step == 0 || n + 32 > bytes.len()) {
            let result = decode(&bytes[..n]);
            if n < data_end {
                assert!(result.is_err(), "{name}: truncated at {n} of {} decoded", bytes.len());
            }
        }
    }
}

#[test]
fn flipped_bytes() {
    for (seed, (name, bytes)) in samples().into_iter().enumerate() {
        let mut rng = Rng(0x9E37_79B9 ^ seed as u32);
        let (mut ok, mut err) = (0, 0);
        for _ in 0..800 {
            let mut m = bytes.clone();
            for _ in 0..1 + rng.below(4) {
                let pos = if rng.below(4) == 0 { 8 + rng.below(25) } else { rng.below(m.len()) };
                m[pos] ^= (rng.next() % 255 + 1) as u8;
            }
            match decode(&m) {
                Ok(_) => ok += 1,
                Err(_) => err += 1,
            }
        }
        // Most flips land in the zlib stream and are caught by it (or its Adler-32).
        assert!(err > ok, "{name}: {ok} ok, {err} err");
    }
}

#[test]
fn hostile_headers() {
    // Valid file structure with random IHDR fields.
    let (_, bytes) = samples().swap_remove(0);
    let mut chunks = common::chunks(&bytes);
    let mut rng = Rng(5);
    for _ in 0..500 {
        let ihdr = &mut chunks[0].1;
        let w = [1, 2, 3, 0x7FFF_FFFF, 0xFFFF_FFFF, rng.next() % 5000, rng.next()][rng.below(7)];
        let h = [1, 7, 0x7FFF_FFFF, 0x8000_0000, rng.next() % 5000, rng.next()][rng.below(6)];
        ihdr[0..4].copy_from_slice(&u32::to_be_bytes(w));
        ihdr[4..8].copy_from_slice(&u32::to_be_bytes(h));
        ihdr[8] = [1, 2, 4, 8, 16, 0, 3][rng.below(7)];
        ihdr[9] = [0, 2, 3, 4, 6, 1, 7][rng.below(7)];
        let file = common::assemble(&chunks);
        let _ = decode(&file);
        let unlimited = Limits { max_width: u32::MAX, max_height: u32::MAX, max_bytes: 64 << 20 };
        let _ = PngDecoder::with_limits(unlimited).decode(&file);
    }
}

#[test]
fn random_zlib_streams() {
    let mut rng = Rng(77);
    for i in 0..3000 {
        let len = rng.below(600);
        let mut data = rng.bytes(len);
        if i % 2 == 0 && data.len() >= 2 {
            data[0] = 0x78;
            data[1] = 0x9C;
        }
        let _ = inflate::zlib_decompress(&data, 1 << 20);
        let _ = inflate::deflate_decompress(&data, 1 << 20);
        let _ = inflate::deflate_decompress(&data, 3);
    }
}

#[test]
fn mutated_zlib_streams() {
    for (name, bytes) in samples() {
        let stream = common::idat_stream(&bytes);
        let full = inflate::zlib_decompress(&stream, usize::MAX).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut rng = Rng(stream.len() as u32 | 1);
        let mut changed = 0;
        for _ in 0..300 {
            let mut m = stream.clone();
            let pos = rng.below(m.len());
            m[pos] ^= 1 << rng.below(8);
            if let Ok(out) = inflate::zlib_decompress(&m, full.len() + 100) {
                changed += usize::from(out != full);
            }
        }
        // Single-bit flips almost always break the stream or its Adler-32. (Adler-32 has
        // collisions: a flip in `radio@384.png` that changes 4 bytes passes it, and zlib
        // accepts that one too.)
        assert!(changed <= 3, "{name}: {changed} of 300 flips changed the output undetected");
    }
}
