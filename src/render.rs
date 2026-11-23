//! wgpu renderer: sky, terrain, block outline, and a batched UI layer.

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use image::RgbaImage;
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::assets::Assets;
use crate::blocks::Atlas;
use crate::config::Config;
use crate::mesh::{MeshData, Vertex};
use crate::ui::{DrawList, Tex, UiVertex};
use crate::world::{BLOCK, CW};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const DYN_VERTS: usize = 1024;
const DYN_INDICES: usize = 2048;

pub struct Camera {
    pub eye: Vec3,
    pub forward: Vec3,
}

/// Everything needed to draw one frame. Without a camera only the UI is drawn (menus).
pub struct Scene<'a> {
    pub camera: Option<&'a Camera>,
    /// World-space box (min, max) to outline, e.g. the targeted block.
    pub highlight: Option<(Vec3, Vec3)>,
    /// A small mesh rebuilt every frame (the player model), drawn with the terrain pipeline.
    pub dynamic: Option<&'a MeshData>,
    pub ui: &'a DrawList,
    pub ui_scale: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    fog_color: [f32; 4],
    sky_top: [f32; 4],
    sky_horizon: [f32; 4],
    params: [f32; 4],
}

struct GpuMesh {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    count: u32,
    center: Vec3,
}

struct UiTexture {
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth: wgpu::TextureView,

    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    atlas_bg: wgpu::BindGroup,
    sky_bg: wgpu::BindGroup,
    sky_has_tex: bool,

    chunk_pipe: wgpu::RenderPipeline,
    sky_pipe: wgpu::RenderPipeline,
    line_pipe: wgpu::RenderPipeline,
    ui_pipe: wgpu::RenderPipeline,

    meshes: HashMap<(i32, i32), Option<GpuMesh>>,
    ui_tex: HashMap<Tex, UiTexture>,
    ui_vbuf: wgpu::Buffer,
    ui_cap: usize,
    line_vbuf: wgpu::Buffer,
    dyn_vb: wgpu::Buffer,
    dyn_ib: wgpu::Buffer,

    fov: f32,
    fog_start_frac: f32,
    fog_end_frac: f32,
    fog_start: f32,
    fog_end: f32,
    fog_color: [f32; 3],
    sky_top: [f32; 3],
    sky_horizon: [f32; 3],
}

