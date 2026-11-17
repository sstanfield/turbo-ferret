//! Shared helpers for the PNG decoder tests: the `png` crate as a reference decoder and
//! encoder, a chunk reader/writer, and a deterministic PRNG.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Decode with the `png` crate (palette, low-bit gray and tRNS expanded) and convert the
/// result to RGBA8: `(width, height, rgba)`.
pub fn reference_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), png::DecodingError> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    assert_eq!(info.bit_depth, png::BitDepth::Eight, "reference output is not 8-bit");
    let rgba = match info.color_type {
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Rgba => buf,
        png::ColorType::Indexed => panic!("palette not expanded"),
    };
    Ok((info.width, info.height, rgba))
}

/// Options for [`encode`].
pub struct Enc<'a> {
    pub width: u32,
    pub height: u32,
    pub color: png::ColorType,
    pub depth: png::BitDepth,
    /// `None` means the encoder's adaptive filter choice.
    pub filter: Option<png::FilterType>,
    pub compression: png::Compression,
    pub palette: Option<&'a [u8]>,
    pub trns: Option<&'a [u8]>,
}

impl Enc<'_> {
    pub fn new(width: u32, height: u32, color: png::ColorType, depth: png::BitDepth) -> Self {
        Enc {
            width,
            height,
            color,
            depth,
            filter: None,
            compression: png::Compression::Default,
            palette: None,
            trns: None,
        }
    }
}

/// Encode packed scanlines with the `png` crate.
pub fn encode(e: &Enc<'_>, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, e.width, e.height);
        enc.set_color(e.color);
        enc.set_depth(e.depth);
        enc.set_compression(e.compression);
        match e.filter {
            Some(f) => {
                enc.set_filter(f);
                enc.set_adaptive_filter(png::AdaptiveFilterType::NonAdaptive);
            }
            None => enc.set_adaptive_filter(png::AdaptiveFilterType::Adaptive),
        }
        if let Some(p) = e.palette {
            enc.set_palette(p.to_vec());
        }
        if let Some(t) = e.trns {
            enc.set_trns(t.to_vec());
        }
        let mut writer = enc.write_header().expect("png header");
        writer.write_image_data(data).expect("png data");
    }
    out
}

/// Bytes per packed scanline.
pub fn row_bytes(width: u32, color: png::ColorType, depth: png::BitDepth) -> usize {
    (width as usize * color.samples() * depth as usize).div_ceil(8)
}

/// The chunks of a PNG file as `(type, body)`, CRCs dropped.
pub fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 8;
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = png[pos + 4..pos + 8].try_into().unwrap();
        out.push((kind, png[pos + 8..pos + 8 + len].to_vec()));
        pos += 12 + len;
    }
    out
}

/// Assemble a PNG file from chunks, with correct CRCs.
pub fn assemble(chunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    for (kind, body) in chunks {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut crc_input = kind.to_vec();
        crc_input.extend_from_slice(body);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }
    out
}

/// The concatenated IDAT data (the zlib stream) of a PNG file.
pub fn idat_stream(png: &[u8]) -> Vec<u8> {
    chunks(png).into_iter().filter(|(k, _)| k == b"IDAT").flat_map(|(_, b)| b).collect()
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// xorshift32: a small deterministic PRNG.
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// The repository's `resources/` directory.
pub fn resources_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources")
}

/// Every `*.png` under `dir`, recursively, sorted.
pub fn png_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
