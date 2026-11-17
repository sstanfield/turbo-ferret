//! Turns a TrueType font into a Turbo Ferret bitmap font: a `.tb.txt` description
//! with a glyph table and one PNG per size (see `turbo_ferret::font::tbbf`).
//!
//! An app that ships the result needs neither a TrueType backend nor this crate:
//! the core's built-in bitmap font loader reads it, and it draws exactly what
//! `turbo-ferret-ttf` would have drawn at those sizes. The price is that the sizes
//! and the characters are fixed when the tool runs.
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author.

use std::fmt;
use std::fmt::Write as _;
use std::rc::Rc;

use turbo_ferret::font::{GlyphImage, GlyphMetrics, GlyphSource};
use turbo_ferret_ttf::{Font, TtfFace};

/// What to generate.
#[derive(Clone, Debug)]
pub struct Options {
    /// The name of the files, without extension: `<stem>.tb.txt`, `<stem>_<size>.png`.
    pub stem: String,
    /// Em sizes in pixels.
    pub sizes: Vec<u16>,
    /// The characters to include. Ones the font lacks are left out (and reported).
    pub chars: Vec<char>,
}

impl Options {
    /// Printable ASCII and Latin-1: U+0020 to U+007E and U+00A0 to U+00FF.
    pub fn default_chars() -> Vec<char> {
        (' '..='~').chain('\u{a0}'..='\u{ff}').collect()
    }
}

/// One generated bitmap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    pub size: u16,
    /// The file name the description refers to.
    pub file_name: String,
    pub width: u32,
    pub height: u32,
    /// Coverage, one byte per pixel, row by row.
    pub alpha: Vec<u8>,
}

impl Bitmap {
    /// The bitmap as a PNG file: white, with the coverage as alpha.
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.width, self.height);
        enc.set_color(png::ColorType::GrayscaleAlpha);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Best);
        let data: Vec<u8> = self.alpha.iter().flat_map(|&a| [255, a]).collect();
        // Writing into a Vec cannot fail, and the size matches the data.
        let mut writer = enc.write_header().expect("png header");
        writer.write_image_data(&data).expect("png data");
        drop(writer);
        out
    }
}

/// The generated font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// The contents of `<stem>.tb.txt`.
    pub description: String,
    pub bitmaps: Vec<Bitmap>,
    /// Characters that were asked for and are not in the font.
    pub missing: Vec<char>,
}

impl Output {
    /// The files to write: name and contents.
    pub fn files(&self, stem: &str) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![(format!("{stem}.tb.txt"), self.description.clone().into_bytes())];
        files.extend(self.bitmaps.iter().map(|b| (b.file_name.clone(), b.to_png())));
        files
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The file is not a TrueType font this tool can read.
    NotAFont,
    /// No size or no character was asked for, or the font has none of the characters.
    NothingToDo,
    /// A size's glyphs do not fit in the largest bitmap (4096 pixels wide and high).
    TooLarge(u16),
    /// A glyph of this size is larger than the core draws (a glyph cache page).
    GlyphTooLarge(u16, char),
    /// The description could not name its bitmaps with this stem: the resource
    /// parser would read the file name as numbers.
    BadStem(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotAFont => write!(f, "not a TrueType font (fonts with PostScript outlines are not supported)"),
            Error::NothingToDo => write!(f, "no sizes, or none of the characters are in the font"),
            Error::TooLarge(size) => write!(f, "the glyphs of size {size} do not fit in a 4096 x 4096 bitmap"),
            Error::GlyphTooLarge(size, c) => {
                let page = turbo_ferret::font::GLYPH_PAGE_SIZE;
                write!(
                    f,
                    "at size {size} the glyph U+{:04X} is larger than {} x {} pixels, the most that is drawn",
                    *c as u32, page.w, page.h
                )
            }
            Error::BadStem(stem) => write!(f, "the name {stem:?} cannot be used: it reads as a list of numbers"),
        }
    }
}

impl std::error::Error for Error {}

/// The largest bitmap side.
const MAX_SIDE: u32 = 4096;

