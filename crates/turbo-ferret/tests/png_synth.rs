//! Synthesized PNGs: every supported color type and bit depth, every filter type and the
//! `png` crate's compressors, compared with the `png` crate's own decoding. Also the
//! unsupported and malformed cases, and the limits.

mod common;

use common::{Enc, Rng};
use png::{BitDepth, ColorType, Compression, FilterType};
use turbo_ferret::image::inflate::{self, InflateError};
use turbo_ferret::image::{Limits, PngDecoder};
use turbo_ferret::{DecodeError, ImageDecoder};

const FILTERS: [Option<FilterType>; 6] = [
    Some(FilterType::NoFilter),
    Some(FilterType::Sub),
    Some(FilterType::Up),
    Some(FilterType::Avg),
    Some(FilterType::Paeth),
    None, // adaptive: a different filter per row
];
const COMPRESSIONS: [Compression; 3] = [Compression::Fast, Compression::Default, Compression::Best];
const SIZES: [(u32, u32); 4] = [(1, 1), (7, 3), (33, 17), (130, 41)];

/// Scanlines that are part smooth gradient, part noise, so that every filter and both
/// literals and matches get exercised. Padding bits at row ends are random too.
fn pixels(e: &Enc<'_>, rng: &mut Rng) -> Vec<u8> {
    let rb = common::row_bytes(e.width, e.color, e.depth);
    let mut data = Vec::with_capacity(rb * e.height as usize);
    for y in 0..e.height as usize {
        for x in 0..rb {
            let v = if (x / 16 + y / 4) % 3 == 0 { rng.next() as u8 } else { (x * 3 + y * 5) as u8 };
            data.push(v);
        }
    }
    data
}

fn decode(bytes: &[u8]) -> Result<turbo_ferret::Image, DecodeError> {
    PngDecoder::default().decode(bytes)
}

/// Compare with the `png` crate; returns the number of fully transparent pixels.
fn check(e: &Enc<'_>, data: &[u8], what: &str) -> usize {
    let bytes = common::encode(e, data);
    let ours = decode(&bytes).unwrap_or_else(|err| panic!("{what}: {err}"));
    let (w, h, want) = common::reference_rgba(&bytes).unwrap();
    assert_eq!((ours.size.w, ours.size.h), (w as i32, h as i32), "{what}");
    assert!(ours.rgba == want, "{what}: pixels differ");
    ours.rgba.chunks_exact(4).filter(|p| p[3] == 0).count()
}

/// Run every filter, compression and size for one color format.
fn sweep(color: ColorType, depth: BitDepth, palette: Option<&[u8]>, trns: Option<&[u8]>) {
    let mut rng = Rng(0x1234_5678 ^ depth as u32 ^ (color as u32) << 8);
    let mut transparent = 0;
    for &(width, height) in &SIZES {
        for &filter in &FILTERS {
            for &compression in &COMPRESSIONS {
                let e = Enc { width, height, color, depth, filter, compression, palette, trns };
                let mut data = pixels(&e, &mut rng);
                if color == ColorType::Indexed && depth == BitDepth::Eight {
                    // Mostly valid indices, a few past the palette (opaque black).
                    let n = palette.map_or(1, |p| p.len() / 3) as u32;
                    for b in &mut data {
                        if rng.below(20) != 0 {
                            *b = (u32::from(*b) % n) as u8;
                        }
                    }
                }
                let what =
                    format!("{color:?} {depth:?} {width}x{height} {filter:?} {compression:?} trns={}", trns.is_some());
                transparent += check(&e, &data, &what);
            }
        }
    }
    if trns.is_some() {
        assert!(transparent > 0, "{color:?} {depth:?}: the tRNS case never produced a transparent pixel");
    }
}

fn palette(n: usize) -> Vec<u8> {
    (0..n).flat_map(|i| [(i * 37) as u8, (i * 91 + 7) as u8, (255 - i * 13 % 256) as u8]).collect()
}

#[test]
fn gray() {
    for depth in [BitDepth::One, BitDepth::Two, BitDepth::Four, BitDepth::Eight] {
        sweep(ColorType::Grayscale, depth, None, None);
        // A transparent gray value inside the depth's range.
        let key = (1u16 << depth as u16) - 2;
        sweep(ColorType::Grayscale, depth, None, Some(&key.to_be_bytes()));
    }
}

