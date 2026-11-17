//! Golden-image harness for tests that render with `SoftRenderer`.
//!
//! Goldens live in `<workspace>/tests/golden/<name>.png` and hold the framebuffer
//! bytes verbatim (premultiplied RGBA8). Tests clear to an opaque background, so the
//! stored images are ordinary opaque PNGs.
//!
//! - `BLESS=1 cargo test -p turbo-ferret-soft` (re)writes the goldens.
//! - On a mismatch or a missing golden, `<name>.actual.png` is written next to it
//!   (gitignored) and the test panics with the number of differing pixels.

#![allow(dead_code)] // each test binary uses a different subset

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use turbo_ferret_soft::SoftRenderer;

pub fn golden_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(2).expect("workspace root").join("tests").join("golden")
}

fn blessing() -> bool {
    std::env::var_os("BLESS").is_some_and(|v| v == "1")
}

/// Compare the framebuffer with `tests/golden/<name>.png`.
pub fn assert_golden(name: &str, r: &SoftRenderer) {
    let dir = golden_dir();
    let path = dir.join(format!("{name}.png"));
    let actual = dir.join(format!("{name}.actual.png"));
    let (w, h) = (r.size().w as u32, r.size().h as u32);

    if blessing() {
        fs::create_dir_all(&dir).expect("create golden dir");
        write_png(&path, w, h, r.pixels());
        let _ = fs::remove_file(&actual);
        return;
    }

    let problem = match read_png(&path) {
        Err(e) => format!("golden {} missing or unreadable: {e}", path.display()),
        Ok((gw, gh, _)) if (gw, gh) != (w, h) => {
            format!("golden {} is {gw}x{gh}, framebuffer is {w}x{h}", path.display())
        }
        Ok((_, _, golden)) => {
            let differing = golden.chunks_exact(4).zip(r.pixels().chunks_exact(4)).filter(|(a, b)| a != b).count();
            if differing == 0 {
                let _ = fs::remove_file(&actual);
                return;
            }
            format!("{differing} of {} pixels differ from golden {}", w * h, path.display())
        }
    };
    fs::create_dir_all(&dir).expect("create golden dir");
    write_png(&actual, w, h, r.pixels());
    panic!("{problem}\n  actual written to {}\n  run with BLESS=1 to accept", actual.display());
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let file = File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    let mut enc = png::Encoder::new(BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(rgba).expect("png data");
    writer.finish().expect("png finish");
}

/// Read an RGBA8 PNG (other formats are reported as errors).
fn read_png(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut reader = png::Decoder::new(file).read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(format!("expected RGBA8, found {:?} {:?}", info.color_type, info.bit_depth));
    }
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}
