//! [`MiniquadRenderer`]: the core's [`Renderer`] on a miniquad `RenderingBackend`.
//!
//! One shader (texel × vertex color, premultiplied), dynamic vertex and index
//! buffers (grown as needed), a scissor rect per clip, blend `(One, OneMinusSrcAlpha)`
//! for color and alpha. Solid fills sample a 1×1 white texture, so every quad goes
//! through the same pipeline.
//!
//! Draws are recorded during the frame and submitted at `end_frame` with one vertex
//! upload (earlier only when a bitmap that pending draws use is updated or
//! destroyed, or a submission is full). Each recorded draw gets its own small index
//! buffer whose indices point at its quads, because miniquad's Metal backend can only
//! draw from index 0 (`draw` asserts `base_element == 0`). Buffers come from pools
//! and each is written at most once per frame: miniquad's Metal backend keeps 30
//! copies of a buffer and runs out if one buffer is updated more often in a frame.
//!
//! TB: renderers/tb_renderer_gl.cpp (TBRendererGL, TBBitmapGL)

use miniquad::{
    Backend, BlendFactor, BlendState, BlendValue, BufferId, BufferLayout, BufferSource, BufferType, BufferUsage,
    Equation, FilterMode, MipmapFilterMode, Pipeline, PipelineParams, RenderingBackend, ShaderError, ShaderId,
    ShaderMeta, ShaderSource, TextureFormat, TextureId, TextureKind, TextureParams, TextureWrap, UniformBlockLayout,
    UniformDesc, UniformType, UniformsSource, VertexAttribute, VertexFormat,
};
use turbo_ferret::geom::{Color, Rect, Size};
use turbo_ferret::render::{BitmapId, Quad, Renderer};

/// Most quads one submission can hold: 16-bit indices address 65536 vertices.
pub const MAX_QUADS: usize = 65536 / 4;

/// Smallest buffer sizes, in quads.
const MIN_VERTEX_QUADS: usize = 1024; // the core painter's batch size
const MIN_INDEX_QUADS: usize = 16;

/// Counters for the last finished frame (or the current one, while it runs).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// `draw` calls on the GPU (one per bitmap or clip change after merging).
    pub draw_calls: u32,
    pub quads: u32,
    /// Vertex buffer uploads (1 per frame unless a bitmap changed mid-frame).
    pub uploads: u32,
}

/// A vertex: position in framebuffer pixels, texture coordinate, premultiplied color.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

/// The vertex shader input, in `Vertex` field order.
const ATTRIBUTES: [VertexAttribute; 3] = [
    VertexAttribute::new("in_pos", VertexFormat::Float2),
    VertexAttribute::new("in_uv", VertexFormat::Float2),
    VertexAttribute::new("in_color", VertexFormat::Byte4),
];

#[repr(C)]
struct Uniforms {
    /// `ndc = pos * transform.xy + transform.zw`
    transform: [f32; 4],
}

/// GLSL 100 runs on GL 2.1+, GLES 2+ and WebGL 1 (miniquad relies on it everywhere).
/// The color attribute arrives as 0..255 (miniquad does not normalize `Byte4`).
const GLSL_VERTEX: &str = r#"#version 100
attribute vec2 in_pos;
attribute vec2 in_uv;
attribute vec4 in_color;
uniform vec4 transform;
varying vec2 uv;
varying vec4 color;
void main() {
    gl_Position = vec4(in_pos * transform.xy + transform.zw, 0.0, 1.0);
    uv = in_uv;
    color = in_color / 255.0;
}
"#;

/// highp where available: mediump cannot address every texel of a large atlas.
const GLSL_FRAGMENT: &str = r#"#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying vec2 uv;
varying vec4 color;
uniform sampler2D tex;
void main() {
    gl_FragColor = texture2D(tex, uv) * color;
}
"#;

/// For miniquad's Metal backend (`AppleGfxApi::Metal`): uniforms at buffer 0, the
/// vertex buffer at 1, entry points named as miniquad expects.
const METAL: &str = r#"#include <metal_stdlib>
using namespace metal;

