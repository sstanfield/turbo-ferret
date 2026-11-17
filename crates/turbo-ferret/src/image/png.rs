//! PNG decoding: signature, chunk walk, `IHDR`/`PLTE`/`tRNS`/`IDAT`, unfiltering and
//! conversion to straight-alpha RGBA8.
//!
//! Supported: non-interlaced images of color type 0 (gray), 2 (RGB), 3 (palette),
//! 4 (gray + alpha) and 6 (RGBA) at bit depth 8, plus gray and palette at 1, 2 and 4
//! bits. `tRNS` gives palette alpha or a single transparent gray/RGB color.
//! Not supported (`DecodeError::Unsupported`): Adam7 interlacing and 16-bit channels.
//! Ancillary chunks are skipped and chunk CRCs are not checked (the zlib Adler-32
//! covers the image data).
//!
//! Not a port: TB decodes images through the vendored stb_image (tb_image_loader_stb.cpp).

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use super::Limits;
use super::inflate::{self, InflateError};
use crate::geom::Size;
use crate::services::{DecodeError, Image};

/// The eight bytes every PNG file starts with.
pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// DEFLATE expands at most about 1032:1, which bounds the output worth pre-allocating.
const MAX_DEFLATE_RATIO: usize = 1032;

/// Decode a PNG file to RGBA8, within `limits`.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Image, DecodeError> {
    let rest = bytes.strip_prefix(&SIGNATURE).ok_or(DecodeError::UnknownFormat)?;
    let mut chunks = Chunks { rest };

    let header = match chunks.next() {
        Some(Ok((b"IHDR", body))) => Header::parse(body)?,
        Some(Err(e)) => return Err(e),
        _ => return Err(DecodeError::Corrupt("missing IHDR")),
    };
    let layout = header.layout(limits)?;

    let mut palette: Option<&[u8]> = None;
    let mut trns: Option<&[u8]> = None;
    let mut idat: Vec<&[u8]> = Vec::new();
    for chunk in chunks {
        let (kind, body) = chunk?;
        match kind {
            b"IHDR" => return Err(DecodeError::Corrupt("duplicate IHDR")),
            b"PLTE" => palette = Some(body),
            b"tRNS" => trns = Some(body),
            b"IDAT" => idat.push(body),
            b"IEND" => break,
            // Bit 5 of the first byte clear: a critical chunk we must understand.
            _ if kind[0] & 0x20 == 0 => return Err(DecodeError::Unsupported("unknown critical chunk")),
            _ => {} // ancillary: skipped
        }
    }

    let format = Format::new(&header, palette, trns)?;
    if idat.is_empty() {
        return Err(DecodeError::Corrupt("missing IDAT"));
    }
    let joined;
    let zdata: &[u8] = if let [single] = idat[..] {
        single
    } else {
        joined = idat.concat();
        &joined
    };

    let expected = layout.raw_len;
    let capacity = expected.min(zdata.len().saturating_mul(MAX_DEFLATE_RATIO).saturating_add(64));
    let mut raw = inflate::zlib_decompress_with_capacity(zdata, capacity, expected).map_err(|e| match e {
        InflateError::OutputLimit => DecodeError::Corrupt("too much image data"),
        e => DecodeError::Corrupt(e.as_str()),
    })?;
    if raw.len() != expected {
        return Err(DecodeError::Corrupt("not enough image data"));
    }

    let mut rgba = vec![0u8; layout.rgba_len];
    let zero_row = vec![0u8; layout.row_bytes];
    let mut prev: &[u8] = &zero_row;
    for (row, out) in raw.chunks_exact_mut(layout.row_bytes + 1).zip(rgba.chunks_exact_mut(layout.width * 4)) {
        let Some((&mut filter, cur)) = row.split_first_mut() else { break };
        unfilter(filter, layout.bpp, prev, cur)?;
        format.expand(cur, out);
        prev = cur;
    }

    Ok(Image { size: Size::new(layout.width as i32, layout.height as i32), rgba })
}

// ---------------------------------------------------------------------------
// Chunks and header
// ---------------------------------------------------------------------------

