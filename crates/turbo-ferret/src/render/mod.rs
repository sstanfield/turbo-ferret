//! Rendering: the narrow renderer interface, the painter (batching) and atlases.

pub mod atlas;
pub mod painter;
pub mod renderer;

pub use atlas::{Atlas, FragmentId};
pub use painter::Painter;
pub use renderer::{BitmapId, Quad, Renderer};
