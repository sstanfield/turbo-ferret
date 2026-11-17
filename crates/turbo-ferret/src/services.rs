//! Services: the std-dependent (or host-provided) parts of the engine, behind traits.
//!
//! TB: tb_system.h (TBFile, TBImageLoader, TBDebugOut). Time is not a service: the
//! host calls `ui.set_time()` (port-design.md section 10).

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::geom::Size;

// ---------------------------------------------------------------------------
// Resource loader
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    NotFound(String),
    Io(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotFound(p) => write!(f, "resource not found: {p}"),
            LoadError::Io(e) => write!(f, "resource read error: {e}"),
        }
    }
}

/// Loads bytes for a path (skins, fonts, `@file` includes, layouts).
pub trait ResourceLoader {
    fn load(&mut self, path: &str) -> Result<Cow<'static, [u8]>, LoadError>;

    /// Resolve `rel` against the file `base` (relative to its directory).
    fn resolve(&self, base: &str, rel: &str) -> String {
        resolve_path(base, rel)
    }
}

/// Default path resolution: `rel` relative to the directory of `base`, unless absolute.
pub fn resolve_path(base: &str, rel: &str) -> String {
    if rel.starts_with('/') || base.is_empty() {
        return String::from(rel);
    }
    match base.rfind(['/', '\\']) {
        Some(i) => {
            let mut s = String::from(&base[..=i]);
            s.push_str(rel);
            s
        }
        None => String::from(rel),
    }
}

/// An in-memory table of resources (for example `include_bytes!` entries). `no_std`.
#[derive(Default)]
pub struct MemLoader {
    entries: Vec<(String, Cow<'static, [u8]>)>,
}

impl MemLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a static entry (no copy).
    pub fn with(mut self, path: &str, data: &'static [u8]) -> Self {
        self.add(path, data);
        self
    }

    pub fn add(&mut self, path: &str, data: &'static [u8]) {
        self.insert(path, Cow::Borrowed(data));
    }

    pub fn add_owned(&mut self, path: &str, data: Vec<u8>) {
        self.insert(path, Cow::Owned(data));
    }

    fn insert(&mut self, path: &str, data: Cow<'static, [u8]>) {
        let path = normalize(path);
        if let Some(e) = self.entries.iter_mut().find(|e| e.0 == path) {
            e.1 = data;
        } else {
            self.entries.push((path, data));
        }
    }
}

/// Collapse `./` and `a/../` segments so resolved paths match table keys.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            _ => parts.push(seg),
        }
    }
    let mut s = String::new();
    if path.starts_with('/') {
        s.push('/');
    }
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(p);
    }
    s
}

impl ResourceLoader for MemLoader {
    fn load(&mut self, path: &str) -> Result<Cow<'static, [u8]>, LoadError> {
        let path = normalize(path);
        self.entries.iter().find(|e| e.0 == path).map(|e| e.1.clone()).ok_or(LoadError::NotFound(path))
    }
}

/// Loads files through `std::fs`, relative to a root directory.
#[cfg(feature = "std")]
pub struct FsLoader {
    root: std::path::PathBuf,
}

#[cfg(feature = "std")]
impl FsLoader {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        FsLoader { root: root.into() }
    }
}

