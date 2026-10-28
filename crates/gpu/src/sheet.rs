//! The drawing sheet pass (Plan D-2): a drawing's active sheet as plain 2D data, drawn in the
//! egui render pass the sheet canvas paints into. The paper first, then the views' shaded
//! pictures, then filled shapes (arrowheads, text fills), then strokes, so every line lies over
//! the shading under it.
//!
//! Coordinates are paper inches with y up. The geometry does not depend on the zoom: a stroke's
//! vertices carry their pixel offset across the line, and [`SheetView`] (paper at the canvas
//! centre, pixels per inch, the canvas size) maps paper to clip space in the shader, so panning
//! and zooming only rewrite the uniform. Strokes get a one-pixel fringe whose coverage fades to
//! nothing, so lines are smooth without multisampling.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::view::GpuTarget;

/// What a sheet shows: its paper, and what lies on it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetScene {
    /// x0, y0, x1, y1 in inches; empty (all zero) when there is no sheet.
    pub paper: [f32; 4],
    pub paper_color: [u8; 4],
    pub strokes: Vec<SheetStroke>,
    pub fills: Vec<SheetFill>,
    pub images: Vec<SheetImage>,
}

/// A polyline drawn `width_px` wide whatever the zoom.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetStroke {
    pub points: Vec<[f32; 2]>,
    pub color: [u8; 4],
    pub width_px: f32,
}

/// Filled triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetFill {
    pub triangles: Vec<[[f32; 2]; 3]>,
    pub color: [u8; 4],
}

/// A picture (straight-alpha RGBA rows, the first row at the top) over `rect` (x0, y0, x1, y1
/// in inches).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetImage {
    pub rect: [f32; 4],
    pub rgba: Arc<Vec<u8>>,
    pub size: [u32; 2],
}

/// One draw of the pass after the paper, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetDraw {
    Image,
    Fill,
    Stroke,
}

/// Where the canvas looks: the paper point at its centre, its scale in physical pixels per inch,
/// and its size in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SheetView {
    pub center: [f32; 2],
    pub px_per_in: f32,
    pub size_px: [f32; 2],
}

/// A vertex of the paper, a fill or a stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Vertex {
    /// Paper inches.
    pub pos: [f32; 2],
    /// Pixels added after the map (y up): a stroke's corner pushed across and past its ends.
    pub extrude: [f32; 2],
    pub color: [u8; 4],
    /// Signed pixels from the stroke's centre line, across it.
    pub dist: f32,
    /// Half the stroke's width in pixels ([`SOLID_HALF`] for fills: never faded).
    pub half: f32,
}

/// Bytes per [`Vertex`].
const VERTEX_SIZE: usize = 28;

/// The `half` of a filled vertex: so wide its coverage is always full.
pub(crate) const SOLID_HALF: f32 = 1.0e6;

/// A vertex of a picture's quad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ImageVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
}

const IMAGE_VERTEX_SIZE: usize = 16;

/// The pass's shader: paper to clip space, the stroke fringe's coverage, pictures, and sRGB
/// colours turned linear for an sRGB target.
pub(crate) const SHADER: &str = r"
struct View {
    center: vec2<f32>,
    half_size: vec2<f32>,
    px_per_in: f32,
    linear_out: f32,
    pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> view: View;

fn to_clip(pos: vec2<f32>, extrude: vec2<f32>) -> vec4<f32> {
    let px = (pos - view.center) * view.px_per_in + extrude;
    return vec4<f32>(px / view.half_size, 0.0, 1.0);
}

fn linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn out_colour(c: vec4<f32>) -> vec4<f32> {
    if view.linear_out > 0.5 {
        return vec4<f32>(linear(c.rgb), c.a);
    }
    return c;
}

struct InkOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) dist: f32,
    @location(2) half: f32,
};

@vertex
fn vs_ink(@location(0) pos: vec2<f32>, @location(1) extrude: vec2<f32>, @location(2) color: vec4<f32>, @location(3) dist: f32, @location(4) half: f32) -> InkOut {
    var o: InkOut;
    o.clip = to_clip(pos, extrude);
    o.color = color;
    o.dist = dist;
    o.half = half;
    return o;
}

@fragment
fn fs_ink(i: InkOut) -> @location(0) vec4<f32> {
    let coverage = clamp(i.half + 0.5 - abs(i.dist), 0.0, 1.0);
    return out_colour(vec4<f32>(i.color.rgb, i.color.a * coverage));
}

struct PicOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var picture_sampler: sampler;