struct Uniforms { float4 transform; };

struct Vertex {
    float2 pos   [[attribute(0)]];
    float2 uv    [[attribute(1)]];
    uchar4 color [[attribute(2)]];
};

struct Varyings {
    float4 position [[position]];
    float2 uv;
    float4 color;
};

vertex Varyings vertexShader(Vertex v [[stage_in]], constant Uniforms& u [[buffer(0)]]) {
    Varyings out;
    out.position = float4(v.pos * u.transform.xy + u.transform.zw, 0.0, 1.0);
    out.uv = v.uv;
    out.color = float4(v.color) / 255.0;
    return out;
}

fragment float4 fragmentShader(Varyings in [[stage_in]], texture2d<float> tex [[texture(0)]], sampler smp [[sampler(0)]]) {
    return tex.sample(smp, in.uv) * in.color;
}
"#;

fn shader_meta() -> ShaderMeta {
    ShaderMeta {
        uniforms: UniformBlockLayout { uniforms: vec![UniformDesc::new("transform", UniformType::Float4)] },
        images: vec!["tex".to_string()],
    }
}

#[derive(Copy, Clone, Debug)]
struct Texture {
    id: TextureId,
    size: Size,
}

/// A recorded draw: `count` quads starting at quad `first`, one texture, one scissor.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cmd {
    pub bitmap: Option<BitmapId>,
    /// Top-left origin, inside the viewport, not empty.
    pub clip: Rect,
    pub first: u32,
    pub count: u32,
}

/// Vertices and draw commands waiting for submission (no GPU calls).
#[derive(Default)]
pub(crate) struct Batch {
    pub vertices: Vec<Vertex>,
    pub cmds: Vec<Cmd>,
}

impl Batch {
    pub fn quads(&self) -> usize {
        self.vertices.len() / 4
    }

    pub fn is_full(&self) -> bool {
        self.quads() >= MAX_QUADS
    }

    /// True if a pending command draws `bitmap`.
    pub fn uses(&self, bitmap: BitmapId) -> bool {
        self.cmds.iter().any(|c| c.bitmap == Some(bitmap))
    }

    /// Append one quad (the caller checks `is_full` first). Consecutive quads with the
    /// same bitmap and clip share a command. `tex` is the bitmap's size (`None` for
    /// solid fills, which sample texel 0 of the white texture).
    pub fn push(&mut self, bitmap: Option<BitmapId>, clip: Rect, q: &Quad, tex: Option<Size>) {
        let quad = self.quads() as u32;
        match self.cmds.last_mut() {
            Some(c) if c.bitmap == bitmap && c.clip == clip => c.count += 1,
            _ => self.cmds.push(Cmd { bitmap, clip, first: quad, count: 1 }),
        }
        self.vertices.extend_from_slice(&quad_vertices(q, tex));
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
        self.cmds.clear();
    }
}

/// Texture coordinates `[u0, v0, u1, v1]` for the top-left and bottom-right corners of
/// `dst`. Interpolated across the quad, destination pixel `i` samples texel coordinate
/// `src.x + (i + 0.5) * src.w / dst.w` (as [`Quad::src`] documents), so a negative
/// `src.w` or `src.h` (mirroring, with `x`/`y` at the right/bottom edge) needs no
/// special case: `u0 > u1` flips the image.
pub(crate) fn quad_uv(src: Rect, tex: Size) -> [f32; 4] {
    let (tw, th) = (tex.w.max(1) as f32, tex.h.max(1) as f32);
    let (x, y) = (src.x as f32, src.y as f32);
    [x / tw, y / th, (x + src.w as f32) / tw, (y + src.h as f32) / th]
}