#[test]
fn rgb() {
    sweep(ColorType::Rgb, BitDepth::Eight, None, None);
    // Gradient pixels hit this key now and then.
    sweep(ColorType::Rgb, BitDepth::Eight, None, Some(&[0, 6, 0, 9, 0, 12]));
}

#[test]
fn indexed() {
    for depth in [BitDepth::One, BitDepth::Two, BitDepth::Four, BitDepth::Eight] {
        let n = 1usize << depth as usize;
        let pal = palette(n);
        sweep(ColorType::Indexed, depth, Some(&pal), None);
        // tRNS shorter than the palette: the rest stay opaque.
        let alpha: Vec<u8> = (0..n.div_ceil(2)).map(|i| (i * 51) as u8).collect();
        sweep(ColorType::Indexed, depth, Some(&pal), Some(&alpha));
    }
    // A palette smaller than the index range (indices past it read as opaque black).
    let pal = palette(5);
    sweep(ColorType::Indexed, BitDepth::Eight, Some(&pal), Some(&[0, 128]));
    sweep(ColorType::Indexed, BitDepth::Four, Some(&pal), None);
}

#[test]
fn gray_alpha_and_rgba() {
    sweep(ColorType::GrayscaleAlpha, BitDepth::Eight, None, None);
    sweep(ColorType::Rgba, BitDepth::Eight, None, None);
}

#[test]
fn large_image_multiple_blocks() {
    // Big enough for several DEFLATE blocks, split over many IDAT chunks.
    let mut rng = Rng(42);
    for compression in COMPRESSIONS {
        let mut e = Enc::new(700, 300, ColorType::Rgba, BitDepth::Eight);
        e.compression = compression;
        let data = pixels(&e, &mut rng);
        check(&e, &data, &format!("large {compression:?}"));
    }
}

#[test]
fn many_small_idat_chunks() {
    let mut rng = Rng(7);
    let e = Enc::new(40, 20, ColorType::Rgb, BitDepth::Eight);
    let bytes = common::encode(&e, &pixels(&e, &mut rng));
    let want = decode(&bytes).unwrap();
    // Re-split the zlib stream into 1..=13-byte IDATs, with an ancillary chunk between.
    let stream = common::idat_stream(&bytes);
    let mut chunks: Vec<([u8; 4], Vec<u8>)> =
        common::chunks(&bytes).into_iter().filter(|(k, _)| k != b"IDAT" && k != b"IEND").collect();
    chunks.push((*b"tEXt", b"Comment\0between".to_vec()));
    let mut rest = &stream[..];
    while !rest.is_empty() {
        let n = (rng.below(13) + 1).min(rest.len());
        chunks.push((*b"IDAT", rest[..n].to_vec()));
        rest = &rest[n..];
    }
    chunks.push((*b"IEND", Vec::new()));
    let resplit = common::assemble(&chunks);
    assert_eq!(decode(&resplit).unwrap(), want);
    // The `png` crate agrees on the re-assembled file.
    assert_eq!(common::reference_rgba(&resplit).unwrap().2, want.rgba);
    // A missing IEND is tolerated.
    chunks.pop();
    assert_eq!(decode(&common::assemble(&chunks)).unwrap(), want);
}

/// The inflater against the `png` crate's DEFLATE encoders (fdeflate for `Fast`,
/// miniz_oxide otherwise): an 8-bit gray image with filter None decompresses to its
/// rows, each prefixed with a 0 filter byte.
#[test]
fn inflate_matches_png_crate_compressors() {
    let mut rng = Rng(99);
    for (width, height) in [(1, 1), (100, 1), (256, 256), (1000, 300)] {
        for compression in COMPRESSIONS {
            let mut e = Enc::new(width, height, ColorType::Grayscale, BitDepth::Eight);
            e.filter = Some(FilterType::NoFilter);
            e.compression = compression;
            let data = pixels(&e, &mut rng);
            let stream = common::idat_stream(&common::encode(&e, &data));
            let want: Vec<u8> =
                data.chunks(width as usize).flat_map(|row| std::iter::once(0).chain(row.iter().copied())).collect();
            assert_eq!(
                inflate::zlib_decompress(&stream, usize::MAX).unwrap(),
                want,
                "{width}x{height} {compression:?}"
            );
            assert_eq!(inflate::zlib_decompress(&stream, want.len() - 1), Err(InflateError::OutputLimit));
        }
    }
}

