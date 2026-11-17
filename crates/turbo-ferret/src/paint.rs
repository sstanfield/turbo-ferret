//! The paint context handed to `Widget::paint`. Filled in at M3.

use core::marker::PhantomData;

/// Read-only access to the tree plus drawing helpers during paint.
pub struct PaintCtx<'a> {
    _p: PhantomData<&'a ()>,
}
