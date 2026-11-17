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

pub mod anim;
pub mod arena;
pub mod event;
mod flags;
pub mod font;
pub mod geom;
pub mod id;
#[cfg(feature = "png")]
pub mod image;
pub mod lang;
pub mod layout;
pub mod math;
pub mod msg;
pub mod paint;
pub mod reader;
pub mod render;
pub mod res;
pub mod services;
pub mod skin;
pub mod ui;
pub mod value;
pub mod widget;
pub mod widgets;

pub use arena::{Arena, SideTable, WidgetId};
pub use event::{Event, EventKind, Modifiers, SpecialKey};
pub use font::FontDesc;
pub use geom::{Color, Edges, Point, Rect, Region, Size};
pub use id::TbId;
pub use lang::Language;
pub use layout::{Gravity, LayoutParams, PreferredSize, SizeConstraints, Visibility};
pub use msg::{Message, Millis};
pub use paint::PaintCtx;
pub use reader::{Factory, InflateError};
pub use render::{BitmapId, Quad, Renderer};
pub use res::{ParseError, RefTrees, ResNode};
#[cfg(feature = "std")]
pub use services::FsLoader;
pub use services::{DecodeError, Image, ImageDecoder, LoadError, MemLoader, ResourceLoader, Services};
pub use ui::{Config, Ctx, FocusReason, ListenerId, NodeFlags, NodeRef, Ui, UiListener};
pub use value::{Value, ValueKind};
pub use widget::{Align, Axis, DieAction, HitStatus, ScrollInfo, Widget, WidgetState, Z, ZRel};
