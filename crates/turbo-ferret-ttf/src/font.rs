//! Reading a TrueType file: the table directory, the tables a rasterizer needs, the
//! character map and glyph outlines.
//!
//! Every read is bounds checked and a file that does not make sense gives `None`
//! (or an empty glyph), never a panic.
//!
//! The format is described in Apple's TrueType Reference Manual and Microsoft's
//! OpenType specification. TB used stb_truetype (Sean Barrett) or FreeType for this;
//! none of their code is used here. TB: tb_font_renderer_stb.cpp,
//! tb_font_renderer_freetype.cpp

use alloc::vec::Vec;

/// How deep composite glyphs may nest (the specification suggests a limit; fonts
/// in the wild use 1 or 2).
const MAX_COMPONENT_DEPTH: u32 = 8;

/// Most points and components one glyph may have, so a malformed file cannot make
/// a glyph take unbounded time (composites can reference each other many times).
const MAX_GLYPH_POINTS: u32 = 100_000;

/// Receives a glyph outline in font units (y up), contour by contour. Curves are
/// quadratic Béziers.
pub trait OutlineSink {
    fn move_to(&mut self, x: f32, y: f32);
    fn line_to(&mut self, x: f32, y: f32);
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32);
    /// The contour is closed with a line back to its start.
    fn close(&mut self);
}

/// A glyph's bounding box in font units (y up), from the glyph header.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