/// Walks the chunks after the signature: `(type, body)`. CRCs are skipped (a missing
/// CRC at the very end is tolerated); a body that runs past the end is an error.
struct Chunks<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Result<(&'a [u8; 4], &'a [u8]), DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let Some((&[l0, l1, l2, l3], after_len)) = self.rest.split_first_chunk::<4>() else {
            self.rest = &[];
            return Some(Err(DecodeError::Corrupt("truncated chunk")));
        };
        let len = u32::from_be_bytes([l0, l1, l2, l3]);
        let Some((kind, after_kind)) = after_len.split_first_chunk::<4>() else {
            self.rest = &[];
            return Some(Err(DecodeError::Corrupt("truncated chunk")));
        };
        let body = match usize::try_from(len) {
            Ok(n) if len <= 0x7FFF_FFFF => after_kind.get(..n),
            _ => None,
        };
        let Some(body) = body else {
            self.rest = &[];
            return Some(Err(DecodeError::Corrupt("truncated chunk")));
        };
        self.rest = after_kind.get(body.len() + 4..).unwrap_or(&[]);
        Some(Ok((kind, body)))
    }
}

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
}

impl Header {
    fn parse(body: &[u8]) -> Result<Header, DecodeError> {
        let Some(&[w0, w1, w2, w3, h0, h1, h2, h3, depth, color, compression, filter, interlace]) =
            body.first_chunk::<13>().filter(|_| body.len() == 13)
        else {
            return Err(DecodeError::Corrupt("bad IHDR length"));
        };
        let width = u32::from_be_bytes([w0, w1, w2, w3]);
        let height = u32::from_be_bytes([h0, h1, h2, h3]);
        if width == 0 || height == 0 || width > 0x7FFF_FFFF || height > 0x7FFF_FFFF {
            return Err(DecodeError::Corrupt("bad image dimensions"));
        }
        let depth_ok = match color {
            0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
            3 => matches!(depth, 1 | 2 | 4 | 8),
            2 | 4 | 6 => matches!(depth, 8 | 16),
            _ => return Err(DecodeError::Corrupt("bad color type")),
        };
        if !depth_ok {
            return Err(DecodeError::Corrupt("bad bit depth for color type"));
        }
        if compression != 0 {
            return Err(DecodeError::Corrupt("bad compression method"));
        }
        if filter != 0 {
            return Err(DecodeError::Corrupt("bad filter method"));
        }
        match interlace {
            0 => {}
            1 => return Err(DecodeError::Unsupported("Adam7 interlaced PNG")),
            _ => return Err(DecodeError::Corrupt("bad interlace method")),
        }
        if depth == 16 {
            return Err(DecodeError::Unsupported("16-bit PNG channels"));
        }
        Ok(Header { width, height, depth, color })
    }

    fn channels(&self) -> u64 {
        match self.color {
            2 => 3,
            4 => 2,
            6 => 4,
            _ => 1, // 0, 3
        }
    }

    /// Buffer sizes, checked against `limits` before anything is allocated.
    fn layout(&self, limits: &Limits) -> Result<Layout, DecodeError> {
        if self.width > limits.max_width || self.height > limits.max_height {
            return Err(DecodeError::TooLarge);
        }
        let (w, h) = (u64::from(self.width), u64::from(self.height));
        let bits_per_pixel = self.channels() * u64::from(self.depth);
        let row_bytes = (w * bits_per_pixel).div_ceil(8);
        let to_usize = |n: Option<u64>| {
            n.and_then(|n| usize::try_from(n).ok()).filter(|&n| n <= limits.max_bytes).ok_or(DecodeError::TooLarge)
        };
        Ok(Layout {
            width: usize::try_from(w).map_err(|_| DecodeError::TooLarge)?,
            height: usize::try_from(h).map_err(|_| DecodeError::TooLarge)?,
            row_bytes: to_usize(Some(row_bytes))?,
            bpp: bits_per_pixel.div_ceil(8) as usize,
            raw_len: to_usize(h.checked_mul(row_bytes + 1))?,
            rgba_len: to_usize(w.checked_mul(h).and_then(|n| n.checked_mul(4)))?,
        })
    }
}

struct Layout {
    width: usize,
    height: usize,
    /// Bytes per scanline, without the filter byte.
    row_bytes: usize,
    /// Bytes per complete pixel, at least 1 (the filters' "left" distance).
    bpp: usize,
    /// Decompressed length: `height * (1 + row_bytes)`.
    raw_len: usize,
    /// Output length: `width * height * 4`.
    rgba_len: usize,
}

// ---------------------------------------------------------------------------
// Unfiltering
// ---------------------------------------------------------------------------