/// Four vertices (top-left, top-right, bottom-right, bottom-left) for a quad.
pub(crate) fn quad_vertices(q: &Quad, tex: Option<Size>) -> [Vertex; 4] {
    let (x0, y0) = (q.dst.x as f32, q.dst.y as f32);
    let (x1, y1) = (x0 + q.dst.w as f32, y0 + q.dst.h as f32);
    let [u0, v0, u1, v1] = match tex {
        Some(size) => quad_uv(q.src, size),
        None => [0.0; 4],
    };
    let color = [q.color.r, q.color.g, q.color.b, q.color.a];
    [
        Vertex { pos: [x0, y0], uv: [u0, v0], color },
        Vertex { pos: [x1, y0], uv: [u1, v0], color },
        Vertex { pos: [x1, y1], uv: [u1, v1], color },
        Vertex { pos: [x0, y1], uv: [u0, v1], color },
    ]
}

/// Indices for quads `first .. first + count` of a submission (two triangles each),
/// replacing the contents of `out`. Quads past `MAX_QUADS` are left out.
pub(crate) fn quad_indices(first: u32, count: u32, out: &mut Vec<u16>) {
    out.clear();
    let end = (first as usize + count as usize).min(MAX_QUADS);
    for q in first as usize..end {
        let b = (q * 4) as u16;
        out.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
}

/// Dynamic GPU buffers handed out in order during a frame, so that each is written
/// at most once per frame. Each grows (is re-created larger) when a frame needs it to.
struct BufferPool {
    kind: BufferType,
    min_quads: usize,
    /// Buffer and the quads it holds.
    bufs: Vec<(BufferId, usize)>,
    next: usize,
}

impl BufferPool {
    fn new(kind: BufferType, min_quads: usize) -> BufferPool {
        BufferPool { kind, min_quads, bufs: Vec::new(), next: 0 }
    }

    /// Start handing out from the first buffer again (a new frame).
    fn rewind(&mut self) {
        self.next = 0;
    }

    /// The next buffer of this frame, holding at least `quads` quads.
    fn take(&mut self, ctx: &mut dyn RenderingBackend, quads: usize) -> BufferId {
        let i = self.next;
        self.next += 1;
        if let Some(&(id, cap)) = self.bufs.get(i) {
            if cap >= quads {
                return id;
            }
            // Last used in an earlier frame (each buffer serves once per frame).
            ctx.delete_buffer(id);
        }
        let cap = quads.next_power_of_two().clamp(self.min_quads, MAX_QUADS);
        let source = match self.kind {
            BufferType::VertexBuffer => BufferSource::empty::<Vertex>(cap * 4),
            BufferType::IndexBuffer => BufferSource::empty::<u16>(cap * 6),
        };
        let id = ctx.new_buffer(self.kind, BufferUsage::Stream, source);
        if i < self.bufs.len() {
            self.bufs[i] = (id, cap);
        } else {
            self.bufs.push((id, cap));
        }
        id
    }

    fn delete_all(&mut self, ctx: &mut dyn RenderingBackend) {
        for (id, _) in self.bufs.drain(..) {
            ctx.delete_buffer(id);
        }
        self.next = 0;
    }
}

/// Orthographic projection: framebuffer pixels (top-left origin, y down) to
/// normalized device coordinates.
pub(crate) fn ortho(viewport: Size) -> [f32; 4] {
    if viewport.w <= 0 || viewport.h <= 0 {
        return [0.0; 4];
    }
    [2.0 / viewport.w as f32, -2.0 / viewport.h as f32, -1.0, 1.0]
}

/// `clip` (top-left origin) as miniquad's `apply_scissor_rect` arguments, which use
/// GL's bottom-left origin on every backend (its Metal backend converts). `None` if
/// nothing of `clip` is inside the viewport. TB: TBRendererGL::SetClipRect
pub(crate) fn scissor(clip: Rect, viewport: Size) -> Option<(i32, i32, i32, i32)> {
    let c = clip.clip(&Rect::from_size(viewport));
    if c.is_empty() {
        return None;
    }
    Some((c.x, viewport.h - c.bottom(), c.w, c.h))
}

/// A [`Renderer`] that draws with miniquad (OpenGL, GLES, WebGL, or Metal on Apple
/// platforms).
///
/// It owns the app's `Box<dyn RenderingBackend>`. miniquad 0.4 gives the app one
/// context (from `miniquad::window::new_rendering_backend()`) and no longer passes
/// it to event callbacks, and the renderer's GPU objects (pipeline, buffers, the
/// textures behind every `BitmapId`) only make sense together with that context.
/// Owning it gives one value to keep in the app's `EventHandler` and to hand to
/// `Painter::new`, with no borrow to rebuild each frame; [`context_mut`] lends it
/// back for the app's own drawing and for render passes.
///
/// # Frame protocol
///
/// The app begins and ends the render pass; the renderer draws into the pass that is
/// open when the frame ends:
///
/// ```ignore
/// renderer.context_mut().begin_default_pass(PassAction::clear_color(0.1, 0.1, 0.1, 1.0));
/// {
///     let mut p = Painter::new(&mut renderer, viewport); // begin_frame
///     // ... draw ...
/// } // end_frame: everything is submitted here
/// renderer.context_mut().end_render_pass();
/// renderer.context_mut().commit_frame();
/// ```
///
/// # Conventions
///
/// - **Pixels.** `begin_frame(viewport)` sets an orthographic projection in which one
///   unit is one framebuffer pixel, origin top-left, y down. The viewport should be
///   the framebuffer size (`miniquad::window::screen_size()`), which is what
///   [`InputAdapter`](crate::InputAdapter) gives `Ui::set_viewport`, so the UI works
///   in physical pixels on HiDPI displays.
/// - **Filtering.** Bitmaps sample with linear filtering and clamp-to-edge (as TB's
///   GL renderer does). A 1:1 blit samples texel centers, so it is exact; scaled
///   blits are smooth, and the core atlas's 1 pixel gutter keeps neighbours from
///   bleeding in. [`set_filter`](MiniquadRenderer::set_filter) switches to nearest.
/// - **Color.** Bitmaps and quad colors are premultiplied RGBA8; output is
///   `texel × color`, blended with `(One, OneMinusSrcAlpha)` for color and alpha.
///
/// [`context_mut`]: MiniquadRenderer::context_mut
pub struct MiniquadRenderer {
    ctx: Box<dyn RenderingBackend>,
    shader: ShaderId,
    pipeline: Pipeline,
    /// One vertex buffer per submission in a frame (usually one per frame).
    vertex_buffers: BufferPool,
    /// One index buffer per draw command in a frame.
    index_buffers: BufferPool,
    /// Scratch for index data.
    indices: Vec<u16>,
    white: TextureId,
    /// Slot table: `BitmapId(i)` is `textures[i]`; destroyed slots are reused.
    textures: Vec<Option<Texture>>,
    filter: FilterMode,
    viewport: Size,
    in_frame: bool,
    batch: Batch,
    counting: FrameStats,
    stats: FrameStats,
}

impl MiniquadRenderer {
    /// Create the shader, pipeline, buffers and white texture on `ctx`.
    pub fn new(mut ctx: Box<dyn RenderingBackend>) -> Result<MiniquadRenderer, ShaderError> {
        let source = match ctx.info().backend {
            Backend::OpenGl => ShaderSource::Glsl { vertex: GLSL_VERTEX, fragment: GLSL_FRAGMENT },
            Backend::Metal => ShaderSource::Msl { program: METAL },
        };
        let shader = ctx.new_shader(source, shader_meta())?;
        // Premultiplied alpha: out = src + dst * (1 - src.a), for color and alpha.
        let blend =
            BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::OneMinusValue(BlendValue::SourceAlpha));
        let params = PipelineParams { color_blend: Some(blend), alpha_blend: Some(blend), ..Default::default() };
        let pipeline = ctx.new_pipeline(&[BufferLayout::default()], &ATTRIBUTES, shader, params);
        let filter = FilterMode::Linear;
        let white = ctx.new_texture_from_data_and_format(&[255; 4], texture_params(Size::new(1, 1), filter));
        Ok(MiniquadRenderer {
            ctx,
            shader,
            pipeline,
            vertex_buffers: BufferPool::new(BufferType::VertexBuffer, MIN_VERTEX_QUADS),
            index_buffers: BufferPool::new(BufferType::IndexBuffer, MIN_INDEX_QUADS),
            indices: Vec::new(),
            white,
            textures: Vec::new(),
            filter,
            viewport: Size::ZERO,
            in_frame: false,
            batch: Batch::default(),
            counting: FrameStats::default(),
            stats: FrameStats::default(),
        })
    }

    /// The miniquad context, for the app's own drawing and render passes.
    pub fn context(&self) -> &dyn RenderingBackend {
        &*self.ctx
    }

    /// The miniquad context, for the app's own drawing and render passes.
    pub fn context_mut(&mut self) -> &mut dyn RenderingBackend {
        &mut *self.ctx
    }

    /// Texture filtering for every bitmap, current and future. Default: linear.
    pub fn set_filter(&mut self, filter: FilterMode) {
        self.filter = filter;
        for t in self.textures.iter().flatten() {
            self.ctx.texture_set_filter(t.id, filter, MipmapFilterMode::None);
        }
    }

    pub fn filter(&self) -> FilterMode {
        self.filter
    }

    /// Number of live bitmaps.
    pub fn bitmap_count(&self) -> usize {
        self.textures.iter().flatten().count()
    }

    /// The miniquad texture behind a bitmap.
    pub fn texture(&self, bitmap: BitmapId) -> Option<TextureId> {
        self.tex(bitmap).map(|t| t.id)
    }

    /// Counters of the last finished frame.
    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    fn tex(&self, bitmap: BitmapId) -> Option<Texture> {
        *self.textures.get(bitmap.0 as usize)?
    }

    /// Upload the pending vertices and draw every pending command.
    fn submit(&mut self) {
        let MiniquadRenderer {
            ctx,
            pipeline,
            vertex_buffers,
            index_buffers,
            indices,
            white,
            textures,
            batch,
            counting,
            ..
        } = self;
        let viewport = self.viewport;
        if batch.cmds.is_empty() {
            batch.clear();
            return;
        }
        let ctx = &mut **ctx;
        let vb = vertex_buffers.take(ctx, batch.quads());
        ctx.buffer_update(vb, BufferSource::slice(&batch.vertices));
        ctx.apply_pipeline(pipeline);
        ctx.apply_uniforms(UniformsSource::table(&Uniforms { transform: ortho(viewport) }));
        counting.uploads += 1;
        for c in &batch.cmds {
            let Some((x, y, w, h)) = scissor(c.clip, viewport) else { continue };
            // Commands only reference live bitmaps: update/destroy submit first.
            let tex = match c.bitmap {
                None => *white,
                Some(b) => match textures.get(b.0 as usize).copied().flatten() {
                    Some(t) => t.id,
                    None => continue,
                },
            };
            let ib = index_buffers.take(ctx, c.count as usize);
            quad_indices(c.first, c.count, indices);
            ctx.buffer_update(ib, BufferSource::slice(indices));
            ctx.apply_scissor_rect(x, y, w, h);
            ctx.apply_bindings_from_slice(&[vb], ib, &[tex]);
            ctx.draw(0, indices.len() as i32, 1);
            counting.draw_calls += 1;
            counting.quads += c.count;
        }
        // Leave the scissor open for whatever the app draws next.
        ctx.apply_scissor_rect(0, 0, viewport.w, viewport.h);
        batch.clear();
        if !self.in_frame {
            // Not inside begin/end_frame: the next submission starts a new "frame".
            self.vertex_buffers.rewind();
            self.index_buffers.rewind();
        }
    }
}