#[test]
fn sixteen_bit_is_unsupported() {
    for (color, samples) in
        [(ColorType::Grayscale, 1), (ColorType::Rgb, 3), (ColorType::GrayscaleAlpha, 2), (ColorType::Rgba, 4)]
    {
        let e = Enc::new(3, 2, color, BitDepth::Sixteen);
        let bytes = common::encode(&e, &vec![0x55; 3 * 2 * 2 * samples]);
        let mut reference = png::Decoder::new(&bytes[..]).read_info().unwrap();
        let mut buf = vec![0; reference.output_buffer_size()];
        reference.next_frame(&mut buf).unwrap();
        assert_eq!(decode(&bytes), Err(DecodeError::Unsupported("16-bit PNG channels")), "{color:?}");
    }
}

#[test]
fn interlaced_is_unsupported() {
    // The `png` crate cannot write Adam7, so build one by hand: a 1x1 gray image's
    // single pass is the same as its non-interlaced scanline.
    let e = Enc::new(1, 1, ColorType::Grayscale, BitDepth::Eight);
    let plain = common::encode(&e, &[0x80]);
    let mut chunks = common::chunks(&plain);
    assert_eq!(&chunks[0].0, b"IHDR");
    chunks[0].1[12] = 1;
    let bytes = common::assemble(&chunks);
    assert_eq!(common::reference_rgba(&bytes).unwrap().2, [0x80, 0x80, 0x80, 255]);
    assert_eq!(decode(&bytes), Err(DecodeError::Unsupported("Adam7 interlaced PNG")));
}

#[test]
fn not_png() {
    for bytes in [&b""[..], b"\x89PNG", b"GIF89a\x01\x00\x01\x00", b"\xFF\xD8\xFF\xE0\x00\x10JFIF", b"hello world"] {
        assert_eq!(decode(bytes), Err(DecodeError::UnknownFormat), "{bytes:?}");
    }
}

/// A valid 4x3 RGBA file, then edits to its chunks.
fn edited(edit: impl FnOnce(&mut Vec<([u8; 4], Vec<u8>)>)) -> Result<turbo_ferret::Image, DecodeError> {
    let e = Enc::new(4, 3, ColorType::Rgba, BitDepth::Eight);
    let mut chunks = common::chunks(&common::encode(&e, &[7; 48]));
    edit(&mut chunks);
    decode(&common::assemble(&chunks))
}

#[test]
fn malformed_structure() {
    assert!(edited(|_| {}).is_ok());
    assert_eq!(edited(|c| c.retain(|(k, _)| k != b"IDAT")), Err(DecodeError::Corrupt("missing IDAT")));
    assert_eq!(edited(|c| c.remove(0).1.clear()), Err(DecodeError::Corrupt("missing IHDR")));
    assert_eq!(edited(|c| c.insert(1, c[0].clone())), Err(DecodeError::Corrupt("duplicate IHDR")));
    assert_eq!(edited(|c| c[0].1.push(0)), Err(DecodeError::Corrupt("bad IHDR length")));
    assert_eq!(edited(|c| c[0].1[0..4].copy_from_slice(&[0; 4])), Err(DecodeError::Corrupt("bad image dimensions")));
    assert_eq!(edited(|c| c[0].1[9] = 5), Err(DecodeError::Corrupt("bad color type")));
    assert_eq!(edited(|c| c[0].1[8] = 4), Err(DecodeError::Corrupt("bad bit depth for color type")));
    assert_eq!(edited(|c| c[0].1[10] = 1), Err(DecodeError::Corrupt("bad compression method")));
    assert_eq!(edited(|c| c[0].1[11] = 1), Err(DecodeError::Corrupt("bad filter method")));
    assert_eq!(edited(|c| c[0].1[12] = 2), Err(DecodeError::Corrupt("bad interlace method")));
    assert_eq!(
        edited(|c| c.insert(1, (*b"CRIT", vec![1, 2, 3]))),
        Err(DecodeError::Unsupported("unknown critical chunk"))
    );
    assert!(edited(|c| c.insert(1, (*b"crIt", vec![1, 2, 3]))).is_ok());
    // Height 4 instead of 3: the stream ends early.
    assert_eq!(edited(|c| c[0].1[7] = 4), Err(DecodeError::Corrupt("not enough image data")));
    // Height 2: the stream has a row too many.
    assert_eq!(edited(|c| c[0].1[7] = 2), Err(DecodeError::Corrupt("too much image data")));
    // Width 3: rows no longer line up, and the data runs past the expected length.
    assert!(matches!(edited(|c| c[0].1[3] = 3), Err(DecodeError::Corrupt(_))));
    // Palette image without PLTE.
    assert_eq!(
        edited(|c| {
            c[0].1[8] = 8;
            c[0].1[9] = 3;
        }),
        Err(DecodeError::Corrupt("missing PLTE"))
    );
}