#[cfg(feature = "std")]
impl ResourceLoader for FsLoader {
    fn load(&mut self, path: &str) -> Result<Cow<'static, [u8]>, LoadError> {
        let full = self.root.join(path);
        match std::fs::read(&full) {
            Ok(d) => Ok(Cow::Owned(d)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(LoadError::NotFound(String::from(path))),
            Err(e) => Err(LoadError::Io(alloc::format!("{path}: {e}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Image decoder
// ---------------------------------------------------------------------------

/// A decoded image: straight-alpha RGBA8, row-major, `size.w * size.h * 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub size: Size,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Not a format this decoder understands.
    UnknownFormat,
    /// Recognized but uses an unsupported feature (interlace, 16-bit, ...).
    Unsupported(&'static str),
    /// Malformed or truncated data.
    Corrupt(&'static str),
    /// Exceeds the decoder's size limits.
    TooLarge,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnknownFormat => write!(f, "unknown image format"),
            DecodeError::Unsupported(w) => write!(f, "unsupported image: {w}"),
            DecodeError::Corrupt(w) => write!(f, "corrupt image: {w}"),
            DecodeError::TooLarge => write!(f, "image too large"),
        }
    }
}

pub trait ImageDecoder {
    fn decode(&mut self, bytes: &[u8]) -> Result<Image, DecodeError>;
}

/// A decoder that rejects everything (for hosts that provide no images).
pub struct NoImages;
impl ImageDecoder for NoImages {
    fn decode(&mut self, _: &[u8]) -> Result<Image, DecodeError> {
        Err(DecodeError::UnknownFormat)
    }
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

/// The text clipboard, used by edit fields. The default ([`MemClipboard`]) keeps the
/// text in memory; a host with a system clipboard provides its own.
/// TB: TBClipboard
pub trait Clipboard {
    fn set_text(&mut self, text: &str);
    /// The text on the clipboard, if there is any.
    fn text(&mut self) -> Option<String>;
    /// TB: TBClipboard::HasText
    fn has_text(&mut self) -> bool {
        self.text().is_some_and(|t| !t.is_empty())
    }
    /// TB: TBClipboard::Empty
    fn clear(&mut self) {
        self.set_text("");
    }
}

/// A clipboard that lives in this `Ui` only.
#[derive(Default)]
pub struct MemClipboard(String);

impl Clipboard for MemClipboard {
    fn set_text(&mut self, text: &str) {
        self.0 = String::from(text);
    }
    fn text(&mut self) -> Option<String> {
        (!self.0.is_empty()).then(|| self.0.clone())
    }
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

pub type LogSink = fn(Level, fmt::Arguments<'_>);

#[cfg(feature = "std")]
pub fn stderr_log(level: Level, args: fmt::Arguments<'_>) {
    if level >= Level::Info {
        std::eprintln!("[turbo-ferret {level:?}] {args}");
    }
}

// ---------------------------------------------------------------------------
// Services bundle
// ---------------------------------------------------------------------------

pub struct Services {
    pub loader: Box<dyn ResourceLoader>,
    pub images: Box<dyn ImageDecoder>,
    pub clipboard: Box<dyn Clipboard>,
    pub log: Option<LogSink>,
}

impl Services {
    /// Services with the given loader, the default image decoder (if any) and no log.
    pub fn with_loader(loader: impl ResourceLoader + 'static) -> Self {
        Services {
            loader: Box::new(loader),
            images: default_decoder(),
            clipboard: Box::new(MemClipboard::default()),
            log: default_log(),
        }
    }

    pub(crate) fn log(&self, level: Level, args: fmt::Arguments<'_>) {
        if let Some(sink) = self.log {
            sink(level, args);
        }
    }
}

fn default_decoder() -> Box<dyn ImageDecoder> {
    #[cfg(feature = "png")]
    {
        Box::new(<crate::image::PngDecoder as Default>::default())
    }
    #[cfg(not(feature = "png"))]
    {
        Box::new(NoImages)
    }
}

fn default_log() -> Option<LogSink> {
    #[cfg(feature = "std")]
    {
        Some(stderr_log)
    }
    #[cfg(not(feature = "std"))]
    {
        None
    }
}

#[cfg(feature = "std")]
impl Default for Services {
    fn default() -> Self {
        Services::with_loader(FsLoader::new("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve() {
        assert_eq!(resolve_path("skin/skin.tb.txt", "a.png"), "skin/a.png");
        assert_eq!(resolve_path("skin.tb.txt", "a.png"), "a.png");
        assert_eq!(resolve_path("x/y/z.txt", "/abs.png"), "/abs.png");
    }

    #[test]
    fn mem_loader() {
        let mut m = MemLoader::new().with("res/a.txt", b"hello");
        assert_eq!(&*m.load("res/a.txt").unwrap(), b"hello");
        assert_eq!(&*m.load("./res/x/../a.txt").unwrap(), b"hello");
        assert!(matches!(m.load("nope"), Err(LoadError::NotFound(_))));
        m.add_owned("b", alloc::vec![1, 2]);
        assert_eq!(&*m.load("b").unwrap(), &[1, 2]);
    }
}
