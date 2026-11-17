//! zlib (RFC 1950) and DEFLATE (RFC 1951) decompression.
//!
//! Supports stored, fixed-Huffman and dynamic-Huffman blocks. Huffman codes are decoded
//! through two-level lookup tables (one probe for short codes, two for long ones), never
//! by walking a tree bit by bit. The zlib Adler-32 trailer is checked; preset
//! dictionaries are rejected. Code-set validation follows zlib: over-subscribed codes are
//! errors, and incomplete codes are allowed only for a single literal/length or distance
//! code of length 1.
//!
//! Not a port: TB decodes images through the vendored stb_image (tb_image_loader_stb.cpp).
//! Every input is handled without panicking, and the output never grows past `max_out`.

use alloc::vec::Vec;
use core::fmt;

/// Why a stream could not be decompressed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InflateError {
    /// The input ended before the end of the stream (or its checksum).
    Truncated,
    /// A bad zlib header: compression method, window size, check bits or a preset dictionary.
    BadHeader,
    /// A block of the reserved type 3.
    BadBlockType,
    /// A stored block whose length does not match its one's complement.
    BadStoredLength,
    /// Code lengths that do not form a usable Huffman code.
    BadCodeLengths,
    /// An unused code, or a length (286, 287) or distance (30, 31) symbol that cannot occur.
    BadSymbol,
    /// A back-reference to before the start of the output.
    BadDistance,
    /// The output would grow past the caller's `max_out`.
    OutputLimit,
    /// The Adler-32 of the output does not match the stream's trailer.
    BadChecksum,
}

impl InflateError {
    /// A short static description (used for `DecodeError::Corrupt`).
    pub fn as_str(self) -> &'static str {
        match self {
            InflateError::Truncated => "truncated zlib stream",
            InflateError::BadHeader => "bad zlib header",
            InflateError::BadBlockType => "bad deflate block type",
            InflateError::BadStoredLength => "bad stored block length",
            InflateError::BadCodeLengths => "bad huffman code lengths",
            InflateError::BadSymbol => "bad huffman symbol",
            InflateError::BadDistance => "deflate distance too far back",
            InflateError::OutputLimit => "decompressed data too large",
            InflateError::BadChecksum => "zlib checksum mismatch",
        }
    }
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Decompress a zlib stream, failing with [`InflateError::OutputLimit`] if the output
/// would exceed `max_out` bytes. Bytes after the Adler-32 trailer are ignored.
pub fn zlib_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    zlib_decompress_with_capacity(data, 0, max_out)
}

/// [`zlib_decompress`] with the output pre-allocated to `capacity` bytes (clamped to
/// `max_out`), for callers that know the decompressed length.
pub fn zlib_decompress_with_capacity(data: &[u8], capacity: usize, max_out: usize) -> Result<Vec<u8>, InflateError> {
    let (&[cmf, flg], body) = data.split_first_chunk::<2>().ok_or(InflateError::Truncated)?;
    let check = (u16::from(cmf) << 8 | u16::from(flg)) % 31;
    if cmf & 0x0F != 8 || cmf >> 4 > 7 || check != 0 || flg & 0x20 != 0 {
        return Err(InflateError::BadHeader);
    }
    let mut out = Vec::with_capacity(capacity.min(max_out));
    let mut inf = Inflater::new(body);
    inf.run(&mut out, max_out)?;
    let end = inf.bits.byte_position();
    let trailer = body.get(end..).and_then(|s| s.first_chunk::<4>()).ok_or(InflateError::Truncated)?;
    if u32::from_be_bytes(*trailer) != adler32(&out) {
        return Err(InflateError::BadChecksum);
    }
    Ok(out)
}

/// Decompress a raw DEFLATE stream (no zlib header or trailer). Bytes after the final
/// block are ignored.
pub fn deflate_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    let mut out = Vec::new();
    Inflater::new(data).run(&mut out, max_out)?;
    Ok(out)
}