#[test]
fn bad_filter_and_checksum() {
    let e = Enc::new(2, 2, ColorType::Grayscale, BitDepth::Eight);
    let bytes = common::encode(&e, &[1, 2, 3, 4]);
    let mut chunks = common::chunks(&bytes);
    // Replace the image data with a stored zlib stream we control.
    let rows = [0u8, 1, 2, 5, 3, 4]; // filter 5 on the second row
    let mut z = vec![0x78, 0x01, 0x01, rows.len() as u8, 0, !(rows.len() as u8), 0xFF];
    z.extend_from_slice(&rows);
    z.extend_from_slice(&inflate::adler32(&rows).to_be_bytes());
    let idat = chunks.iter_mut().find(|(k, _)| k == b"IDAT").unwrap();
    idat.1 = z.clone();
    assert_eq!(decode(&common::assemble(&chunks)), Err(DecodeError::Corrupt("bad scanline filter")));
    let n = z.len();
    let idat = chunks.iter_mut().find(|(k, _)| k == b"IDAT").unwrap();
    idat.1[n - 1] ^= 0xFF;
    assert_eq!(decode(&common::assemble(&chunks)), Err(DecodeError::Corrupt("zlib checksum mismatch")));
}

#[test]
fn limits() {
    let e = Enc::new(9, 5, ColorType::Rgba, BitDepth::Eight);
    let bytes = common::encode(&e, &[1; 9 * 5 * 4]);
    let with = |limits: Limits| PngDecoder::with_limits(limits).decode(&bytes);
    assert!(with(Limits::default()).is_ok());
    assert_eq!(with(Limits { max_width: 8, ..Limits::default() }), Err(DecodeError::TooLarge));
    assert_eq!(with(Limits { max_height: 4, ..Limits::default() }), Err(DecodeError::TooLarge));
    assert!(with(Limits { max_width: 9, max_height: 5, max_bytes: 9 * 5 * 4 + 5 }).is_ok());
    assert_eq!(with(Limits { max_bytes: 9 * 5 * 4 + 4, ..Limits::default() }), Err(DecodeError::TooLarge));
    assert_eq!(Limits::default(), Limits { max_width: 16384, max_height: 16384, max_bytes: 256 << 20 });

    // Huge declared dimensions fail before anything is allocated.
    let mut chunks = common::chunks(&bytes);
    chunks[0].1[0..8].copy_from_slice(&[0x7F, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF]);
    let huge = common::assemble(&chunks);
    assert_eq!(decode(&huge), Err(DecodeError::TooLarge));
    let unlimited = Limits { max_width: u32::MAX, max_height: u32::MAX, max_bytes: usize::MAX };
    // Without limits, the output is still only pre-allocated up to what the compressed
    // data could expand to, and the short stream fails.
    assert_eq!(PngDecoder::with_limits(unlimited).decode(&huge), Err(DecodeError::Corrupt("not enough image data")));
}

#[test]
fn decompression_bomb_stops_at_expected_size() {
    // 4 MB of zeros compressed into a few KB, behind an IHDR that claims 1x1.
    let e = Enc::new(2000, 2000, ColorType::Grayscale, BitDepth::Eight);
    let bytes = common::encode(&e, &vec![0; 2000 * 2000]);
    let mut chunks = common::chunks(&bytes);
    chunks[0].1[0..8].copy_from_slice(&[0, 0, 0, 1, 0, 0, 0, 1]);
    assert_eq!(decode(&common::assemble(&chunks)), Err(DecodeError::Corrupt("too much image data")));
}