/// Rasterize `ttf` at each size and pack the glyphs.
pub fn generate(ttf: &[u8], opts: &Options) -> Result<Output, Error> {
    let font = Rc::new(Font::parse(ttf.to_vec()).ok_or(Error::NotAFont)?);
    let mut chars = opts.chars.clone();
    chars.sort();
    chars.dedup();
    let missing: Vec<char> = chars.iter().copied().filter(|&c| font.glyph_index(c).is_none()).collect();
    chars.retain(|c| !missing.contains(c));
    let mut sizes = opts.sizes.clone();
    sizes.sort();
    sizes.dedup();
    if chars.is_empty() || sizes.is_empty() {
        return Err(Error::NothingToDo);
    }
    // The space comes along whether it was asked for or not: text without one
    // would otherwise draw a question mark there.
    if font.glyph_index(' ').is_some() && !chars.contains(&' ') {
        chars.insert(0, ' ');
    }

    // The description names its bitmaps by this stem, and has to read back.
    let probe = format!("{}_1.png", opts.stem);
    let readable = opts.stem.chars().all(|c| !c.is_control())
        && opts.stem.trim() == opts.stem
        && matches!(turbo_ferret::Value::parse_auto(&probe), turbo_ferret::Value::Str(_));
    if !readable || opts.stem.is_empty() {
        return Err(Error::BadStem(opts.stem.clone()));
    }

    let mut description = String::from("info\n\tgenerator turbo-ferret-fontgen\n");
    let mut bitmaps = Vec::new();
    for &size in &sizes {
        let mut face = TtfFace::new(font.clone(), size);
        let metrics = face.metrics();
        let mut glyphs: Vec<(char, GlyphMetrics, Vec<u8>)> = Vec::new();
        for &c in &chars {
            let m = face.glyph_metrics(c).unwrap_or_default();
            let mut img = GlyphImage::new();
            let data = if face.rasterize(c, &mut img) { img.data } else { Vec::new() };
            // A glyph with an outline but no image was too large to rasterize; one
            // larger than a glyph cache page would never be drawn.
            let page = turbo_ferret::font::GLYPH_PAGE_SIZE;
            let has_outline = face.font().glyph_index(c).is_some_and(|g| face.font().glyph_bounds(g).is_some());
            if (has_outline && data.is_empty()) || m.w > page.w || m.h > page.h {
                return Err(Error::GlyphTooLarge(size, c));
            }
            glyphs.push((c, m, data));
        }
        let (width, height, places) = pack(&glyphs).ok_or(Error::TooLarge(size))?;
        let mut alpha = vec![0u8; (width * height) as usize];
        for ((_, m, data), &(x, y)) in glyphs.iter().zip(&places) {
            for row in 0..m.h as usize {
                let dst = (y as usize + row) * width as usize + x as usize;
                alpha[dst..dst + m.w as usize].copy_from_slice(&data[row * m.w as usize..(row + 1) * m.w as usize]);
            }
        }

        let file_name = format!("{}_{size}.png", opts.stem);
        let _ = writeln!(description, "size {size}");
        let _ = writeln!(description, "\tbitmap {file_name}");
        let _ = writeln!(description, "\tascent {}", metrics.ascent);
        let _ = writeln!(description, "\tdescent {}", metrics.descent);
        let _ = writeln!(description, "\theight {}", metrics.height);
        let _ = writeln!(description, "\tglyphs");
        for ((c, m, _), &(x, y)) in glyphs.iter().zip(&places) {
            let (x, y) = if m.w > 0 && m.h > 0 { (x, y) } else { (0, 0) };
            let _ =
                writeln!(description, "\t\tU+{:04X} {x} {y} {} {} {} {} {}", *c as u32, m.w, m.h, m.x, m.y, m.advance);
        }
        bitmaps.push(Bitmap { size, file_name, width, height, alpha });
    }
    Ok(Output { description, bitmaps, missing })
}

/// A packed bitmap: its size and each glyph's position.
type Packed = (u32, u32, Vec<(u32, u32)>);

/// Place the glyph images in rows, tallest first, with a clear pixel between
/// them. Returns the bitmap's size and each glyph's position (in the order given).
fn pack(glyphs: &[(char, GlyphMetrics, Vec<u8>)]) -> Option<Packed> {
    let area: u64 = glyphs.iter().map(|(_, m, _)| (m.w as u64 + 1) * (m.h as u64 + 1)).sum();
    let widest = glyphs.iter().map(|(_, m, _)| m.w as u32 + 1).max().unwrap_or(1);
    // A power of two wide, about square.
    let mut width = 32u32;
    while u64::from(width) * u64::from(width) < area || width < widest {
        width *= 2;
    }
    if width > MAX_SIDE {
        return None;
    }
    let mut order: Vec<usize> = (0..glyphs.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(glyphs[i].1.h), i));
    let mut places = vec![(0u32, 0u32); glyphs.len()];
    let (mut x, mut y, mut row_h) = (0u32, 0u32, 0u32);
    for i in order {
        let (w, h) = (glyphs[i].1.w as u32, glyphs[i].1.h as u32);
        if w == 0 || h == 0 {
            continue;
        }
        if x + w > width {
            x = 0;
            y += row_h + 1;
            row_h = 0;
        }
        places[i] = (x, y);
        x += w + 1;
        row_h = row_h.max(h);
    }
    let height = (y + row_h).max(1);
    (height <= MAX_SIDE).then_some((width, height, places))
}

/// Parse a character list: comma separated items, each a code point (`65`, `U+41`,
/// `0x41`) or a range of them (`32-126`, `U+20-U+7E`).
pub fn parse_chars(spec: &str) -> Result<Vec<char>, String> {
    fn code(s: &str) -> Result<u32, String> {
        let s = s.trim();
        let hex = s.strip_prefix("U+").or_else(|| s.strip_prefix("u+")).or_else(|| s.strip_prefix("0x"));
        match hex {
            Some(h) => u32::from_str_radix(h, 16),
            None => s.parse(),
        }
        .map_err(|_| format!("not a code point: {s:?}"))
    }
    let mut chars = Vec::new();
    for item in spec.split(',').filter(|i| !i.trim().is_empty()) {
        let (first, last) = match item.split_once('-') {
            Some((a, b)) => (code(a)?, code(b)?),
            None => {
                let c = code(item)?;
                (c, c)
            }
        };
        if first > last || last - first > 0x11_0000 {
            return Err(format!("not a range: {item:?}"));
        }
        chars.extend((first..=last).filter_map(char::from_u32));
    }
    Ok(chars)
}