fn texture_params(size: Size, filter: FilterMode) -> TextureParams {
    TextureParams {
        kind: TextureKind::Texture2D,
        format: TextureFormat::RGBA8,
        wrap: TextureWrap::Clamp,
        min_filter: filter,
        mag_filter: filter,
        mipmap_filter: MipmapFilterMode::None,
        width: size.w as u32,
        height: size.h as u32,
        allocate_mipmaps: false,
        sample_count: 1,
    }
}

fn byte_len(size: Size) -> Option<usize> {
    if size.w <= 0 || size.h <= 0 {
        return None;
    }
    (size.w as usize).checked_mul(size.h as usize)?.checked_mul(4)
}

impl Renderer for MiniquadRenderer {
    /// TB: TBRendererGL::BeginPaint
    fn begin_frame(&mut self, viewport: Size) {
        self.submit(); // anything drawn outside a frame
        self.viewport = viewport.max(Size::ZERO);
        self.in_frame = true;
        self.counting = FrameStats::default();
        self.vertex_buffers.rewind();
        self.index_buffers.rewind();
    }

    fn end_frame(&mut self) {
        self.submit();
        self.in_frame = false;
        self.stats = self.counting;
    }

    /// TB: TBRendererGL::CreateBitmap, TBBitmapGL::Init
    fn create_bitmap(&mut self, size: Size, rgba: &[u8]) -> Option<BitmapId> {
        let n = byte_len(size)?;
        if rgba.len() < n {
            return None;
        }
        let slot = match self.textures.iter().position(Option::is_none) {
            Some(i) => i,
            None => {
                self.textures.push(None);
                self.textures.len() - 1
            }
        };
        let id = u32::try_from(slot).ok()?;
        let tex = self.ctx.new_texture_from_data_and_format(&rgba[..n], texture_params(size, self.filter));
        self.textures[slot] = Some(Texture { id: tex, size });
        Some(BitmapId(id))
    }

