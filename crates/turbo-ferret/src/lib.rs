//! # Turbo Ferret
//!
//! Turbo Ferret is a Rust port of Turbo Badger by Emil Segerås
//! (<https://github.com/fruxo/turbobadger>). It is not the original software, and it
//! is not endorsed by its author.
//!
//! A small, host-driven UI toolkit for games: the host owns the loop, feeds input and
//! time, and calls `update` and `paint` with a renderer. The core is `no_std` + `alloc`
//! with zero dependencies; std services sit behind traits with std defaults.
//!
//! Turbo Badger's zlib notice ships in the `NOTICE` file of every crate.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod arena;
pub mod geom;
pub mod id;
#[cfg(feature = "png")]
pub mod image;
pub mod lang;
pub mod math;
pub mod render;
pub mod res;
pub mod services;
pub mod value;

pub use arena::{Arena, SideTable, WidgetId};
pub use geom::{Color, Edges, Point, Rect, Region, Size};
pub use id::TbId;
pub use render::{BitmapId, Quad, Renderer};
#[cfg(feature = "std")]
pub use services::FsLoader;
pub use services::{DecodeError, Image, ImageDecoder, LoadError, MemLoader, ResourceLoader, Services};
pub use value::{Value, ValueKind};