/// A 2x3 transform: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Copy, Clone, Debug)]
struct Transform {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Transform {
    const IDENTITY: Transform = Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// `self` after `inner`: a point goes through `inner` first.
    fn then(&self, inner: &Transform) -> Transform {
        Transform {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            e: self.a * inner.e + self.c * inner.f + self.e,
            f: self.b * inner.e + self.d * inner.f + self.f,
        }
    }
}

/// A byte range of the file.
#[derive(Copy, Clone, Debug, Default)]
struct Table {
    offset: usize,
    len: usize,
}

/// The character map subtable in use.
#[derive(Copy, Clone, Debug)]
struct Cmap {
    /// Offset of the subtable in the file.
    offset: usize,
    format: u16,
}

/// A parsed TrueType font (outlines in a `glyf` table). Fonts with PostScript
/// outlines (`CFF `), bitmap-only fonts and variable-font variations are not
/// supported: the first two fail to parse, the last renders the default instance.
#[derive(Clone, Debug)]
pub struct Font {
    data: Vec<u8>,
    units_per_em: u16,
    num_glyphs: u16,
    /// `loca` holds 32-bit offsets (else 16-bit, in units of two bytes).
    loca_long: bool,
    ascent: i16,
    descent: i16,
    line_gap: i16,
    /// Number of (advance, bearing) pairs in `hmtx`; later glyphs repeat the last advance.
    num_h_metrics: u16,
    hmtx: Table,
    loca: Table,
    glyf: Table,
    cmap: Option<Cmap>,
}

// Big-endian reads; `None` past the end.
fn u8_at(d: &[u8], o: usize) -> Option<u8> {
    d.get(o).copied()
}
fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    let b = d.get(o..o.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}
fn i16_at(d: &[u8], o: usize) -> Option<i16> {
    u16_at(d, o).map(|v| v as i16)
}
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    let b = d.get(o..o.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Font {
    /// Parse a `.ttf` file (or the first font of a `.ttc` collection). `None` if it
    /// is not a TrueType font with `glyf` outlines or a required table is missing or
    /// malformed.
    pub fn parse(data: Vec<u8>) -> Option<Font> {
        let d = &data[..];
        let mut base = 0usize;
        let mut tag = u32_at(d, 0)?;
        if tag == u32::from_be_bytes(*b"ttcf") {
            // A collection: use its first font.
            base = u32_at(d, 12)? as usize;
            tag = u32_at(d, base)?;
        }
        if base >= d.len() {
            return None;
        }
        // 0x00010000 and 'true' are TrueType outlines. 'OTTO' has CFF outlines.
        if tag != 0x0001_0000 && tag != u32::from_be_bytes(*b"true") {
            return None;
        }
        let num_tables = u16_at(d, base + 4)? as usize;
        let find = |name: &[u8; 4]| -> Option<Table> {
            let want = u32::from_be_bytes(*name);
            for i in 0..num_tables {
                let rec = base + 12 + i * 16;
                if u32_at(d, rec)? == want {
                    let offset = u32_at(d, rec + 8)? as usize;
                    let len = u32_at(d, rec + 12)? as usize;
                    // The table must lie inside the file.
                    d.get(offset..offset.checked_add(len)?)?;
                    return Some(Table { offset, len });
                }
            }
            None
        };

        let head = find(b"head")?;
        let maxp = find(b"maxp")?;
        let hhea = find(b"hhea")?;
        let hmtx = find(b"hmtx")?;
        let loca = find(b"loca")?;
        let glyf = find(b"glyf")?;

        let units_per_em = u16_at(d, head.offset + 18)?;
        if units_per_em == 0 {
            return None;
        }
        let loca_long = match i16_at(d, head.offset + 50)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let num_glyphs = u16_at(d, maxp.offset + 4)?;
        let ascent = i16_at(d, hhea.offset + 4)?;
        let descent = i16_at(d, hhea.offset + 6)?;
        let line_gap = i16_at(d, hhea.offset + 8)?;
        let num_h_metrics = u16_at(d, hhea.offset + 34)?;
        let cmap = find(b"cmap").and_then(|t| pick_cmap(d, t));

        Some(Font {
            units_per_em,
            num_glyphs,
            loca_long,
            ascent,
            descent,
            line_gap,
            num_h_metrics,
            hmtx,
            loca,
            glyf,
            cmap,
            data,
        })
    }

    /// The file this font was parsed from.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Font units per em square (commonly 2048 or 1000).
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// Typographic ascent in font units (positive), from `hhea`.
    pub fn ascent(&self) -> i16 {
        self.ascent
    }

    /// Typographic descent in font units (negative below the baseline), from `hhea`.
    pub fn descent(&self) -> i16 {
        self.descent
    }

    /// Extra space between lines in font units, from `hhea`.
    pub fn line_gap(&self) -> i16 {
        self.line_gap
    }

    /// The glyph for a character, or `None` if the font has none (glyph 0, the
    /// "missing glyph", is never returned).
    pub fn glyph_index(&self, c: char) -> Option<u16> {
        let cmap = self.cmap?;
        let d = &self.data[..];
        let cp = c as u32;
        let o = cmap.offset;
        let glyph = match cmap.format {
            // Byte encoding table: 256 glyph indices.
            0 => {
                if cp >= 256 {
                    return None;
                }
                u16::from(u8_at(d, o + 6 + cp as usize)?)
            }
            // Segment mapping to delta values (the Basic Multilingual Plane).
            4 => {
                if cp > 0xFFFF {
                    return None;
                }
                let cp = cp as u16;
                let seg_count = (u16_at(d, o + 6)? / 2) as usize;
                let end_codes = o + 14;
                let start_codes = end_codes + seg_count * 2 + 2;
                let deltas = start_codes + seg_count * 2;
                let range_offsets = deltas + seg_count * 2;
                // The first segment whose end code is >= cp.
                let (mut lo, mut hi) = (0usize, seg_count);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if u16_at(d, end_codes + mid * 2)? < cp {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                if lo >= seg_count {
                    return None;
                }
                let start = u16_at(d, start_codes + lo * 2)?;
                if cp < start {
                    return None;
                }
                let delta = u16_at(d, deltas + lo * 2)?;
                let range_offset = u16_at(d, range_offsets + lo * 2)? as usize;
                if range_offset == 0 {
                    cp.wrapping_add(delta)
                } else {
                    // The offset is from its own position into the glyph id array.
                    let at = range_offsets + lo * 2 + range_offset + (cp - start) as usize * 2;
                    match u16_at(d, at)? {
                        0 => 0,
                        g => g.wrapping_add(delta),
                    }
                }
            }
            // Trimmed table mapping: a range of glyph indices.
            6 => {
                let first = u32::from(u16_at(d, o + 6)?);
                let count = u32::from(u16_at(d, o + 8)?);
                if cp < first || cp - first >= count {
                    return None;
                }
                u16_at(d, o + 10 + (cp - first) as usize * 2)?
            }
            // Segmented coverage: groups of (start char, end char, start glyph).
            12 => {
                let groups = u32_at(d, o + 12)? as usize;
                let (mut lo, mut hi) = (0usize, groups);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let g = o.checked_add(16)?.checked_add(mid.checked_mul(12)?)?;
                    let (start, end) = (u32_at(d, g)?, u32_at(d, g + 4)?);
                    if cp < start {
                        hi = mid;
                    } else if cp > end {
                        lo = mid + 1;
                    } else {
                        let glyph = u32_at(d, g + 8)?.checked_add(cp - start)?;
                        return u16::try_from(glyph).ok().filter(|&g| g != 0 && g < self.num_glyphs);
                    }
                }
                return None;
            }
            _ => return None,
        };
        (glyph != 0 && glyph < self.num_glyphs).then_some(glyph)
    }

    /// How far the pen moves after `glyph`, in font units.
    pub fn advance_width(&self, glyph: u16) -> u16 {
        if self.num_h_metrics == 0 {
            return 0;
        }
        // Glyphs past the last pair repeat its advance (monospaced fonts use this).
        let i = glyph.min(self.num_h_metrics - 1) as usize;
        self.table_u16(self.hmtx, i * 4).unwrap_or(0)
    }

    fn table_u16(&self, t: Table, o: usize) -> Option<u16> {
        if o.checked_add(2)? > t.len {
            return None;
        }
        u16_at(&self.data, t.offset + o)
    }

    /// The glyph's data in `glyf`, or `None` if it has no outline (a space) or the
    /// file is malformed.
    fn glyph_data(&self, glyph: u16) -> Option<&[u8]> {
        if glyph >= self.num_glyphs {
            return None;
        }
        let g = glyph as usize;
        let (start, end) = if self.loca_long {
            let at = |i: usize| -> Option<usize> {
                if i * 4 + 4 > self.loca.len {
                    return None;
                }
                Some(u32_at(&self.data, self.loca.offset + i * 4)? as usize)
            };
            (at(g)?, at(g + 1)?)
        } else {
            let at = |i: usize| -> Option<usize> { Some(self.table_u16(self.loca, i * 2)? as usize * 2) };
            (at(g)?, at(g + 1)?)
        };
        if end <= start || end > self.glyf.len {
            return None;
        }
        self.data.get(self.glyf.offset + start..self.glyf.offset + end)
    }

    /// The bounding box of the glyph's outline, or `None` if it has none (a space).
    pub fn glyph_bounds(&self, glyph: u16) -> Option<Bounds> {
        let d = self.glyph_data(glyph)?;
        let b = Bounds { x_min: i16_at(d, 2)?, y_min: i16_at(d, 4)?, x_max: i16_at(d, 6)?, y_max: i16_at(d, 8)? };
        (b.x_max > b.x_min && b.y_max > b.y_min).then_some(b)
    }

    /// Send the glyph's outline to `sink`. False if the glyph has no outline or its
    /// data is malformed (what was read before the error has been sent already).
    pub fn outline(&self, glyph: u16, sink: &mut dyn OutlineSink) -> bool {
        let mut budget = MAX_GLYPH_POINTS;
        self.outline_inner(glyph, &Transform::IDENTITY, 0, &mut budget, sink).is_some()
    }

    fn outline_inner(
        &self,
        glyph: u16,
        t: &Transform,
        depth: u32,
        budget: &mut u32,
        sink: &mut dyn OutlineSink,
    ) -> Option<()> {
        let d = self.glyph_data(glyph)?;
        let contours = i16_at(d, 0)?;
        if contours >= 0 {
            simple_outline(d, contours as usize, t, budget, sink)
        } else {
            if depth >= MAX_COMPONENT_DEPTH {
                return None;
            }
            self.composite_outline(d, t, depth, budget, sink)
        }
    }

    /// A glyph made of other glyphs, each with a transform.
    fn composite_outline(
        &self,
        d: &[u8],
        t: &Transform,
        depth: u32,
        budget: &mut u32,
        sink: &mut dyn OutlineSink,
    ) -> Option<()> {
        const ARGS_ARE_WORDS: u16 = 0x0001;
        const ARGS_ARE_XY_VALUES: u16 = 0x0002;
        const HAVE_A_SCALE: u16 = 0x0008;
        const MORE_COMPONENTS: u16 = 0x0020;
        const HAVE_XY_SCALE: u16 = 0x0040;
        const HAVE_TWO_BY_TWO: u16 = 0x0080;
        const SCALED_COMPONENT_OFFSET: u16 = 0x0800;

        let f2dot14 = |o: usize| -> Option<f32> { Some(f32::from(i16_at(d, o)?) / 16384.0) };
        let mut o = 10usize;
        loop {
            *budget = budget.checked_sub(1)?;
            let flags = u16_at(d, o)?;
            let component = u16_at(d, o + 2)?;
            o += 4;
            let (arg1, arg2) = if flags & ARGS_ARE_WORDS != 0 {
                let v = (f32::from(i16_at(d, o)?), f32::from(i16_at(d, o + 2)?));
                o += 4;
                v
            } else {
                let v = (f32::from(u8_at(d, o)? as i8), f32::from(u8_at(d, o + 1)? as i8));
                o += 2;
                v
            };
            // Without this flag the arguments are point numbers to match up. That is
            // rare; such a component is placed without an offset (as stb_truetype does).
            let (mut e, mut f) = if flags & ARGS_ARE_XY_VALUES != 0 { (arg1, arg2) } else { (0.0, 0.0) };
            let (mut a, mut b, mut c, mut dd) = (1.0, 0.0, 0.0, 1.0);
            if flags & HAVE_A_SCALE != 0 {
                a = f2dot14(o)?;
                dd = a;
                o += 2;
            } else if flags & HAVE_XY_SCALE != 0 {
                a = f2dot14(o)?;
                dd = f2dot14(o + 2)?;
                o += 4;
            } else if flags & HAVE_TWO_BY_TWO != 0 {
                a = f2dot14(o)?;
                b = f2dot14(o + 2)?;
                c = f2dot14(o + 4)?;
                dd = f2dot14(o + 6)?;
                o += 8;
            }
            if flags & SCALED_COMPONENT_OFFSET != 0 {
                e *= turbo_ferret::math::sqrt(a * a + b * b);
                f *= turbo_ferret::math::sqrt(c * c + dd * dd);
            }
            let inner = Transform { a, b, c, d: dd, e, f };
            // A component that fails to load is left out; the rest still draw.
            let _ = self.outline_inner(component, &t.then(&inner), depth + 1, budget, sink);
            if flags & MORE_COMPONENTS == 0 {
                return Some(());
            }
        }
    }
}

/// Pick the best character map: full Unicode first, then the Basic Multilingual
/// Plane, then a symbol encoding (whose characters are used as they are).
fn pick_cmap(d: &[u8], t: Table) -> Option<Cmap> {
    let count = u16_at(d, t.offset + 2)? as usize;
    let mut best: Option<(u32, Cmap)> = None;
    for i in 0..count {
        let rec = t.offset + 4 + i * 8;
        let platform = u16_at(d, rec)?;
        let encoding = u16_at(d, rec + 2)?;
        let sub = u32_at(d, rec + 4)? as usize;
        if sub >= t.len {
            continue;
        }
        let offset = t.offset + sub;
        let Some(format) = u16_at(d, offset) else { continue };
        let unicode = match (platform, encoding) {
            (3, 10) | (0, 4) | (0, 6) => 3, // Unicode, full range
            (3, 1) | (0, 0..=3) => 2,       // Unicode, BMP
            (3, 0) => 1,                    // Windows symbol
            _ => 0,                         // Mac Roman and others: not Unicode
        };
        let format_score = match format {
            12 => 3,
            4 => 2,
            6 | 0 => 1,
            _ => 0,
        };
        if unicode == 0 || format_score == 0 {
            continue;
        }
        let score = unicode * 4 + format_score;
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, Cmap { offset, format }));
        }
    }
    best.map(|(_, c)| c)
}