    /// TB: TBBitmapGL::SetData (which also flushes first)
    fn update_bitmap(&mut self, bitmap: BitmapId, rgba: &[u8]) {
        let Some(t) = self.tex(bitmap) else { return };
        let Some(n) = byte_len(t.size) else { return };
        if rgba.len() < n {
            return;
        }
        if self.batch.uses(bitmap) {
            self.submit(); // pending draws must see the old contents (H-22)
        }
        self.ctx.texture_update(t.id, &rgba[..n]);
    }

    /// TB: TBBitmapGL::~TBBitmapGL (which also flushes first)
    fn destroy_bitmap(&mut self, bitmap: BitmapId) {
        let Some(t) = self.tex(bitmap) else { return };
        if self.batch.uses(bitmap) {
            self.submit();
        }
        self.ctx.delete_texture(t.id);
        self.textures[bitmap.0 as usize] = None;
    }

    fn draw(&mut self, bitmap: Option<BitmapId>, clip: Rect, quads: &[Quad]) {
        let tex = match bitmap {
            None => None,
            Some(b) => match self.tex(b) {
                Some(t) => Some(t.size),
                None => return, // unknown bitmap: draw nothing
            },
        };
        let clip = clip.clip(&Rect::from_size(self.viewport));
        if clip.is_empty() {
            return;
        }
        for q in quads {
            // Outside the clip, or a zero color (texel × 0 changes nothing).
            if !q.dst.intersects(&clip) || q.color == Color::TRANSPARENT {
                continue;
            }
            if self.batch.is_full() {
                self.submit();
            }
            self.batch.push(bitmap, clip, q, tex);
        }
        if !self.in_frame {
            self.submit();
        }
    }
}