fn make_texture(device: &wgpu::Device, queue: &wgpu::Queue, img: &RgbaImage, label: &str) -> wgpu::TextureView {
    let tex = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: img.width(), height: img.height(), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        img.as_raw(),
    );
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn make_depth(device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn load_png(assets: &Assets, rel: &str) -> Option<RgbaImage> {
    let path = assets.resolve(rel)?;
    match image::open(&path) {
        Ok(i) => Some(i.to_rgba8()),
        Err(e) => {
            eprintln!("warning: cannot read {}: {e}", path.display());
            None
        }
    }
}

fn ui_buffer(device: &wgpu::Device, vertices: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ui vertices"),
        size: (vertices * std::mem::size_of::<UiVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Renderer {
    pub async fn new(window: Arc<Window>, assets: &Assets, cfg: &Config, atlas: &Atlas) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).context("cannot create a window surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("no compatible GPU found")?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .context("cannot open the GPU")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let depth = make_depth(&device, config.width, config.height);

        // ----- bind group layouts -----
        let globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
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

        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sky_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let tex_bg = |view: &wgpu::TextureView, sampler: &wgpu::Sampler, label: &str| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &tex_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            })
        };

        // ----- textures -----
        let atlas_view = make_texture(&device, &queue, &atlas.image, "atlas");
        let atlas_bg = tex_bg(&atlas_view, &nearest, "atlas bg");

        let sky_img = load_png(assets, "textures/sky/sky.png");
        let sky_has_tex = sky_img.is_some();
        let sky_img = sky_img.unwrap_or_else(|| RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255])));
        let sky_view = make_texture(&device, &queue, &sky_img, "sky");
        let sky_bg = tex_bg(&sky_view, &sky_sampler, "sky bg");

        // UI textures. White is always present; the rest are optional PNGs.
        let mut ui_tex = HashMap::new();
        let white = RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        let white_view = make_texture(&device, &queue, &white, "ui white");
        ui_tex.insert(Tex::White, UiTexture { bind_group: tex_bg(&white_view, &nearest, "ui white bg"), size: (1, 1) });
        ui_tex.insert(Tex::Atlas, UiTexture { bind_group: tex_bg(&atlas_view, &nearest, "ui atlas bg"), size: atlas.image.dimensions() });
        for (tex, rel, sampler) in [
            (Tex::Font, "textures/ui/font.png", &nearest),
            (Tex::Banner, "textures/ui/banner.png", &linear),
            (Tex::Crosshair, "textures/ui/crosshair.png", &nearest),
        ] {
            match load_png(assets, rel) {
                Some(img) => {
                    let view = make_texture(&device, &queue, &img, rel);
                    ui_tex.insert(tex, UiTexture { bind_group: tex_bg(&view, sampler, rel), size: img.dimensions() });
                }
                None if tex == Tex::Font => eprintln!("warning: {rel} is missing, text will not be visible"),
                None => {}
            }
        }

        // ----- globals -----
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals bg"),
            layout: &globals_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }],
        });

        // ----- pipelines -----
        let shader = |label: &str, src: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            })
        };
        let chunk_shader = shader("chunk", include_str!("shaders/chunk.wgsl"));
        let sky_shader = shader("sky", include_str!("shaders/sky.wgsl"));
        let line_shader = shader("line", include_str!("shaders/line.wgsl"));
        let ui_shader = shader("ui", include_str!("shaders/ui.wgsl"));

        let layout = |label: &str, bgls: &[&wgpu::BindGroupLayout]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: bgls,
                push_constant_ranges: &[],
            })
        };
        let chunk_layout = layout("chunk layout", &[&globals_bgl, &tex_bgl]);
        let line_layout = layout("line layout", &[&globals_bgl]);
        let ui_layout = layout("ui layout", &[&tex_bgl]);

        let no_depth_write = wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };

        const CHUNK_ATTRS: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32];
        let chunk_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("chunk pipeline"),
            layout: Some(&chunk_layout),
            vertex: wgpu::VertexState {
                module: &chunk_shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &CHUNK_ATTRS,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &chunk_shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sky_layout = layout("sky layout", &[&globals_bgl, &tex_bgl]);
        let sky_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky pipeline"),
            layout: Some(&sky_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(no_depth_write.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        const LINE_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
        let line_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("line pipeline"),
            layout: Some(&line_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &LINE_ATTRS,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        const UI_ATTRS: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let ui_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui pipeline"),
            layout: Some(&ui_layout),
            vertex: wgpu::VertexState {
                module: &ui_shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<UiVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &UI_ATTRS,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ui_shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(no_depth_write),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let ui_cap = 8192;
        let ui_vbuf = ui_buffer(&device, ui_cap);
        let line_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline"),
            size: 24 * 12,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let dyn_vb = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dynamic vb"),
            size: (DYN_VERTS * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dyn_ib = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dynamic ib"),
            size: (DYN_INDICES * 4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let view_dist = cfg.render.render_distance as f32 * CW as f32 * BLOCK;
        let sky = &cfg.sky;
        Ok(Renderer {
            surface,
            device,
            queue,
            config,
            depth,
            globals_buf,
            globals_bg,
            atlas_bg,
            sky_bg,
            sky_has_tex,
            chunk_pipe,
            sky_pipe,
            line_pipe,
            ui_pipe,
            meshes: HashMap::new(),
            ui_tex,
            ui_vbuf,
            ui_cap,
            line_vbuf,
            dyn_vb,
            dyn_ib,
            fov: cfg.render.fov,
            fog_start_frac: sky.fog_start,
            fog_end_frac: sky.fog_end,
            fog_start: view_dist * sky.fog_start,
            fog_end: view_dist * sky.fog_end,
            fog_color: sky.fog_color.unwrap_or(sky.horizon).linear(),
            sky_top: sky.top.linear(),
            sky_horizon: sky.horizon.linear(),
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Pixel size of an optional UI image (None if the PNG is missing).
    pub fn ui_size(&self, tex: Tex) -> Option<(u32, u32)> {
        self.ui_tex.get(&tex).map(|t| t.size)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.depth = make_depth(&self.device, w, h);
    }

    /// Re-aim the fog (and fov) after the player changed settings.
    pub fn set_view(&mut self, render_distance: i32, fov: f32) {
        let view_dist = render_distance as f32 * CW as f32 * BLOCK;
        self.fog_start = view_dist * self.fog_start_frac;
        self.fog_end = view_dist * self.fog_end_frac;
        self.fov = fov;
    }

    pub fn has_mesh(&self, key: (i32, i32)) -> bool {
        self.meshes.contains_key(&key)
    }

    pub fn remove_chunk(&mut self, key: (i32, i32)) {
        self.meshes.remove(&key);
    }

    /// Drop every chunk mesh (when switching worlds).
    pub fn clear_chunks(&mut self) {
        self.meshes.clear();
    }

    pub fn upload(&mut self, key: (i32, i32), mesh: MeshData) {
        if mesh.indices.is_empty() {
            self.meshes.insert(key, None);
            return;
        }
        let vb = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("chunk vb"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("chunk ib"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let half = CW as f32 * BLOCK / 2.0;
        let center = Vec3::new(
            key.0 as f32 * CW as f32 * BLOCK + half,
            0.0,
            key.1 as f32 * CW as f32 * BLOCK + half,
        );
        self.meshes.insert(key, Some(GpuMesh { vb, ib, count: mesh.indices.len() as u32, center }));
    }

    pub fn render(&mut self, scene: &Scene) -> Result<(), wgpu::SurfaceError> {
        // ---- per-frame uploads ----
        if let Some(cam) = scene.camera {
            let aspect = self.config.width as f32 / self.config.height as f32;
            let proj = Mat4::perspective_rh(self.fov.to_radians(), aspect, 0.05, 2000.0);
            let view = Mat4::look_to_rh(cam.eye, cam.forward, Vec3::Y);
            let vp = proj * view;
            let v4 = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
            let globals = Globals {
                view_proj: vp.to_cols_array_2d(),
                inv_view_proj: vp.inverse().to_cols_array_2d(),
                cam_pos: [cam.eye.x, cam.eye.y, cam.eye.z, 1.0],
                fog_color: v4(self.fog_color),
                sky_top: v4(self.sky_top),
                sky_horizon: v4(self.sky_horizon),
                params: [self.fog_start, self.fog_end, self.sky_has_tex as u32 as f32, 0.0],
            };
            self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        }

        if let Some((lo, hi)) = scene.highlight {
            let e = 0.004;
            let (lo, hi) = (lo - Vec3::splat(e), hi + Vec3::splat(e));
            let c = |x: bool, y: bool, z: bool| [if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z }];
            let corners = [
                c(false, false, false), c(true, false, false), c(true, false, true), c(false, false, true),
                c(false, true, false), c(true, true, false), c(true, true, true), c(false, true, true),
            ];
            const EDGES: [(usize, usize); 12] =
                [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
            let verts: Vec<[f32; 3]> = EDGES.iter().flat_map(|&(a, b)| [corners[a], corners[b]]).collect();
            self.queue.write_buffer(&self.line_vbuf, 0, bytemuck::cast_slice(&verts));
        }

        let mut dyn_count = 0;
        if let Some(m) = scene.dynamic {
            if m.vertices.len() <= DYN_VERTS && m.indices.len() <= DYN_INDICES && !m.indices.is_empty() {
                self.queue.write_buffer(&self.dyn_vb, 0, bytemuck::cast_slice(&m.vertices));
                self.queue.write_buffer(&self.dyn_ib, 0, bytemuck::cast_slice(&m.indices));
                dyn_count = m.indices.len() as u32;
            }
        }

        let (sw, sh) = (self.config.width as f32, self.config.height as f32);
        let ui_verts: Vec<UiVertex> = scene
            .ui
            .verts
            .iter()
            .map(|v| UiVertex {
                pos: [v.pos[0] * scene.ui_scale / sw * 2.0 - 1.0, 1.0 - v.pos[1] * scene.ui_scale / sh * 2.0],
                uv: v.uv,
                color: v.color,
            })
            .collect();
        if ui_verts.len() > self.ui_cap {
            self.ui_cap = ui_verts.len().next_power_of_two();
            self.ui_vbuf = ui_buffer(&self.device, self.ui_cap);
        }
        if !ui_verts.is_empty() {
            self.queue.write_buffer(&self.ui_vbuf, 0, bytemuck::cast_slice(&ui_verts));
        }

        // ---- draw ----
        let frame = self.surface.get_current_texture()?;
        let color_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if let Some(cam) = scene.camera {
                pass.set_pipeline(&self.sky_pipe);
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.set_bind_group(1, &self.sky_bg, &[]);
                pass.draw(0..3, 0..1);

                pass.set_pipeline(&self.chunk_pipe);
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.set_bind_group(1, &self.atlas_bg, &[]);
                let flat_fwd = Vec3::new(cam.forward.x, 0.0, cam.forward.z).normalize_or_zero();
                let margin = CW as f32 * BLOCK * 1.5;
                for mesh in self.meshes.values().flatten() {
                    // Cheap cull: skip chunks clearly behind the camera.
                    let to = Vec3::new(mesh.center.x - cam.eye.x, 0.0, mesh.center.z - cam.eye.z);
                    if to.dot(flat_fwd) < -margin && cam.forward.y.abs() < 0.9 {
                        continue;
                    }
                    pass.set_vertex_buffer(0, mesh.vb.slice(..));
                    pass.set_index_buffer(mesh.ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.count, 0, 0..1);
                }

                if dyn_count > 0 {
                    pass.set_vertex_buffer(0, self.dyn_vb.slice(..));
                    pass.set_index_buffer(self.dyn_ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..dyn_count, 0, 0..1);
                }

                if scene.highlight.is_some() {
                    pass.set_pipeline(&self.line_pipe);
                    pass.set_bind_group(0, &self.globals_bg, &[]);
                    pass.set_vertex_buffer(0, self.line_vbuf.slice(..));
                    pass.draw(0..24, 0..1);
                }
            }

            if !ui_verts.is_empty() {
                pass.set_pipeline(&self.ui_pipe);
                pass.set_vertex_buffer(0, self.ui_vbuf.slice(..));
                for cmd in &scene.ui.cmds {
                    if let Some(t) = self.ui_tex.get(&cmd.tex) {
                        pass.set_bind_group(0, &t.bind_group, &[]);
                        pass.draw(cmd.start..cmd.start + cmd.count, 0..1);
                    }
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}