/// One point of a simple glyph.
#[derive(Copy, Clone)]
struct GlyphPoint {
    x: f32,
    y: f32,
    on_curve: bool,
}

/// A glyph of contours of on-curve and off-curve (quadratic control) points.
fn simple_outline(
    d: &[u8],
    contours: usize,
    t: &Transform,
    budget: &mut u32,
    sink: &mut dyn OutlineSink,
) -> Option<()> {
    const ON_CURVE: u8 = 0x01;
    const X_SHORT: u8 = 0x02;
    const Y_SHORT: u8 = 0x04;
    const REPEAT: u8 = 0x08;
    const X_SAME_OR_POSITIVE: u8 = 0x10;
    const Y_SAME_OR_POSITIVE: u8 = 0x20;

    if contours == 0 {
        return Some(());
    }
    let ends = 10usize;
    let num_points = u16_at(d, ends + (contours - 1) * 2)? as usize + 1;
    *budget = budget.checked_sub(num_points as u32)?;
    let instructions = u16_at(d, ends + contours * 2)? as usize;
    let mut o = ends + contours * 2 + 2 + instructions;

    // Flags, with runs.
    let mut flags = Vec::with_capacity(num_points);
    while flags.len() < num_points {
        let f = u8_at(d, o)?;
        o += 1;
        flags.push(f);
        if f & REPEAT != 0 {
            let n = u8_at(d, o)? as usize;
            o += 1;
            for _ in 0..n.min(num_points - flags.len()) {
                flags.push(f);
            }
        }
    }

    // Coordinates are deltas: a byte with a sign flag, a word, or "same as before".
    let mut points = Vec::with_capacity(num_points);
    let mut x = 0i32;
    for &f in &flags {
        if f & X_SHORT != 0 {
            let dx = i32::from(u8_at(d, o)?);
            o += 1;
            x = x.wrapping_add(if f & X_SAME_OR_POSITIVE != 0 { dx } else { -dx });
        } else if f & X_SAME_OR_POSITIVE == 0 {
            x = x.wrapping_add(i32::from(i16_at(d, o)?));
            o += 2;
        }
        points.push(GlyphPoint { x: x as f32, y: 0.0, on_curve: f & ON_CURVE != 0 });
    }
    let mut y = 0i32;
    for (p, &f) in points.iter_mut().zip(&flags) {
        if f & Y_SHORT != 0 {
            let dy = i32::from(u8_at(d, o)?);
            o += 1;
            y = y.wrapping_add(if f & Y_SAME_OR_POSITIVE != 0 { dy } else { -dy });
        } else if f & Y_SAME_OR_POSITIVE == 0 {
            y = y.wrapping_add(i32::from(i16_at(d, o)?));
            o += 2;
        }
        p.y = y as f32;
    }
    for p in &mut points {
        (p.x, p.y) = t.apply(p.x, p.y);
    }

    let mut start = 0usize;
    for c in 0..contours {
        let end = u16_at(d, ends + c * 2)? as usize;
        if end < start || end >= num_points {
            return None;
        }
        emit_contour(&points[start..=end], sink);
        start = end + 1;
    }
    Some(())
}