/// Undo a scanline's filter in place. `prev` is the previous scanline, already
/// unfiltered (zeros for the first one); it has the same length as `cur`. `bpp` is
/// 1 to 4 for every supported format, and both lengths are multiples of it.
fn unfilter(filter: u8, bpp: usize, prev: &[u8], cur: &mut [u8]) -> Result<(), DecodeError> {
    if filter > 4 {
        return Err(DecodeError::Corrupt("bad scanline filter"));
    }
    match bpp {
        1 => unfilter_px::<1>(filter, prev, cur),
        2 => unfilter_px::<2>(filter, prev, cur),
        3 => unfilter_px::<3>(filter, prev, cur),
        4 => unfilter_px::<4>(filter, prev, cur),
        _ => return Err(DecodeError::Unsupported("pixel size")),
    }
    Ok(())
}

/// [`unfilter`] for `N`-byte pixels: the "left" and "upper-left" bytes are carried in
/// small arrays, so the inner loops have no bounds checks.
#[inline(always)]
fn unfilter_px<const N: usize>(filter: u8, prev: &[u8], cur: &mut [u8]) {
    let pixels = cur.chunks_exact_mut(N).zip(prev.chunks_exact(N));
    let mut left = [0u8; N];
    let mut up_left = [0u8; N];
    match filter {
        1 => {
            for (px, _) in pixels {
                for i in 0..N {
                    px[i] = px[i].wrapping_add(left[i]);
                    left[i] = px[i];
                }
            }
        }
        2 => {
            for (c, &p) in cur.iter_mut().zip(prev) {
                *c = c.wrapping_add(p);
            }
        }
        3 => {
            for (px, up) in pixels {
                for i in 0..N {
                    px[i] = px[i].wrapping_add(((u16::from(left[i]) + u16::from(up[i])) >> 1) as u8);
                    left[i] = px[i];
                }
            }
        }
        4 => {
            // With nothing to the left (all zeros), Paeth picks the byte above.
            for (px, up) in pixels {
                for i in 0..N {
                    px[i] = px[i].wrapping_add(paeth(left[i], up[i], up_left[i]));
                    left[i] = px[i];
                    up_left[i] = up[i];
                }
            }
        }
        _ => {} // 0: None
    }
}

