//! Bitmap fonts in Turbo Badger's own format: one strip per size, glyphs side by
//! side with clear columns between them, drawn with a look that is baked in (a
//! shadow, a glow, a color gradient).
//!
//! These take the place of the bitmap fonts Turbo Badger ships, which its author
//! made by hand from fonts whose licences he could not vouch for. The ones made
//! here come from an openly licensed TrueType font.
//!
//! How a strip keeps the font's metrics, although the format knows only "a glyph is
//! as wide as its ink, plus a constant": every glyph gets a cell as wide as its
//! advance (plus a margin for the effect), with its ink at its bearing inside, and
//! a row of pixels of alpha 1 (invisible) ties the cell together, so the loader
//! finds the whole cell as the glyph.

use std::fmt::Write as _;
use std::rc::Rc;

use turbo_ferret::font::{GlyphImage, GlyphSource};
use turbo_ferret_ttf::{Font, TtfFace};

use crate::Error;

/// The look of a strip font.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripStyle {
    /// The glyphs' color at the top and at the bottom of the line (RGB).
    pub fill_top: [u8; 3],
    pub fill_bottom: [u8; 3],
    /// The color behind the glyphs (a shadow or a glow), RGB.
    pub halo: [u8; 3],
    /// How far the halo is blurred, in pixels.
    pub halo_radius: u8,
    /// How strong the halo is: its coverage is multiplied by this.
    pub halo_strength: f32,
    /// The halo's offset (a drop shadow is moved down).
    pub halo_offset: (i32, i32),
    /// A color font is drawn as it is. Otherwise it is drawn in the text color
    /// (white glyphs take it; a black halo stays black).
    pub color: bool,
    /// The font size the halo's radius and offset are meant for: at other sizes
    /// they are scaled with the size (0: the same at every size).
    pub halo_at_size: u16,
}

impl StripStyle {
    /// White with a soft dark shadow below, in the text color: the look of Turbo
    /// Badger's default font.
    pub const SHADOW: StripStyle = StripStyle {
        fill_top: [255, 255, 255],
        fill_bottom: [255, 255, 255],
        halo: [0, 0, 0],
        halo_radius: 1,
        halo_strength: 0.9,
        halo_offset: (0, 1),
        color: false,
        halo_at_size: 14,
    };
    /// White letters in a wide blue glow.
    pub const GLOW: StripStyle = StripStyle {
        fill_top: [255, 255, 255],
        fill_bottom: [190, 235, 255],
        halo: [0, 140, 255],
        halo_radius: 5,
        halo_strength: 2.2,
        halo_offset: (0, 0),
        color: true,
        halo_at_size: 0,
    };
    /// Yellow to red, with a dark outline.
    pub const SUNSET: StripStyle = StripStyle {
        fill_top: [255, 236, 120],
        fill_bottom: [240, 70, 30],
        halo: [60, 10, 0],
        halo_radius: 2,
        halo_strength: 3.0,
        halo_offset: (0, 1),
        color: true,
        halo_at_size: 0,
    };
    /// Pink to violet, with a soft purple shadow.
    pub const CANDY: StripStyle = StripStyle {
        fill_top: [255, 200, 235],
        fill_bottom: [170, 90, 255],
        halo: [40, 0, 90],
        halo_radius: 3,
        halo_strength: 2.0,
        halo_offset: (2, 3),
        color: true,
        halo_at_size: 0,
    };

    /// A style by its name on the command line.
    pub fn by_name(name: &str) -> Option<StripStyle> {
        match name {
            "shadow" => Some(StripStyle::SHADOW),
            "glow" => Some(StripStyle::GLOW),
            "sunset" => Some(StripStyle::SUNSET),
            "candy" => Some(StripStyle::CANDY),
            _ => None,
        }
    }

    /// The style for one font size (see `halo_at_size`).
    fn at_size(&self, size: u16) -> StripStyle {
        if self.halo_at_size == 0 {
            return *self;
        }
        let scale = |v: i32| (v * i32::from(size) + i32::from(self.halo_at_size) / 2) / i32::from(self.halo_at_size);
        StripStyle {
            halo_radius: scale(i32::from(self.halo_radius)).clamp(0, 64) as u8,
            halo_offset: (scale(self.halo_offset.0), scale(self.halo_offset.1)),
            ..*self
        }
    }

    /// The room the halo needs around a glyph.
    fn pad(&self) -> i32 {
        i32::from(self.halo_radius) + self.halo_offset.0.abs().max(self.halo_offset.1.abs())
    }
}

