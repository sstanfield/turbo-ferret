//! Every PNG under `resources/` decodes to exactly the pixels the `png` crate produces.

mod common;

use std::time::{Duration, Instant};

use turbo_ferret::ImageDecoder;
use turbo_ferret::image::PngDecoder;

#[test]
fn resources_match_png_crate() {
    let files = common::png_files(&common::resources_dir());
    assert!(files.len() >= 100, "expected the ~146 resource PNGs, found {}", files.len());

    let mut decoder = PngDecoder::default();
    let (mut ours, mut reference) = (Duration::ZERO, Duration::ZERO);
    let (mut pixels, mut compressed) = (0usize, 0usize);
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let start = Instant::now();
        let image = decoder.decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        ours += start.elapsed();

        let start = Instant::now();
        let (w, h, want) = common::reference_rgba(&bytes).unwrap();
        reference += start.elapsed();
        assert_eq!((image.size.w, image.size.h), (w as i32, h as i32), "{}", path.display());
        assert_eq!(image.rgba.len(), want.len(), "{}", path.display());
        if image.rgba != want {
            let i = image.rgba.iter().zip(&want).position(|(a, b)| a != b).unwrap();
            let px = i / 4;
            panic!(
                "{}: first difference at pixel ({}, {}): ours {:?}, png crate {:?}",
                path.display(),
                px % w as usize,
                px / w as usize,
                &image.rgba[px * 4..px * 4 + 4],
                &want[px * 4..px * 4 + 4]
            );
        }
        pixels += image.rgba.len() / 4;
        compressed += bytes.len();
    }
    eprintln!(
        "decoded {} PNGs ({} bytes, {} pixels) in {:.2} ms (png crate: {:.2} ms)",
        files.len(),
        compressed,
        pixels,
        ours.as_secs_f64() * 1000.0,
        reference.as_secs_f64() * 1000.0
    );
}