/// Turn a contour's points into lines and curves. Two off-curve points in a row
/// have an on-curve point halfway between them.
fn emit_contour(pts: &[GlyphPoint], sink: &mut dyn OutlineSink) {
    if pts.len() < 2 {
        return;
    }
    let mid = |a: GlyphPoint, b: GlyphPoint| ((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
    let n = pts.len();
    // Where to begin: an on-curve point if there is one, else a midpoint.
    let (first, begin) = match pts.iter().position(|p| p.on_curve) {
        Some(i) => (i, (pts[i].x, pts[i].y)),
        None => (0, mid(pts[n - 1], pts[0])),
    };
    let all_off = !pts[first].on_curve;
    sink.move_to(begin.0, begin.1);
    // The control point waiting for its end point.
    let mut ctrl: Option<GlyphPoint> = None;
    // Walk the other points in order, starting after `first` (or at it if every
    // point is off-curve, since the start is then a midpoint).
    let count = if all_off { n } else { n - 1 };
    for k in 0..count {
        let p = pts[(first + if all_off { 0 } else { 1 } + k) % n];
        match (ctrl, p.on_curve) {
            (None, true) => sink.line_to(p.x, p.y),
            (None, false) => ctrl = Some(p),
            (Some(c), true) => {
                sink.quad_to(c.x, c.y, p.x, p.y);
                ctrl = None;
            }
            (Some(c), false) => {
                let m = mid(c, p);
                sink.quad_to(c.x, c.y, m.0, m.1);
                ctrl = Some(p);
            }
        }
    }
    if let Some(c) = ctrl {
        sink.quad_to(c.x, c.y, begin.0, begin.1);
    }
    sink.close();
}