/// One generated strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Strip {
    pub size: u16,
    pub file_name: String,
    pub width: u32,
    pub height: u32,
    /// Straight-alpha RGBA8, row by row.
    pub rgba: Vec<u8>,
}

impl Strip {
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Best);
        // Writing into a Vec cannot fail, and the size matches the data.
        let mut writer = enc.write_header().expect("png header");
        writer.write_image_data(&self.rgba).expect("png data");
        drop(writer);
        out
    }
}

/// A generated strip font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripOutput {
    /// The contents of `<stem>.tb.txt`.
    pub description: String,
    pub strips: Vec<Strip>,
    /// Characters that were asked for and are not in the font.
    pub missing: Vec<char>,
}

impl StripOutput {
    /// The files to write: name and contents.
    pub fn files(&self, stem: &str) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![(format!("{stem}.tb.txt"), self.description.clone().into_bytes())];
        files.extend(self.strips.iter().map(|s| (s.file_name.clone(), s.to_png())));
        files
    }
}

/// The widest a strip may be (the core's PNG decoder takes images up to this).
const MAX_STRIP_WIDTH: u32 = 16_384;

/// Rasterize `ttf` at each size into a strip in the given style. The space is not
/// drawn (the format gives it as a width); other characters without ink get an
/// empty cell.
pub fn generate_strip(ttf: &[u8], opts: &crate::Options, style: &StripStyle) -> Result<StripOutput, Error> {
    let font = Rc::new(Font::parse(ttf.to_vec()).ok_or(Error::NotAFont)?);
    let mut chars = opts.chars.clone();
    chars.sort();
    chars.dedup();
    chars.retain(|&c| c != ' ');
    let missing: Vec<char> = chars.iter().copied().filter(|&c| font.glyph_index(c).is_none()).collect();
    chars.retain(|c| !missing.contains(c));
    let mut sizes = opts.sizes.clone();
    sizes.sort();
    sizes.dedup();
    if chars.is_empty() || sizes.is_empty() {
        return Err(Error::NothingToDo);
    }
    if !crate::stem_reads_back(&opts.stem) {
        return Err(Error::BadStem(opts.stem.clone()));
    }

    let glyph_str: String = chars.iter().collect();
    let mut description = String::from("info\n\tgenerator turbo-ferret-fontgen\n");
    let _ = writeln!(description, "\tglyph_str {glyph_str}");
    if style.color {
        description.push_str("\trgb 1\n");
    }
    let mut strips = Vec::new();
    for &size in &sizes {
        let style = &style.at_size(size);
        let pad = style.pad();
        let mut face = TtfFace::new(font.clone(), size);
        let metrics = face.metrics();
        let mut glyphs = Vec::new();
        for &c in &chars {
            let m = face.glyph_metrics(c).unwrap_or_default();
            let mut img = GlyphImage::new();
            let data = if face.rasterize(c, &mut img) { img.data } else { Vec::new() };
            glyphs.push((m, data));
        }
        // Ink that reaches left of the pen or right of the advance needs room in
        // every cell (the format has one offset and one width correction).
        let left = glyphs.iter().map(|(m, _)| -m.x).max().unwrap_or(0).max(0);
        let right = glyphs.iter().map(|(m, _)| m.x + m.w - m.advance).max().unwrap_or(0).max(0);
        let height = metrics.ascent + metrics.descent + 2 * pad;
        let baseline = pad + metrics.ascent;
        let cell_w = |advance: i32| left + advance.max(1) + right + 2 * pad;
        let width: i64 = glyphs.iter().map(|(m, _)| i64::from(cell_w(m.advance)) + 2).sum();
        if width > i64::from(MAX_STRIP_WIDTH) || height <= 0 {
            return Err(Error::TooLarge(size));
        }
        let (width, height) = (width as usize, height as usize);
        let mut rgba = vec![0u8; width * height * 4];
        let mut x0 = 0usize;
        for (m, data) in &glyphs {
            let cw = cell_w(m.advance) as usize;
            let cell = render_cell(style, cw, height, (left + pad + m.x, baseline + m.y), (m.w, m.h), data);
            for y in 0..height {
                let dst = (y * width + x0) * 4;
                rgba[dst..dst + cw * 4].copy_from_slice(&cell[y * cw * 4..(y + 1) * cw * 4]);
            }
            // Tie the cell together for the loader: every column has a pixel.
            let row = (baseline as usize).min(height) - 1;
            for x in x0..x0 + cw {
                let p = (row * width + x) * 4;
                if rgba[p + 3] == 0 {
                    rgba[p..p + 4].copy_from_slice(&[0, 0, 0, 1]);
                }
            }
            x0 += cw + 2;
        }

        let file_name = format!("{}_{size}.png", opts.stem);
        let space = face.glyph_metrics(' ').map_or(i32::from(size) / 3, |m| m.advance);
        let _ = writeln!(description, "size {size}");
        let _ = writeln!(description, "\tbitmap {file_name}");
        let _ = writeln!(description, "\tascent {}", metrics.ascent + pad);
        let _ = writeln!(description, "\tdescent {}", metrics.descent + pad);
        let _ = writeln!(description, "\tx_ofs {}", -(left + pad));
        let _ = writeln!(description, "\tadvance_delta {}", -(left + right + 2 * pad));
        let _ = writeln!(description, "\tspace_advance {space}");
        strips.push(Strip { size, file_name, width: width as u32, height: height as u32, rgba });
    }
    Ok(StripOutput { description, strips, missing })
}