impl Drop for MiniquadRenderer {
    fn drop(&mut self) {
        for t in self.textures.drain(..).flatten() {
            self.ctx.delete_texture(t.id);
        }
        self.ctx.delete_texture(self.white);
        self.vertex_buffers.delete_all(&mut *self.ctx);
        self.index_buffers.delete_all(&mut *self.ctx);
        self.ctx.delete_pipeline(self.pipeline);
        self.ctx.delete_shader(self.shader);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(dst: Rect, src: Rect) -> Quad {
        Quad { dst, src, color: Color::WHITE }
    }

    /// Texel coordinate sampled at the center of destination pixel `i`, by
    /// interpolating u across the quad as the rasterizer does.
    fn sampled(u0: f32, u1: f32, i: i32, dst_w: i32, tex_w: i32) -> f32 {
        let t = (i as f32 + 0.5) / dst_w as f32;
        (u0 + (u1 - u0) * t) * tex_w as f32
    }

    #[test]
    fn uv_plain() {
        let tex = Size::new(64, 32);
        assert_eq!(quad_uv(Rect::new(8, 4, 16, 8), tex), [0.125, 0.125, 0.375, 0.375]);
        assert_eq!(quad_uv(Rect::new(0, 0, 64, 32), tex), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn uv_mirrored() {
        let tex = Size::new(64, 32);
        // Horizontal: x is the right edge.
        assert_eq!(quad_uv(Rect::new(24, 4, -16, 8), tex), [0.375, 0.125, 0.125, 0.375]);
        // Vertical: y is the bottom edge.
        assert_eq!(quad_uv(Rect::new(8, 12, 16, -8), tex), [0.125, 0.375, 0.375, 0.125]);
        // Both.
        assert_eq!(quad_uv(Rect::new(24, 12, -16, -8), tex), [0.375, 0.375, 0.125, 0.125]);
    }

    #[test]
    fn uv_matches_texel_mapping() {
        // Quad::src: texel of destination pixel i is floor(src.x + (i + 0.5) * src.w / dst.w),
        // the same formula as the software renderer.
        let tex = Size::new(16, 16);
        for (src_x, src_w, dst_w) in [(0, 4, 4), (4, -4, 4), (0, 4, 8), (2, 8, 4), (12, -8, 16), (3, 5, 5)] {
            let [u0, _, u1, _] = quad_uv(Rect::new(src_x, 0, src_w, 1), tex);
            for i in 0..dst_w {
                let got = sampled(u0, u1, i, dst_w, tex.w);
                let want = src_x as f32 + (i as f32 + 0.5) * src_w as f32 / dst_w as f32;
                assert!((got - want).abs() < 1e-4, "src {src_x}+{src_w} dst {dst_w} px {i}: {got} vs {want}");
            }
        }
        // A 1:1 blit samples texel centers exactly, so linear filtering is exact.
        let [u0, _, u1, _] = quad_uv(Rect::new(5, 0, 3, 1), tex);
        assert_eq!((0..3).map(|i| sampled(u0, u1, i, 3, 16)).collect::<Vec<_>>(), [5.5, 6.5, 7.5]);
        // Mirrored 1:1 samples the same centers in reverse.
        let [u0, _, u1, _] = quad_uv(Rect::new(8, 0, -3, 1), tex);
        assert_eq!((0..3).map(|i| sampled(u0, u1, i, 3, 16)).collect::<Vec<_>>(), [7.5, 6.5, 5.5]);
    }

    #[test]
    fn vertices_of_a_quad() {
        let q = Quad { dst: Rect::new(10, 20, 30, 40), src: Rect::new(4, 0, -4, 2), color: Color::rgba(1, 2, 3, 4) };
        let v = quad_vertices(&q, Some(Size::new(8, 4)));
        let pos: Vec<[f32; 2]> = v.iter().map(|v| v.pos).collect();
        assert_eq!(pos, [[10.0, 20.0], [40.0, 20.0], [40.0, 60.0], [10.0, 60.0]]);
        let uv: Vec<[f32; 2]> = v.iter().map(|v| v.uv).collect();
        assert_eq!(uv, [[0.5, 0.0], [0.0, 0.0], [0.0, 0.5], [0.5, 0.5]]);
        assert!(v.iter().all(|v| v.color == [1, 2, 3, 4]));
        // Solid fills sample texel 0 of the white texture.
        assert!(quad_vertices(&q, None).iter().all(|v| v.uv == [0.0, 0.0]));
        assert_eq!(std::mem::size_of::<Vertex>(), 20);
    }

    #[test]
    fn indices() {
        let mut v = vec![9; 3];
        quad_indices(0, 2, &mut v);
        assert_eq!(v, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        // A command's indices point at its own quads: draws always start at index 0.
        quad_indices(3, 1, &mut v);
        assert_eq!(v, [12, 13, 14, 12, 14, 15]);
        quad_indices(0, 0, &mut v);
        assert!(v.is_empty());
        // The last quad of a full submission uses the last 16-bit index.
        quad_indices(MAX_QUADS as u32 - 1, 5, &mut v);
        assert_eq!(v.len(), 6);
        assert_eq!(*v.iter().max().unwrap(), u16::MAX);
    }

    #[test]
    fn projection() {
        let [sx, sy, tx, ty] = ortho(Size::new(200, 100));
        let ndc = |x: f32, y: f32| (x * sx + tx, y * sy + ty);
        assert_eq!(ndc(0.0, 0.0), (-1.0, 1.0)); // top left
        assert_eq!(ndc(200.0, 100.0), (1.0, -1.0)); // bottom right
        assert_eq!(ndc(100.0, 50.0), (0.0, 0.0));
        assert_eq!(ortho(Size::new(0, 100)), [0.0; 4]);
    }

    #[test]
    fn scissor_flips_y_and_clamps() {
        let vp = Size::new(200, 100);
        assert_eq!(scissor(Rect::new(0, 0, 200, 100), vp), Some((0, 0, 200, 100)));
        // Top strip in UI coordinates is the top of GL's bottom-up framebuffer.
        assert_eq!(scissor(Rect::new(10, 0, 50, 20), vp), Some((10, 80, 50, 20)));
        assert_eq!(scissor(Rect::new(10, 80, 50, 20), vp), Some((10, 0, 50, 20)));
        // Clamped to the viewport.
        assert_eq!(scissor(Rect::new(-10, 90, 300, 30), vp), Some((0, 0, 200, 10)));
        assert_eq!(scissor(Rect::new(0, 100, 10, 10), vp), None);
        assert_eq!(scissor(Rect::new(5, 5, 0, 10), vp), None);
    }

    #[test]
    fn batch_merges_and_splits() {
        let mut b = Batch::default();
        let full = Rect::new(0, 0, 100, 100);
        let half = Rect::new(0, 0, 50, 100);
        let q = quad(Rect::new(0, 0, 4, 4), Rect::new(0, 0, 4, 4));
        let t = Some(Size::new(4, 4));
        b.push(None, full, &q, None);
        b.push(None, full, &q, None);
        b.push(Some(BitmapId(1)), full, &q, t);
        b.push(Some(BitmapId(1)), half, &q, t);
        b.push(Some(BitmapId(1)), half, &q, t);
        b.push(None, half, &q, None);
        assert_eq!(
            b.cmds,
            [
                Cmd { bitmap: None, clip: full, first: 0, count: 2 },
                Cmd { bitmap: Some(BitmapId(1)), clip: full, first: 2, count: 1 },
                Cmd { bitmap: Some(BitmapId(1)), clip: half, first: 3, count: 2 },
                Cmd { bitmap: None, clip: half, first: 5, count: 1 },
            ]
        );
        assert_eq!(b.quads(), 6);
        assert_eq!(b.vertices.len(), 24);
        assert!(b.uses(BitmapId(1)));
        assert!(!b.uses(BitmapId(2)));
        assert!(!b.is_full());
        b.clear();
        assert_eq!(b.quads(), 0);
        assert!(b.cmds.is_empty());
        assert!(!b.uses(BitmapId(1)));
    }

    #[test]
    fn batch_full_at_max_quads() {
        let mut b = Batch::default();
        let q = quad(Rect::new(0, 0, 1, 1), Rect::ZERO);
        for _ in 0..MAX_QUADS {
            assert!(!b.is_full());
            b.push(None, Rect::new(0, 0, 1, 1), &q, None);
        }
        assert!(b.is_full());
        assert_eq!(b.cmds.len(), 1);
        assert_eq!(b.cmds[0].count as usize, MAX_QUADS);
    }

    #[test]
    fn byte_lengths() {
        assert_eq!(byte_len(Size::new(3, 2)), Some(24));
        assert_eq!(byte_len(Size::new(0, 2)), None);
        assert_eq!(byte_len(Size::new(-1, 2)), None);
    }
}