/// The Paeth predictor: whichever of left, above and upper-left is closest to
/// `left + above - upper_left`, preferring them in that order.
#[inline(always)]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (a16, b16, c16) = (i16::from(a), i16::from(b), i16::from(c));
    let pa = (b16 - c16).abs();
    let pb = (a16 - c16).abs();
    let pc = (a16 + b16 - 2 * c16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// Conversion to RGBA8
// ---------------------------------------------------------------------------

enum Format {
    /// Gray at 1, 2, 4 or 8 bits, with an optional transparent sample value.
    Gray {
        depth: u8,
        key: Option<u8>,
    },
    /// RGB8 with an optional transparent color.
    Rgb {
        key: Option<[u8; 3]>,
    },
    /// Palette indices at 1, 2, 4 or 8 bits, looked up in a 256-entry RGBA table.
    Palette {
        depth: u8,
        table: Box<[[u8; 4]; 256]>,
    },
    GrayAlpha,
    Rgba,
}

impl Format {
    fn new(h: &Header, palette: Option<&[u8]>, trns: Option<&[u8]>) -> Result<Format, DecodeError> {
        Ok(match h.color {
            // A tRNS key is compared on its low byte, as the `png` crate and libpng do
            // for bit depths below 16. A short tRNS is ignored.
            0 => Format::Gray { depth: h.depth, key: trns.and_then(|t| t.get(1)).copied() },
            2 => Format::Rgb { key: trns.and_then(|t| t.first_chunk::<6>()).map(|&[_, r, _, g, _, b]| [r, g, b]) },
            3 => {
                let plte = palette.ok_or(DecodeError::Corrupt("missing PLTE"))?;
                if plte.is_empty() || plte.len() % 3 != 0 || plte.len() > 3 * 256 {
                    return Err(DecodeError::Corrupt("bad PLTE length"));
                }
                // Indices past the palette read as opaque black.
                let mut table = Box::new([[0, 0, 0, 0xFF]; 256]);
                for (entry, rgb) in table.iter_mut().zip(plte.chunks_exact(3)) {
                    entry[..3].copy_from_slice(rgb);
                }
                // A tRNS with more entries than the palette is ignored.
                if let Some(alpha) = trns.filter(|t| t.len() <= plte.len() / 3) {
                    for (entry, &a) in table.iter_mut().zip(alpha) {
                        entry[3] = a;
                    }
                }
                Format::Palette { depth: h.depth, table }
            }
            4 => Format::GrayAlpha,
            _ => Format::Rgba,
        })
    }

    /// Convert one unfiltered scanline to RGBA8 (`out` is `width * 4` bytes).
    fn expand(&self, row: &[u8], out: &mut [u8]) {
        match self {
            Format::Rgba => {
                for (o, &i) in out.iter_mut().zip(row) {
                    *o = i;
                }
            }
            Format::GrayAlpha => {
                for (o, i) in out.chunks_exact_mut(4).zip(row.chunks_exact(2)) {
                    o.copy_from_slice(&[i[0], i[0], i[0], i[1]]);
                }
            }
            Format::Rgb { key } => {
                for (o, i) in out.chunks_exact_mut(4).zip(row.chunks_exact(3)) {
                    let rgb = [i[0], i[1], i[2]];
                    let a = if Some(rgb) == *key { 0 } else { 0xFF };
                    o.copy_from_slice(&[i[0], i[1], i[2], a]);
                }
            }
            Format::Gray { depth: 8, key } => {
                for (o, &g) in out.chunks_exact_mut(4).zip(row) {
                    let a = if Some(g) == *key { 0 } else { 0xFF };
                    o.copy_from_slice(&[g, g, g, a]);
                }
            }
            Format::Gray { depth, key } => {
                // 1, 2, 4 bits: scale to 0..255 by replicating the bits.
                let scale = 255 / ((1u8 << depth) - 1);
                unpack(row, *depth, out, |v, o| {
                    let g = v * scale;
                    let a = if Some(v) == *key { 0 } else { 0xFF };
                    o.copy_from_slice(&[g, g, g, a]);
                });
            }
            Format::Palette { depth: 8, table } => {
                for (o, &i) in out.chunks_exact_mut(4).zip(row) {
                    o.copy_from_slice(&table[usize::from(i)]);
                }
            }
            Format::Palette { depth, table } => {
                unpack(row, *depth, out, |v, o| o.copy_from_slice(&table[usize::from(v)]));
            }
        }
    }
}

/// Call `f` for each `depth`-bit sample (1, 2 or 4 bits, most significant first) and
/// its 4-byte output pixel. Padding bits at the end of the row are ignored.
#[inline(always)]
fn unpack(row: &[u8], depth: u8, out: &mut [u8], mut f: impl FnMut(u8, &mut [u8])) {
    let per_byte = usize::from(8 / depth);
    let mask = (1u8 << depth) - 1;
    for (&byte, pixels) in row.iter().zip(out.chunks_mut(4 * per_byte)) {
        for (k, o) in pixels.chunks_exact_mut(4).enumerate() {
            let shift = 8 - depth * (k as u8 + 1);
            f((byte >> shift) & mask, o);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paeth_matches_spec() {
        for (a, b, c) in [(0, 0, 0), (10, 20, 30), (200, 10, 100), (255, 255, 0), (1, 250, 128), (90, 80, 85)] {
            let p = i32::from(a) + i32::from(b) - i32::from(c);
            let (pa, pb, pc) = ((p - i32::from(a)).abs(), (p - i32::from(b)).abs(), (p - i32::from(c)).abs());
            let want = if pa <= pb && pa <= pc {
                a
            } else if pb <= pc {
                b
            } else {
                c
            };
            assert_eq!(paeth(a, b, c), want);
        }
    }

    #[test]
    fn unpack_bits() {
        let mut out = [0u8; 4 * 5];
        let mut seen = Vec::new();
        unpack(&[0b1101_0010, 0b0100_0000], 2, &mut out, |v, _| seen.push(v));
        assert_eq!(seen, [3, 1, 0, 2, 1]);
    }

    #[test]
    fn not_png() {
        let l = Limits::default();
        assert_eq!(decode(b"", &l), Err(DecodeError::UnknownFormat));
        assert_eq!(decode(b"GIF89a", &l), Err(DecodeError::UnknownFormat));
        assert_eq!(decode(&SIGNATURE[..7], &l), Err(DecodeError::UnknownFormat));
        assert_eq!(decode(&SIGNATURE, &l), Err(DecodeError::Corrupt("missing IHDR")));
    }
}