/// The Adler-32 checksum of `data` (RFC 1950).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    // zlib's NMAX: the longest run for which the sums cannot overflow a u32.
    const NMAX: usize = 5552;
    let (mut a, mut b) = (1u32, 0u32);
    // Four interleaved lanes break the serial a -> b dependency. Byte j of a block of
    // L = 4n bytes adds (L - j) * x to b; for j = 4i + k that is 4 (n - i) - k, so
    // b gains L * a + 4 * sum(lane b) - sum(k * lane a).
    for block in data.chunks(4 * NMAX) {
        let (quads, tail) = block.split_at(block.len() & !3);
        let (mut la, mut lb) = ([0u32; 4], [0u32; 4]);
        for q in quads.chunks_exact(4) {
            for k in 0..4 {
                la[k] += u32::from(q[k]);
                lb[k] += la[k];
            }
        }
        b = (b + quads.len() as u32 * a) % MOD;
        for k in 0..4 {
            la[k] %= MOD;
            lb[k] %= MOD;
            a += la[k];
            b += 4 * lb[k] + k as u32 * (MOD - la[k]);
        }
        for &x in tail {
            a += u32::from(x);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------------------
// Bit reader
// ---------------------------------------------------------------------------

/// LSB-first bit reader with a 64-bit buffer. Invariant: `cnt <= 63`, and the bits of
/// `buf` above `cnt` are either zero or the stream's next bits.
struct Bits<'a> {
    input: &'a [u8],
    pos: usize,
    buf: u64,
    cnt: u32,
}

impl<'a> Bits<'a> {
    fn new(input: &'a [u8]) -> Self {
        Bits { input, pos: 0, buf: 0, cnt: 0 }
    }

    /// Fill the buffer to at least 56 bits, or with whatever input remains.
    #[inline(always)]
    fn refill(&mut self) {
        if let Some(word) = self.input.get(self.pos..).and_then(|s| s.first_chunk::<8>()) {
            // Branchless refill: load 8 bytes, keep the whole bytes that fit.
            self.buf |= u64::from_le_bytes(*word) << self.cnt;
            self.pos += ((63 - self.cnt) >> 3) as usize;
            self.cnt |= 56;
        } else {
            while self.cnt < 56 {
                let Some(&b) = self.input.get(self.pos) else { break };
                self.buf |= u64::from(b) << self.cnt;
                self.pos += 1;
                self.cnt += 8;
            }
        }
    }

    /// Drop `n` bits (`n <= cnt`).
    #[inline(always)]
    fn consume(&mut self, n: u32) {
        self.buf >>= n;
        self.cnt -= n;
    }

    /// Read `n <= 16` bits.
    #[inline(always)]
    fn take(&mut self, n: u32) -> Result<u32, InflateError> {
        if self.cnt < n {
            self.refill();
            if self.cnt < n {
                return Err(InflateError::Truncated);
            }
        }
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.consume(n);
        Ok(v)
    }

    /// Skip to the next byte boundary and hand the buffered whole bytes back to the
    /// input. Returns the byte position of the next unread byte.
    fn byte_position(&mut self) -> usize {
        // Every counted bit came from a byte already passed by `pos`.
        self.pos = self.pos.saturating_sub((self.cnt / 8) as usize);
        self.buf = 0;
        self.cnt = 0;
        self.pos
    }
}

// ---------------------------------------------------------------------------
// Huffman tables
// ---------------------------------------------------------------------------

// A table entry is a u32:
//   bits 0..4   code length in bits; for K_SUB, the subtable's index width
//   bits 4..8   extra bits that follow the code (lengths and distances)
//   bits 8..11  kind
//   bits 16..32 literal byte, length or distance base, code-length symbol, or subtable offset
const K_INVALID: u32 = 0;
const K_LITERAL: u32 = 1 << 8;
const K_BASE: u32 = 2 << 8;
const K_END: u32 = 3 << 8;
const K_SUB: u32 = 4 << 8;
const KIND_MASK: u32 = 7 << 8;

const LIT_BITS: u32 = 10;
const DIST_BITS: u32 = 8;
const CL_BITS: u32 = 7;

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// The order in which code-length code lengths are stored.
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

fn litlen_entry(sym: usize) -> u32 {
    match sym {
        0..=255 => K_LITERAL | (sym as u32) << 16,
        256 => K_END,
        257..=285 => {
            let i = sym - 257;
            K_BASE | u32::from(LEN_EXTRA[i]) << 4 | u32::from(LEN_BASE[i]) << 16
        }
        _ => K_INVALID,
    }
}

fn dist_entry(sym: usize) -> u32 {
    match (DIST_BASE.get(sym), DIST_EXTRA.get(sym)) {
        (Some(&base), Some(&extra)) => K_BASE | u32::from(extra) << 4 | u32::from(base) << 16,
        _ => K_INVALID,
    }
}

fn code_length_entry(sym: usize) -> u32 {
    K_LITERAL | (sym as u32) << 16
}

/// A canonical Huffman code as a primary table of `1 << bits` entries, followed by
/// subtables for codes longer than `bits`.
struct Huffman {
    table: Vec<u32>,
    bits: u32,
}

impl Huffman {
    fn new(bits: u32) -> Self {
        Huffman { table: Vec::new(), bits }
    }

    /// Build from per-symbol code lengths (0 = unused). `entry` gives each symbol's
    /// kind, extra bits and value. An empty code is accepted (every lookup fails).
    fn build(&mut self, lengths: &[u8], allow_single: bool, entry: fn(usize) -> u32) -> Result<(), InflateError> {
        let mut count = [0u32; 16];
        for &l in lengths {
            count[usize::from(l & 15)] += 1;
        }
        count[0] = 0;
        let mut left: i64 = 1;
        let mut max_len = 0u32;
        for (len, &c) in count.iter().enumerate().skip(1) {
            left = (left << 1) - i64::from(c);
            if left < 0 {
                return Err(InflateError::BadCodeLengths); // over-subscribed
            }
            if c != 0 {
                max_len = len as u32;
            }
        }
        let primary = 1usize << self.bits;
        self.table.clear();
        self.table.resize(primary, K_INVALID);
        if max_len == 0 {
            return Ok(());
        }
        if left > 0 && !(allow_single && max_len == 1) {
            return Err(InflateError::BadCodeLengths); // incomplete
        }

        let mut next = [0u32; 16];
        let mut code = 0u32;
        for len in 1..16 {
            code = (code + count[len - 1]) << 1;
            next[len] = code;
        }

        let sub_bits = max_len.saturating_sub(self.bits);
        let sub_size = 1usize << sub_bits;
        for (sym, &l) in lengths.iter().enumerate() {
            let len = u32::from(l & 15);
            if len == 0 {
                continue;
            }
            let code = next[len as usize];
            next[len as usize] += 1;
            let rev = (code.reverse_bits() >> (32 - len)) as usize;
            let e = entry(sym) | len;
            if len <= self.bits {
                for slot in self.table.iter_mut().take(primary).skip(rev).step_by(1 << len) {
                    *slot = e;
                }
            } else {
                let prefix = rev & (primary - 1);
                let head = self.table.get(prefix).copied().unwrap_or(K_INVALID);
                let base = if head & KIND_MASK == K_SUB {
                    (head >> 16) as usize
                } else if head == K_INVALID {
                    let base = self.table.len();
                    self.table.resize(base + sub_size, K_INVALID);
                    if let Some(slot) = self.table.get_mut(prefix) {
                        *slot = K_SUB | sub_bits | (base as u32) << 16;
                    }
                    base
                } else {
                    return Err(InflateError::BadCodeLengths); // cannot happen for a prefix code
                };
                let sub = self.table.get_mut(base..base + sub_size).ok_or(InflateError::BadCodeLengths)?;
                for slot in sub.iter_mut().skip(rev >> self.bits).step_by(1 << (len - self.bits)) {
                    *slot = e;
                }
            }
        }
        Ok(())
    }

    /// The entry for the code at the bottom of `buf`.
    #[inline(always)]
    fn lookup(&self, buf: u64) -> u32 {
        let mask = (1usize << self.bits) - 1;
        let e = self.table.get(buf as usize & mask).copied().unwrap_or(K_INVALID);
        if e & KIND_MASK != K_SUB {
            return e;
        }
        let idx = (e >> 16) as usize + ((buf >> self.bits) as usize & ((1usize << (e & 15)) - 1));
        self.table.get(idx).copied().unwrap_or(K_INVALID)
    }
}

/// Decode one symbol of `h`, returning its entry (never `K_INVALID` or `K_SUB`).
#[inline(always)]
fn decode(bits: &mut Bits<'_>, h: &Huffman) -> Result<u32, InflateError> {
    bits.refill();
    let e = h.lookup(bits.buf);
    let n = e & 15;
    if n > bits.cnt {
        return Err(InflateError::Truncated);
    }
    if e & KIND_MASK == K_INVALID {
        return Err(InflateError::BadSymbol);
    }
    bits.consume(n);
    Ok(e)
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

struct Inflater<'a> {
    bits: Bits<'a>,
    lit: Huffman,
    dist: Huffman,
    cl: Huffman,
}

impl<'a> Inflater<'a> {
    fn new(input: &'a [u8]) -> Self {
        Inflater {
            bits: Bits::new(input),
            lit: Huffman::new(LIT_BITS),
            dist: Huffman::new(DIST_BITS),
            cl: Huffman::new(CL_BITS),
        }
    }

    /// Decode blocks up to and including the final one.
    fn run(&mut self, out: &mut Vec<u8>, max_out: usize) -> Result<(), InflateError> {
        loop {
            let header = self.bits.take(3)?;
            match header >> 1 {
                0 => self.stored(out, max_out)?,
                1 => {
                    self.fixed_tables()?;
                    self.codes(out, max_out)?;
                }
                2 => {
                    self.dynamic_tables()?;
                    self.codes(out, max_out)?;
                }
                _ => return Err(InflateError::BadBlockType),
            }
            if header & 1 != 0 {
                return Ok(());
            }
        }
    }

    fn stored(&mut self, out: &mut Vec<u8>, max_out: usize) -> Result<(), InflateError> {
        let pos = self.bits.byte_position();
        let input = self.bits.input;
        let &[l0, l1, n0, n1] = input.get(pos..).and_then(|s| s.first_chunk::<4>()).ok_or(InflateError::Truncated)?;
        let len = u16::from_le_bytes([l0, l1]);
        if len != !u16::from_le_bytes([n0, n1]) {
            return Err(InflateError::BadStoredLength);
        }
        let len = usize::from(len);
        let start = pos + 4;
        let data = input.get(start..start + len).ok_or(InflateError::Truncated)?;
        if len > max_out.saturating_sub(out.len()) {
            return Err(InflateError::OutputLimit);
        }
        out.extend_from_slice(data);
        self.bits.pos = start + len;
        Ok(())
    }

    fn fixed_tables(&mut self) -> Result<(), InflateError> {
        let mut lens = [0u8; 288];
        for (i, l) in lens.iter_mut().enumerate() {
            *l = match i {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        self.lit.build(&lens, false, litlen_entry)?;
        self.dist.build(&[5; 32], false, dist_entry)
    }

    fn dynamic_tables(&mut self) -> Result<(), InflateError> {
        let hlit = self.bits.take(5)? as usize + 257;
        let hdist = self.bits.take(5)? as usize + 1;
        let hclen = self.bits.take(4)? as usize + 4;
        if hlit > 286 || hdist > 30 {
            return Err(InflateError::BadCodeLengths);
        }
        let mut cl_lens = [0u8; 19];
        for &i in CL_ORDER.iter().take(hclen) {
            cl_lens[i] = self.bits.take(3)? as u8;
        }
        self.cl.build(&cl_lens, false, code_length_entry)?;

        let mut lens = [0u8; 286 + 30];
        let total = hlit + hdist;
        let mut i = 0;
        while i < total {
            let sym = decode(&mut self.bits, &self.cl)? >> 16;
            let (value, repeat) = match sym {
                0..=15 => (sym as u8, 1),
                16 => {
                    let prev = i.checked_sub(1).and_then(|p| lens.get(p)).copied();
                    (prev.ok_or(InflateError::BadCodeLengths)?, 3 + self.bits.take(2)? as usize)
                }
                17 => (0, 3 + self.bits.take(3)? as usize),
                _ => (0, 11 + self.bits.take(7)? as usize),
            };
            let run =
                lens.get_mut(i..i + repeat).filter(|_| i + repeat <= total).ok_or(InflateError::BadCodeLengths)?;
            run.fill(value);
            i += repeat;
        }
        if lens[256] == 0 {
            return Err(InflateError::BadCodeLengths); // no end-of-block code
        }
        self.lit.build(&lens[..hlit], true, litlen_entry)?;
        self.dist.build(&lens[hlit..total], true, dist_entry)
    }

    /// Decode a Huffman-coded block's symbols up to its end-of-block code.
    fn codes(&mut self, out: &mut Vec<u8>, max_out: usize) -> Result<(), InflateError> {
        let bits = &mut self.bits;
        loop {
            let e = decode(bits, &self.lit)?;
            match e & KIND_MASK {
                K_LITERAL => {
                    if out.len() >= max_out {
                        return Err(InflateError::OutputLimit);
                    }
                    out.push((e >> 16) as u8);
                }
                K_BASE => {
                    // After a refill there are >= 56 bits; a match needs at most 48.
                    let len = (e >> 16) as usize + bits.take((e >> 4) & 15)? as usize;
                    let d = decode(bits, &self.dist)?;
                    let dist = (d >> 16) as usize + bits.take((d >> 4) & 15)? as usize;
                    copy_match(out, dist, len, max_out)?;
                }
                K_END => return Ok(()),
                _ => return Err(InflateError::BadSymbol),
            }
        }
    }
}

/// Append `len` bytes copied from `dist` bytes back (the ranges may overlap).
#[inline(always)]
fn copy_match(out: &mut Vec<u8>, dist: usize, len: usize, max_out: usize) -> Result<(), InflateError> {
    let pos = out.len();
    if dist > pos {
        return Err(InflateError::BadDistance);
    }
    if len > max_out.saturating_sub(pos) {
        return Err(InflateError::OutputLimit);
    }
    let start = pos - dist;
    if len <= dist {
        out.extend_from_within(start..start + len);
    } else if dist == 1 {
        let b = out.get(start).copied().unwrap_or(0);
        out.resize(pos + len, b);
    } else {
        // The source repeats with period `dist`, so re-copying from `start` in growing
        // chunks reproduces the byte-by-byte overlapping copy.
        let mut left = len;
        while left > 0 {
            let n = left.min(out.len() - start);
            out.extend_from_within(start..start + n);
            left -= n;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// LSB-first bit writer for building test streams by hand.
    #[derive(Default)]
    struct Writer {
        out: Vec<u8>,
        acc: u64,
        n: u32,
    }

    impl Writer {
        fn bits(&mut self, v: u32, n: u32) {
            self.acc |= u64::from(v) << self.n;
            self.n += n;
            while self.n >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.n -= 8;
            }
        }
        /// A Huffman code, most significant bit first.
        fn code(&mut self, code: u32, len: u32) {
            self.bits(code.reverse_bits() >> (32 - len), len);
        }
        fn align(&mut self) {
            if self.n > 0 {
                self.bits(0, 8 - self.n);
            }
        }
        fn finish(mut self) -> Vec<u8> {
            self.align();
            self.out
        }
        fn fixed_sym(&mut self, sym: u32) {
            match sym {
                0..=143 => self.code(0x30 + sym, 8),
                144..=255 => self.code(0x190 + sym - 144, 9),
                256..=279 => self.code(sym - 256, 7),
                _ => self.code(0xC0 + sym - 280, 8),
            }
        }
        fn fixed_match(&mut self, len: u32, dist: u32) {
            let i = LEN_BASE.iter().rposition(|&b| u32::from(b) <= len).unwrap();
            self.fixed_sym(257 + i as u32);
            self.bits(len - u32::from(LEN_BASE[i]), u32::from(LEN_EXTRA[i]));
            let j = DIST_BASE.iter().rposition(|&b| u32::from(b) <= dist).unwrap();
            self.code(j as u32, 5);
            self.bits(dist - u32::from(DIST_BASE[j]), u32::from(DIST_EXTRA[j]));
        }
        fn stored(&mut self, data: &[u8], last: bool) {
            self.bits(u32::from(last), 1);
            self.bits(0, 2);
            self.align();
            let len = data.len() as u16;
            self.out.extend_from_slice(&len.to_le_bytes());
            self.out.extend_from_slice(&(!len).to_le_bytes());
            self.out.extend_from_slice(data);
        }
    }

    fn zlib_wrap(deflate: &[u8], plain: &[u8]) -> Vec<u8> {
        let mut z = vec![0x78, 0x01];
        z.extend_from_slice(deflate);
        z.extend_from_slice(&adler32(plain).to_be_bytes());
        z
    }

    fn noise(n: usize, mut seed: u32) -> Vec<u8> {
        (0..n)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect()
    }

    #[test]
    fn adler32_known_values() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        // Reference: a byte-at-a-time modular computation.
        let reference = |data: &[u8]| {
            let (mut a, mut b) = (1u64, 0u64);
            for &x in data {
                a = (a + u64::from(x)) % 65521;
                b = (b + a) % 65521;
            }
            (b << 16 | a) as u32
        };
        let ones = vec![0xFF; 100_000];
        let mixed = noise(100_000, 5);
        for n in (0..70).chain([22_207, 22_208, 22_209, 22_211, 44_416, 44_419, 99_999, 100_000]) {
            assert_eq!(adler32(&ones[..n]), reference(&ones[..n]), "0xFF x {n}");
            assert_eq!(adler32(&mixed[..n]), reference(&mixed[..n]), "noise x {n}");
        }
    }

    #[test]
    fn stored_blocks() {
        let mut w = Writer::default();
        w.stored(b"hello ", false);
        w.stored(b"", false);
        w.stored(b"world", true);
        let z = zlib_wrap(&w.finish(), b"hello world");
        assert_eq!(zlib_decompress(&z, 1 << 20).unwrap(), b"hello world");
    }

    #[test]
    fn stored_block_after_huffman_block_mid_byte() {
        // A fixed block of 3 literals leaves the reader mid-byte; the stored block
        // must start at the next byte boundary.
        let mut w = Writer::default();
        w.bits(0, 1);
        w.bits(1, 2);
        for &b in b"abc" {
            w.fixed_sym(u32::from(b));
        }
        w.fixed_sym(256);
        let big = noise(70_000, 7);
        w.stored(&big[..65_535], false);
        w.stored(&big[65_535..], true);
        let mut plain = b"abc".to_vec();
        plain.extend_from_slice(&big);
        let z = zlib_wrap(&w.finish(), &plain);
        assert_eq!(zlib_decompress(&z, 1 << 20).unwrap(), plain);
    }

    #[test]
    fn fixed_huffman() {
        let mut w = Writer::default();
        w.bits(1, 1);
        w.bits(1, 2);
        for &b in b"abc\xFF\x90" {
            w.fixed_sym(u32::from(b));
        }
        w.fixed_match(10, 3); // overlapping: "\xFF\x90" + ...
        w.fixed_match(258, 1);
        w.fixed_sym(256);
        let mut plain = b"abc\xFF\x90".to_vec();
        for _ in 0..10 {
            plain.push(plain[plain.len() - 3]);
        }
        let last = *plain.last().unwrap();
        plain.extend(core::iter::repeat_n(last, 258));
        let z = zlib_wrap(&w.finish(), &plain);
        assert_eq!(zlib_decompress(&z, 1 << 20).unwrap(), plain);
        // The same stream without the zlib wrapper.
        assert_eq!(deflate_decompress(&z[2..z.len() - 4], 1 << 20).unwrap(), plain);
    }

    #[test]
    fn all_match_lengths_and_distances() {
        let mut w = Writer::default();
        w.bits(1, 1);
        w.bits(1, 2);
        let mut plain = noise(33_000, 3);
        for &b in &plain {
            w.fixed_sym(u32::from(b));
        }
        let mut seed = 99u32;
        for len in 3..=258u32 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let dist = 1 + (seed >> 8) % 32_768;
            w.fixed_match(len, dist);
            for _ in 0..len {
                plain.push(plain[plain.len() - dist as usize]);
            }
        }
        w.fixed_sym(256);
        let z = zlib_wrap(&w.finish(), &plain);
        assert_eq!(zlib_decompress(&z, 1 << 20).unwrap(), plain);
    }

    #[test]
    fn distance_32768() {
        let data = noise(32_768, 11);
        let mut w = Writer::default();
        w.bits(1, 1);
        w.bits(1, 2);
        for &b in &data {
            w.fixed_sym(u32::from(b));
        }
        w.fixed_match(258, 32_768);
        w.fixed_match(3, 32_768);
        w.fixed_sym(256);
        let mut plain = data.clone();
        plain.extend_from_slice(&data[..258]);
        plain.extend_from_slice(&data[258..261]);
        let z = zlib_wrap(&w.finish(), &plain);
        assert_eq!(zlib_decompress(&z, 1 << 20).unwrap(), plain);
    }

    #[test]
    fn distance_past_start() {
        let mut w = Writer::default();
        w.bits(1, 1);
        w.bits(1, 2);
        w.fixed_sym(u32::from(b'a'));
        w.fixed_match(3, 2);
        w.fixed_sym(256);
        assert_eq!(deflate_decompress(&w.finish(), 100), Err(InflateError::BadDistance));
    }

    /// The text behind `DYNAMIC`, from a fixed LCG.
    fn dynamic_text() -> Vec<u8> {
        let words: [&[u8]; 12] = [
            b"alpha", b"beta", b"gamma", b"delta", b"epsilon", b"zeta", b"eta", b"theta", b"iota", b"kappa", b"lambda",
            b"mu",
        ];
        let mut x: u64 = 4;
        let mut parts = Vec::new();
        for _ in 0..60 {
            x = (x * 1_103_515_245 + 12_345) & 0x7FFF_FFFF;
            parts.push(words[(x % 12) as usize]);
        }
        let mut text = parts.join(&b" "[..]);
        text.push(b'.');
        text
    }

    /// `zlib.compress(dynamic_text(), 9)`: one dynamic block whose code-length code uses
    /// the repeat symbols 16, 17 and 18.
    const DYNAMIC: [u8; 111] = [
        0x78, 0xda, 0x85, 0x4f, 0x5b, 0x0e, 0x80, 0x20, 0x0c, 0xbb, 0xca, 0x4e, 0xe0, 0x9d, 0x46, 0x20, 0x4a, 0xdc,
        0x64, 0x89, 0xf8, 0xc3, 0xe9, 0xe5, 0xa1, 0xc1, 0x45, 0xa2, 0x1f, 0x2c, 0xac, 0x6d, 0xda, 0x2e, 0xb9, 0x88,
        0x40, 0xc8, 0xc6, 0x22, 0xc4, 0xa5, 0x2c, 0x4e, 0x76, 0x4f, 0x61, 0x03, 0x53, 0x96, 0x19, 0x99, 0x11, 0xac,
        0xa3, 0xfc, 0xf7, 0x21, 0x8f, 0x54, 0x25, 0xf1, 0x43, 0x3c, 0x20, 0x2e, 0xff, 0x66, 0x33, 0x62, 0xf8, 0xd0,
        0x70, 0x79, 0x19, 0xeb, 0x89, 0x5d, 0xd7, 0xb1, 0x16, 0x77, 0x43, 0x2b, 0x8a, 0xe8, 0x0a, 0x15, 0x7e, 0x5d,
        0x81, 0x24, 0x8b, 0x36, 0xd5, 0x85, 0x9f, 0x3e, 0xba, 0x70, 0x63, 0x7e, 0x6f, 0xe9, 0xe1, 0xd3, 0x09, 0xcf,
        0x9f, 0x7e, 0xcd,
    ];

    #[test]
    fn dynamic_huffman() {
        assert_eq!(DYNAMIC[2] & 0b111, 0b101, "final dynamic block");
        let text = dynamic_text();
        assert_eq!(text.len(), 349);
        assert_eq!(zlib_decompress(&DYNAMIC, 1 << 20).unwrap(), text);
        assert_eq!(zlib_decompress_with_capacity(&DYNAMIC, 349, 349).unwrap(), text);
        assert_eq!(zlib_decompress(&DYNAMIC, 348), Err(InflateError::OutputLimit));
    }

    #[test]
    fn empty() {
        assert_eq!(zlib_decompress(&[], 100), Err(InflateError::Truncated));
        assert_eq!(zlib_decompress(&[0x78], 100), Err(InflateError::Truncated));
        assert_eq!(deflate_decompress(&[], 100), Err(InflateError::Truncated));
        // zlib.compress(b""): a single empty fixed block.
        let z = [0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(zlib_decompress(&z, 0).unwrap(), b"");
    }

    #[test]
    fn bad_checksum() {
        let mut z = DYNAMIC;
        z[110] ^= 1;
        assert_eq!(zlib_decompress(&z, 1 << 20), Err(InflateError::BadChecksum));
    }

    #[test]
    fn truncated() {
        for n in 0..DYNAMIC.len() {
            assert_eq!(zlib_decompress(&DYNAMIC[..n], 1 << 20), Err(InflateError::Truncated), "length {n}");
        }
        let mut w = Writer::default();
        w.stored(b"some stored bytes", true);
        let z = zlib_wrap(&w.finish(), b"some stored bytes");
        for n in 0..z.len() {
            assert_eq!(zlib_decompress(&z[..n], 1 << 20), Err(InflateError::Truncated), "length {n}");
        }
    }

    #[test]
    fn bad_headers() {
        let ok = [0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
        let mut z = ok;
        z[0] = 0x79; // compression method 9
        assert_eq!(zlib_decompress(&z, 10), Err(InflateError::BadHeader));
        z = ok;
        z[1] ^= 1; // check bits
        assert_eq!(zlib_decompress(&z, 10), Err(InflateError::BadHeader));
        z = ok;
        z[0] = 0x88; // window size 2^16
        z[1] = 0x1C; // with valid check bits
        assert_eq!((0x881Cu16) % 31, 0);
        assert_eq!(zlib_decompress(&z, 10), Err(InflateError::BadHeader));
        z = ok;
        z[1] = 0xBB; // FDICT set, check bits valid
        assert_eq!((0x78BBu16) % 31, 0);
        assert_eq!(zlib_decompress(&z, 10), Err(InflateError::BadHeader));
    }

    #[test]
    fn bad_blocks() {
        // Block type 3.
        assert_eq!(deflate_decompress(&[0b111], 10), Err(InflateError::BadBlockType));
        // Stored length mismatch.
        assert_eq!(deflate_decompress(&[1, 5, 0, 0xFA, 0xFE, 1, 2, 3, 4, 5], 10), Err(InflateError::BadStoredLength));
        // Stored block past the limit.
        assert_eq!(deflate_decompress(&[1, 5, 0, 0xFA, 0xFF, 1, 2, 3, 4, 5], 4), Err(InflateError::OutputLimit));
        // Fixed length symbol 286 and distance symbol 30.
        let mut w = Writer::default();
        w.bits(1, 1);
        w.bits(1, 2);
        w.fixed_sym(286);
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::BadSymbol));
        let mut w = Writer::default();
        w.bits(1, 1);
        w.bits(1, 2);
        w.fixed_sym(65);
        w.fixed_sym(257);
        w.code(30, 5);
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::BadSymbol));
        // A fixed block with no end-of-block code before the input ends.
        let mut w = Writer::default();
        w.bits(1, 1);
        w.bits(1, 2);
        w.fixed_sym(65);
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::Truncated));
        // Literals past the limit.
        let mut w = Writer::default();
        w.bits(1, 1);
        w.bits(1, 2);
        w.fixed_sym(65);
        w.fixed_sym(65);
        w.fixed_sym(256);
        let d = w.finish();
        assert_eq!(deflate_decompress(&d, 1), Err(InflateError::OutputLimit));
        assert_eq!(deflate_decompress(&d, 2).unwrap(), b"AA");
    }

    fn dynamic_header(w: &mut Writer, hlit: u32, hdist: u32, cl: &[u32; 19]) {
        w.bits(1, 1);
        w.bits(2, 2);
        w.bits(hlit - 257, 5);
        w.bits(hdist - 1, 5);
        w.bits(19 - 4, 4);
        for &i in &CL_ORDER {
            w.bits(cl[i], 3);
        }
    }

    #[test]
    fn bad_dynamic_codes() {
        // Over-subscribed code-length code: 19 codes of length 1.
        let mut w = Writer::default();
        dynamic_header(&mut w, 257, 1, &[1; 19]);
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::BadCodeLengths));

        // Incomplete code-length code: a single code of length 1 is not allowed here.
        let mut cl = [0; 19];
        cl[8] = 1;
        let mut w = Writer::default();
        dynamic_header(&mut w, 257, 1, &cl);
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::BadCodeLengths));

        // Code-length code: symbols 1 and 16 (lengths 1 each). Repeat with no previous length.
        let mut cl = [0; 19];
        cl[1] = 1;
        cl[16] = 1;
        let mut w = Writer::default();
        dynamic_header(&mut w, 257, 1, &cl);
        w.code(1, 1); // symbol 16 first
        w.bits(0, 2);
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::BadCodeLengths));

        // HLIT 287 is rejected.
        let mut w = Writer::default();
        dynamic_header(&mut w, 287, 1, &[1; 19]);
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::BadCodeLengths));

        // Code-length code: symbols 1 and 18. Lengths: 256 x 0 then 1 x 1 (EOB at length
        // 1, an incomplete single code, which is allowed), then the distance code: 1 x 1.
        let mut cl = [0; 19];
        cl[1] = 1;
        cl[18] = 1;
        let mut w = Writer::default();
        dynamic_header(&mut w, 257, 1, &cl);
        w.code(1, 1); // 18: 138 zeros
        w.bits(127, 7);
        w.code(1, 1); // 18: 118 zeros
        w.bits(107, 7);
        w.code(0, 1); // EOB length 1
        w.code(0, 1); // distance 0 length 1
        w.code(0, 1); // EOB
        assert_eq!(deflate_decompress(&w.finish(), 10).unwrap(), b"");

        // The same with a run that overflows the 258 lengths.
        let mut w = Writer::default();
        dynamic_header(&mut w, 257, 1, &cl);
        w.code(1, 1);
        w.bits(127, 7);
        w.code(1, 1);
        w.bits(127, 7);
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::BadCodeLengths));

        // No end-of-block code: EOB gets length 0.
        let mut w = Writer::default();
        dynamic_header(&mut w, 257, 1, &cl);
        w.code(1, 1);
        w.bits(127, 7);
        w.code(1, 1);
        w.bits(109, 7); // 120 zeros: all 257 literal/length codes and the distance code
        assert_eq!(deflate_decompress(&w.finish(), 10), Err(InflateError::BadCodeLengths));
    }

    /// Canonical (code, length) pairs for per-symbol code lengths.
    fn canonical(lens: &[u8]) -> Vec<(u32, u32)> {
        let mut count = [0u32; 16];
        for &l in lens {
            count[usize::from(l)] += 1;
        }
        count[0] = 0;
        let mut next = [0u32; 16];
        let mut c = 0;
        for b in 1..16 {
            c = (c + count[b - 1]) << 1;
            next[b] = c;
        }
        lens.iter()
            .map(|&l| {
                let v = next[usize::from(l)];
                next[usize::from(l)] += 1;
                (v, u32::from(l))
            })
            .collect()
    }

    #[test]
    fn long_codes_use_subtables() {
        // Literal/length code: literals 0..=12 get lengths 1..=13, length symbol 257 gets
        // 14, EOB and length symbol 258 get 15 (complete). Distance code: symbols 0..=14
        // get 1..=15 and symbol 15 gets 15 (complete). Both go past the primary tables.
        let mut lit = [0u8; 259];
        for (i, l) in lit.iter_mut().enumerate().take(13) {
            *l = i as u8 + 1;
        }
        lit[257] = 14;
        lit[256] = 15;
        lit[258] = 15;
        let mut dist = [0u8; 16];
        for (i, l) in dist.iter_mut().enumerate() {
            *l = (i as u8 + 1).min(15);
        }
        // Code-length code: 0..=15 all at length 4 (complete), no repeat symbols.
        let mut cl = [0; 19];
        cl[..16].fill(4);
        let mut w = Writer::default();
        dynamic_header(&mut w, 259, 16, &cl);
        for &l in lit.iter().chain(&dist) {
            w.code(u32::from(l), 4);
        }
        let lc = canonical(&lit);
        let dc = canonical(&dist);
        let mut plain = Vec::new();
        for i in 0..200usize {
            let sym = [0, 12, 5, 11, 1][i % 5];
            w.code(lc[sym].0, lc[sym].1);
            plain.push(sym as u8);
        }
        // (length symbol, length, distance symbol, extra bits, extra value, distance)
        for (ls, len, ds, ne, ev, d) in [(257, 3, 14, 6, 5, 134), (258, 4, 15, 6, 7, 200), (257, 3, 0, 0, 0, 1)] {
            w.code(lc[ls].0, lc[ls].1);
            w.code(dc[ds].0, dc[ds].1);
            w.bits(ev, ne);
            for _ in 0..len {
                plain.push(plain[plain.len() - d]);
            }
        }
        w.code(lc[256].0, lc[256].1);
        assert_eq!(deflate_decompress(&w.finish(), 1000).unwrap(), plain);
    }
}