@vertex
fn vs_picture(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> PicOut {
    var o: PicOut;
    o.clip = to_clip(pos, vec2<f32>(0.0, 0.0));
    o.uv = uv;
    return o;
}

@fragment
fn fs_picture(i: PicOut) -> @location(0) vec4<f32> {
    return out_colour(textureSample(picture, picture_sampler, i.uv));
}
";

/// The uniform for `view`: centre, half size in pixels, pixels per inch, the sRGB flag, padding.
pub(crate) fn uniform(view: &SheetView, linear_out: bool) -> [f32; 8] {
    [view.center[0], view.center[1], view.size_px[0] / 2.0, view.size_px[1] / 2.0, view.px_per_in, if linear_out { 1.0 } else { 0.0 }, 0.0, 0.0]
}

fn solid(pos: [f32; 2], color: [u8; 4]) -> Vertex {
    Vertex { pos, extrude: [0.0, 0.0], color, dist: 0.0, half: SOLID_HALF }
}

fn finite(p: [f32; 2]) -> bool {
    p[0].is_finite() && p[1].is_finite()
}

/// One segment's quad (two triangles): square caps, pushed `half + 1` px across and past its ends.
fn segment(out: &mut Vec<Vertex>, a: [f32; 2], b: [f32; 2], color: [u8; 4], half: f32) {
    if !(finite(a) && finite(b)) {
        return;
    }
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if !(len > 0.0) || !len.is_finite() {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let e = half + 1.0;
    let (nx, ny) = (-uy * e, ux * e);
    let (tx, ty) = (ux * e, uy * e);
    let corner = |p: [f32; 2], along: f32, side: f32| Vertex {
        pos: p,
        extrude: [tx * along + nx * side, ty * along + ny * side],
        color,
        dist: e * side,
        half,
    };
    let (a0, a1, b0, b1) = (corner(a, -1.0, -1.0), corner(a, -1.0, 1.0), corner(b, 1.0, -1.0), corner(b, 1.0, 1.0));
    out.extend([a0, b0, b1, a0, b1, a1]);
}

/// The paper, the fills and the strokes, in drawing order (one vertex buffer).
pub(crate) fn vertices(scene: &SheetScene) -> Vec<Vertex> {
    let mut v = Vec::new();
    let [x0, y0, x1, y1] = scene.paper;
    if x1 > x0 && y1 > y0 && finite([x0, y0]) && finite([x1, y1]) {
        let c = scene.paper_color;
        v.extend([solid([x0, y0], c), solid([x1, y0], c), solid([x1, y1], c), solid([x0, y0], c), solid([x1, y1], c), solid([x0, y1], c)]);
    }
    for f in &scene.fills {
        for t in &f.triangles {
            if t.iter().all(|p| finite(*p)) {
                v.extend(t.iter().map(|p| solid(*p, f.color)));
            }
        }
    }
    for s in &scene.strokes {
        let half = if s.width_px.is_finite() { s.width_px.clamp(0.5, 64.0) / 2.0 } else { 0.5 };
        for w in s.points.windows(2) {
            segment(&mut v, w[0], w[1], s.color, half);
        }
    }
    v
}

/// A picture's two triangles over `rect`, its first row at the top.
pub(crate) fn image_quad(rect: [f32; 4]) -> [ImageVertex; 6] {
    let [x0, y0, x1, y1] = rect;
    let tl = ImageVertex { pos: [x0, y1], uv: [0.0, 0.0] };
    let tr = ImageVertex { pos: [x1, y1], uv: [1.0, 0.0] };
    let br = ImageVertex { pos: [x1, y0], uv: [1.0, 1.0] };
    let bl = ImageVertex { pos: [x0, y0], uv: [0.0, 1.0] };
    [tl, bl, br, tl, br, tr]
}

fn vertex_bytes(v: &[Vertex]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * VERTEX_SIZE);
    for x in v {
        for f in x.pos.iter().chain(&x.extrude) {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out.extend_from_slice(&x.color);
        out.extend_from_slice(&x.dist.to_le_bytes());
        out.extend_from_slice(&x.half.to_le_bytes());
    }
    out
}

fn image_bytes(v: &[ImageVertex]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * IMAGE_VERTEX_SIZE);
    for x in v {
        for f in x.pos.iter().chain(&x.uv) {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}

/// A picture on the device: the scene's pixels it was made from (kept so their address is not
/// reused), its bind group and its quad.
struct Picture {
    rgba: Arc<Vec<u8>>,
    size: [u32; 2],
    rect: [f32; 4],
    bind: wgpu::BindGroup,
    quad: wgpu::Buffer,
}

/// The sheet's pipelines and what the last scene uploaded.
pub struct SheetPass {
    ink: wgpu::RenderPipeline,
    picture: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    view_bind: wgpu::BindGroup,
    picture_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    linear_out: bool,
    /// The scene the buffers below were made from.
    scene: Option<Arc<SheetScene>>,
    ink_buffer: Option<(wgpu::Buffer, u32)>,
    pictures: Vec<Picture>,
}

impl SheetPass {
    /// What the pass draws after the paper, in order: every picture, every fill, every stroke.
    pub fn draw_list(scene: &SheetScene) -> Vec<SheetDraw> {
        let images = scene.images.iter().map(|_| SheetDraw::Image);
        let fills = scene.fills.iter().map(|_| SheetDraw::Fill);
        let strokes = scene.strokes.iter().map(|_| SheetDraw::Stroke);
        images.chain(fills).chain(strokes).collect()
    }

    /// The pipelines for a pass of `target`'s kind (its colour format, depth buffer and samples:
    /// the egui pass the canvas paints into). Depth is neither tested nor written.
    pub fn new(device: &wgpu::Device, target: GpuTarget) -> SheetPass {
        let module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_sheet"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let view_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_sheet_view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let picture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_sheet_picture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_sheet_view"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_sheet_view"),
            layout: &view_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_sheet_picture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let depth = target.depth.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        });
        let pipeline = |label: &str, vs: &str, fs: &str, layouts: &[Option<&wgpu::BindGroupLayout>], buffer: wgpu::VertexBufferLayout<'_>| {
            let layout =
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: layouts, immediate_size: 0 });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    buffers: &[Some(buffer)],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
                depth_stencil: depth.clone(),
                multisample: wgpu::MultisampleState { count: target.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let ink = pipeline(
            "sc_sheet_ink",
            "vs_ink",
            "fs_ink",
            &[Some(&view_bgl)],
            wgpu::VertexBufferLayout {
                array_stride: VERTEX_SIZE as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Float32, 4 => Float32],
            },
        );
        let picture = pipeline(
            "sc_sheet_picture",
            "vs_picture",
            "fs_picture",
            &[Some(&view_bgl), Some(&picture_bgl)],
            wgpu::VertexBufferLayout {
                array_stride: IMAGE_VERTEX_SIZE as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
            },
        );
        SheetPass {
            ink,
            picture,
            uniform,
            view_bind,
            picture_bgl,
            sampler,
            linear_out: target.format.is_srgb(),
            scene: None,
            ink_buffer: None,
            pictures: Vec::new(),
        }
    }

    /// Upload `scene` when it is not the one drawn last (pictures already on the device are
    /// kept), and set where the canvas looks.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &Arc<SheetScene>, view: SheetView) {
        let words = uniform(&view, self.linear_out);
        let bytes: Vec<u8> = words.iter().flat_map(|f| f.to_le_bytes()).collect();
        queue.write_buffer(&self.uniform, 0, &bytes);
        if self.scene.as_ref().is_some_and(|s| Arc::ptr_eq(s, scene)) {
            return;
        }
        self.scene = Some(scene.clone());
        let v = vertices(scene);
        self.ink_buffer = (!v.is_empty()).then(|| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("sc_sheet_ink"),
                contents: &vertex_bytes(&v),
                usage: wgpu::BufferUsages::VERTEX,
            });
            (buffer, u32::try_from(v.len()).unwrap_or(u32::MAX))
        });
        let mut kept = std::mem::take(&mut self.pictures);
        for img in &scene.images {
            let [w, h] = img.size;
            if w == 0 || h == 0 || img.rgba.len() != (w as usize) * (h as usize) * 4 {
                continue;
            }
            if let Some(i) = kept.iter().position(|p| Arc::ptr_eq(&p.rgba, &img.rgba) && p.size == img.size && p.rect == img.rect) {
                self.pictures.push(kept.swap_remove(i));
                continue;
            }
            let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sc_sheet_picture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &img.rgba,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
                size,
            );
            let texture_view = tex.create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sc_sheet_picture"),
                layout: &self.picture_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            let quad = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("sc_sheet_picture_quad"),
                contents: &image_bytes(&image_quad(img.rect)),
                usage: wgpu::BufferUsages::VERTEX,
            });
            self.pictures.push(Picture { rgba: img.rgba.clone(), size: img.size, rect: img.rect, bind, quad });
        }
    }

    /// Draw what [`SheetPass::prepare`] uploaded into `pass`, whose viewport the caller has set
    /// to the canvas: the paper, the pictures, then the fills and strokes.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'static>) {
        let Some((ink, count)) = &self.ink_buffer else { return };
        let paper = self.scene.as_ref().map_or(0, |s| {
            let [x0, y0, x1, y1] = s.paper;
            if x1 > x0 && y1 > y0 && finite([x0, y0]) && finite([x1, y1]) { 6 } else { 0 }
        });
        pass.set_bind_group(0, &self.view_bind, &[]);
        pass.set_pipeline(&self.ink);
        pass.set_vertex_buffer(0, ink.slice(..));
        pass.draw(0..paper.min(*count), 0..1);
        if !self.pictures.is_empty() {
            pass.set_pipeline(&self.picture);
            for p in &self.pictures {
                pass.set_bind_group(1, &p.bind, &[]);
                pass.set_vertex_buffer(0, p.quad.slice(..));
                pass.draw(0..6, 0..1);
            }
            pass.set_pipeline(&self.ink);
            pass.set_vertex_buffer(0, ink.slice(..));
        }
        if *count > paper {
            pass.draw(paper..*count, 0..1);
        }
    }
}

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;