/// One glyph's cell: the halo, then the glyph over it. `at` is where the glyph's
/// image goes in the cell, `size` its size, `alpha` its coverage.
fn render_cell(style: &StripStyle, cw: usize, ch: usize, at: (i32, i32), size: (i32, i32), alpha: &[u8]) -> Vec<u8> {
    let mut cover = vec![0f32; cw * ch];
    for gy in 0..size.1 {
        for gx in 0..size.0 {
            let (x, y) = (at.0 + gx, at.1 + gy);
            if x >= 0 && y >= 0 && (x as usize) < cw && (y as usize) < ch {
                cover[y as usize * cw + x as usize] = f32::from(alpha[(gy * size.0 + gx) as usize]) / 255.0;
            }
        }
    }
    // The halo: the coverage, moved and blurred.
    let mut halo = vec![0f32; cw * ch];
    for y in 0..ch as i32 {
        for x in 0..cw as i32 {
            let (sx, sy) = (x - style.halo_offset.0, y - style.halo_offset.1);
            if sx >= 0 && sy >= 0 && (sx as usize) < cw && (sy as usize) < ch {
                halo[y as usize * cw + x as usize] = cover[sy as usize * cw + sx as usize];
            }
        }
    }
    blur(&mut halo, cw, ch, usize::from(style.halo_radius));

    let mut out = vec![0u8; cw * ch * 4];
    for y in 0..ch {
        // The fill runs from its top color to its bottom color down the line.
        let t = if ch > 1 { y as f32 / (ch - 1) as f32 } else { 0.0 };
        let fill: [f32; 3] =
            core::array::from_fn(|i| f32::from(style.fill_top[i]) * (1.0 - t) + f32::from(style.fill_bottom[i]) * t);
        for x in 0..cw {
            let a = cover[y * cw + x];
            let h = (halo[y * cw + x] * style.halo_strength).min(1.0) * (1.0 - a);
            let alpha_out = a + h;
            if alpha_out <= 0.0 {
                continue;
            }
            let p = (y * cw + x) * 4;
            for i in 0..3 {
                let c = (fill[i] * a + f32::from(style.halo[i]) * h) / alpha_out;
                out[p + i] = (c + 0.5) as u8;
            }
            out[p + 3] = (alpha_out * 255.0 + 0.5) as u8;
        }
    }
    out
}

/// A Gaussian blur of deviation `radius / 2`, in two passes.
fn blur(v: &mut [f32], w: usize, h: usize, radius: usize) {
    if radius == 0 {
        return;
    }
    let dev = radius as f32 / 2.0;
    let kernel: Vec<f32> = (0..=2 * radius)
        .map(|k| {
            let x = k as f32 - radius as f32;
            (-(x * x) / (2.0 * dev * dev)).exp()
        })
        .collect();
    let sum: f32 = kernel.iter().sum();
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (k, weight) in kernel.iter().enumerate() {
                if let Some(sx) = (x + k).checked_sub(radius).filter(|&sx| sx < w) {
                    acc += v[y * w + sx] * weight;
                }
            }
            tmp[y * w + x] = acc / sum;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (k, weight) in kernel.iter().enumerate() {
                if let Some(sy) = (y + k).checked_sub(radius).filter(|&sy| sy < h) {
                    acc += tmp[sy * w + x] * weight;
                }
            }
            v[y * w + x] = acc / sum;
        }
    }
}
