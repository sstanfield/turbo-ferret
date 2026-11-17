//! `turbo-ferret-fontgen`: rasterize a TrueType font into a Turbo Ferret bitmap font.

use std::path::PathBuf;
use std::process::ExitCode;

use turbo_ferret_fontgen::{Options, StripStyle, generate, generate_strip, parse_chars};

const USAGE: &str = "\
usage: turbo-ferret-fontgen <font.ttf> --sizes <n,n,...> --out <dir/name> [options]

Writes <dir/name>.tb.txt and <dir/name>_<size>.png for each size. Register the
.tb.txt file with the font manager like any bitmap font.

  --sizes 14,28          em sizes in pixels (one bitmap each)
  --out dir/name         where to write, without extension
  --chars 32-126,U+A0-U+FF
                         code points and ranges to include
                         (default: Windows-1252: ASCII, Latin-1, and the
                         bullet, euro sign, dashes and curly quotes)
  --text file.txt        also include every character of this UTF-8 file
  --strip <style>        write Turbo Badger's strip format instead, one strip per
                         size with a look baked in: shadow (white, in the text
                         color), glow, sunset, candy (color fonts)
";

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut font, mut sizes, mut out, mut chars, mut text) = (None, None, None, None, Vec::new());
    let mut strip = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--sizes" => sizes = Some(value("--sizes")?),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--chars" => chars = Some(value("--chars")?),
            "--text" => text.push(PathBuf::from(value("--text")?)),
            "--strip" => {
                let name = value("--strip")?;
                strip = Some(StripStyle::by_name(&name).ok_or(format!("unknown style {name:?}"))?);
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}")),
            _ if font.is_none() => font = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    let font = font.ok_or("no font file given")?;
    let out = out.ok_or("no --out given")?;
    let sizes: Vec<u16> = sizes
        .ok_or("no --sizes given")?
        .split(',')
        .map(|s| s.trim().parse::<u16>().ok().filter(|&n| n > 0).ok_or(format!("not a size: {s:?}")))
        .collect::<Result<_, _>>()?;
    let mut chars = match chars {
        Some(spec) => parse_chars(&spec)?,
        None if text.is_empty() => Options::default_chars(),
        None => Vec::new(),
    };
    for path in &text {
        let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        chars.extend(s.chars().filter(|c| !c.is_control()));
    }
    let stem = out.file_name().and_then(|n| n.to_str()).ok_or("--out needs a file name")?.to_string();
    let dir = out.parent().map(PathBuf::from).unwrap_or_default();

    let ttf = std::fs::read(&font).map_err(|e| format!("{}: {e}", font.display()))?;
    let opts = Options { stem: stem.clone(), sizes, chars };
    let (files, missing) = match strip {
        Some(style) => {
            let output = generate_strip(&ttf, &opts, &style).map_err(|e| e.to_string())?;
            (output.files(&stem), output.missing)
        }
        None => {
            let output = generate(&ttf, &opts).map_err(|e| e.to_string())?;
            (output.files(&stem), output.missing)
        }
    };
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    for (name, data) in files {
        let path = dir.join(&name);
        std::fs::write(&path, &data).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{} ({} bytes)", path.display(), data.len());
    }
    if !missing.is_empty() {
        let list: Vec<String> = missing.iter().map(|&c| format!("U+{:04X}", c as u32)).collect();
        eprintln!("not in the font, left out: {}", list.join(" "));
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("turbo-ferret-fontgen: {e}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
