// The live view on the GPU: see_build's scene pipelines again for
// Bgra8Unorm (GPUI's byte order) with 4x multisampling, or one sample on a
// software adapter; targets and a ring of 3 readback buffers sized for the
// largest pane seen, drawn into in part (a resize allocates nothing until
// the pane grows past what it has been); the pick (one pixel, the point and
// the face hit); and the overlays.

use crate::camera::{fog_range, to_f32, Mat4, MAX_CAMERA_DISTANCE};
use crate::gpu::{
    Batches, DeviceError, Draws, Gpu, Renderer, ScenePipelines, Uniforms, DEPTH, FOG_COLOR,
};
use crate::live::camera::Lens;
use crate::live::night::{self, Night};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const PICK: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Uint;
const HEIGHT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;
const RING: usize = 3;
// The overlays' look (web/bluemap-bridge.js:388, 411-433): bone white
// rgb(245, 245, 245), edges 2 pixels wide and opaque; the marked block's cube
// white at half opacity, 1.01 blocks (BlueMap:src/js/PopupMarker.js:51-53).
const BONE: f32 = 245.0 / 255.0;
const LINE_WIDTH: f32 = 2.0;
const MARKED_SIZE: f32 = 1.01;
const MARKED_OPACITY: f32 = 0.5;

struct Targets {
    size: [u32; 2],
    ms: Option<wgpu::TextureView>,
    color: wgpu::Texture,
    depth: wgpu::TextureView,
}

pub struct View {
    pipes: ScenePipelines,
    fill_both: wgpu::RenderPipeline,
    fill_front: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    pick: wgpu::RenderPipeline,
    height: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    scene_group: wgpu::BindGroup,
    pub samples: u32,
    targets: Option<Targets>,
    ring: Vec<wgpu::Buffer>,
    ring_size: [u32; 2],
    next: usize,
    pick_color: wgpu::Texture,
    height_color: wgpu::Texture,
    pick_depth: wgpu::TextureView,
    pick_read: wgpu::Buffer,
    // The night's lightmaps (group 3 of the quads' pipelines), and the
    // night sky's draws.
    lightmaps: wgpu::BindGroup,
    night_sky: NightSky,
}

// The sky pass Minecraft's client draws at 18000 (night.rs), drawn as its
// shaders draw it: the disc fogged by distance, the sun and moon textured,
// the stars; additive draws blend SRC_ALPHA, ONE as the client's pipelines
// do (their alpha left as the disc wrote it: the frame stays opaque).
struct NightSky {
    disc: wgpu::RenderPipeline,
    stars: wgpu::RenderPipeline,
    texture: wgpu::RenderPipeline,
    fog_color: [f32; 4],
    sky_end: f32,
    draws: Vec<NightDraw>,
}

struct NightDraw {
    shader: night::Shader,
    vertices: wgpu::Buffer,
    count: u32,
    model_view: Mat4,
    color: [f32; 4],
    uniforms: wgpu::Buffer,
    group: wgpu::BindGroup,
    texture: wgpu::BindGroup,
}

// A frame on the GPU: its ring slot, its size, and whether its rows are mapped.
pub struct InFlight {
    pub slot: usize,
    pub size: [u32; 2],
    pub submission: wgpu::SubmissionIndex,
    pub mapped: Arc<AtomicBool>,
    pub started: f64,
    pub submitted: f64,
}

// The overlays as the GPU draws them: triangles (both sides, then front
// faces only) and line corners.
#[derive(Default)]
pub struct OverlayGeometry {
    pub both: Vec<[f32; 7]>,
    pub front: Vec<[f32; 7]>,
    pub lines: Vec<[f32; 12]>,
}

fn padded(width: u32) -> u32 {
    (width * 4).div_ceil(256) * 256
}

impl View {
    pub fn new(gpu: &Gpu, renderer: &Renderer, software: bool, night: &Night) -> View {
        let device = &gpu.device;
        // On a software adapter, no multisampling (the answers' floor).
        let samples = if software { 1 } else { crate::gpu::SAMPLES };
        let source = format!(
            "{}\n{}",
            include_str!("../shader.wgsl"),
            include_str!("../live.wgsl")
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("live"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let lightmaps_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lightmaps"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let pipes = renderer.live_pipelines(gpu, &module, &lightmaps_layout, FORMAT, samples);
        let mut maps: Vec<[f32; 4]> = night.day.clone();
        maps.extend_from_slice(&night.night);
        let lightmaps_buffer = buffer_of(
            gpu,
            bytemuck::cast_slice(&maps),
            wgpu::BufferUsages::UNIFORM,
        );
        let lightmaps = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lightmaps"),
            layout: &lightmaps_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: lightmaps_buffer.as_entire_binding(),
            }],
        });
        let night_sky = NightSky::new(gpu, &module, night, samples);
        let overlay_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&renderer.scene_layout)],
            immediate_size: 0,
        });
        let blend = wgpu::BlendState::ALPHA_BLENDING;
        let depth = |write: bool| {
            Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(write),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let float =
            |location: u32, offset: u64, format: wgpu::VertexFormat| wgpu::VertexAttribute {
                format,
                offset,
                shader_location: location,
            };
        let fill_attrs = [
            float(0, 0, wgpu::VertexFormat::Float32x3),
            float(1, 12, wgpu::VertexFormat::Float32x4),
        ];
        let line_attrs = [
            float(0, 0, wgpu::VertexFormat::Float32x3),
            float(1, 12, wgpu::VertexFormat::Float32x3),
            float(2, 24, wgpu::VertexFormat::Float32x4),
            float(3, 40, wgpu::VertexFormat::Float32x2),
        ];
        let overlay = |label: &str,
                       vs: &str,
                       attrs: &[wgpu::VertexAttribute],
                       stride: u64,
                       cull: Option<wgpu::Face>,
                       write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&overlay_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: attrs,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: cull,
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: depth(write),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_overlay"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // A box's fill from both sides and not writing depth (BlueMap's
        // ExtrudeMarker fill: DoubleSide, transparent); the marked cube's
        // front faces, writing depth (three.js MeshBasicMaterial).
        let fill_both = overlay("overlay fill", "vs_fill", &fill_attrs, 28, None, false);
        let fill_front = overlay(
            "marked cube",
            "vs_fill",
            &fill_attrs,
            28,
            Some(wgpu::Face::Back),
            true,
        );
        let line = overlay("overlay lines", "vs_line", &line_attrs, 48, None, false);
        let one_pixel = |label: &str, fs: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&renderer.layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_pick"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: depth(true),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pick = one_pixel("pick", "fs_pick", PICK);
        let height = one_pixel("terrain height", "fs_height", HEIGHT);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("live uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &renderer.scene_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let one = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pick"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let pick_color = one(
            PICK,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let height_color = one(
            HEIGHT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let pick_depth =
            one(DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let pick_read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pick readback"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        View {
            pipes,
            fill_both,
            fill_front,
            line,
            pick,
            height,
            uniforms,
            scene_group,
            samples,
            targets: None,
            ring: Vec::new(),
            ring_size: [0, 0],
            next: 0,
            pick_color,
            height_color,
            pick_depth,
            pick_read,
            lightmaps,
            night_sky,
        }
    }

    // The bytes the targets and ring take at `size`.
    pub fn bytes_at(&self, gpu: &Gpu, size: [u32; 2]) -> u64 {
        let (w, h) = (size[0] as u64, size[1] as u64);
        let multisampled =
            if self.samples == 1 || gpu.adapter.get_info().transient_saves_memory == Some(true) {
                0
            } else {
                self.samples as u64 * 8
            };
        let single = if self.samples == 1 { 8 } else { 4 };
        w * h * (multisampled + single) + RING as u64 * padded(size[0]) as u64 * h
    }

    // The bytes the ring it holds takes (read back into the process's
    // memory), and with `all` its targets too (store.rs, Counting).
    pub fn held_bytes(&self, gpu: &Gpu, all: bool) -> u64 {
        let ring = RING as u64 * padded(self.ring_size[0]) as u64 * self.ring_size[1] as u64;
        let targets = match &self.targets {
            Some(t) if all => {
                let [w, h] = t.size;
                self.bytes_at(gpu, t.size) - RING as u64 * padded(w) as u64 * h as u64
            }
            _ => 0,
        };
        ring + targets
    }

    // Targets and ring at least `size` (the largest seen each way).
    fn ensure(&mut self, gpu: &Gpu, size: [u32; 2]) {
        let have = self.targets.as_ref().map_or([0, 0], |t| t.size);
        if size[0] > have[0] || size[1] > have[1] {
            let s = [size[0].max(have[0]), size[1].max(have[1])];
            let texture = |label: &str, format, samples, usage| {
                gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: s[0],
                        height: s[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
            let transient = attachment | wgpu::TextureUsages::TRANSIENT_ATTACHMENT;
            let (ms, depth) = if self.samples > 1 {
                (
                    Some(
                        texture("live ms", FORMAT, self.samples, transient)
                            .create_view(&Default::default()),
                    ),
                    texture("live depth", DEPTH, self.samples, transient)
                        .create_view(&Default::default()),
                )
            } else {
                (
                    None,
                    texture("live depth", DEPTH, 1, attachment).create_view(&Default::default()),
                )
            };
            let color = texture(
                "live colour",
                FORMAT,
                1,
                attachment | wgpu::TextureUsages::COPY_SRC,
            );
            self.targets = Some(Targets {
                size: s,
                ms,
                color,
                depth,
            });
        }
        if size[0] > self.ring_size[0] || size[1] > self.ring_size[1] {
            let s = [
                size[0].max(self.ring_size[0]),
                size[1].max(self.ring_size[1]),
            ];
            self.ring = (0..RING)
                .map(|_| {
                    gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("live readback"),
                        size: (padded(s[0]) * s[1]) as u64,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    })
                })
                .collect();
            self.ring_size = s;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write_uniforms(
        &self,
        gpu: &Gpu,
        lens: &Lens,
        y_offset: i32,
        bounds: ([i32; 3], [i32; 3]),
        size: [u32; 2],
        proj: &Mat4,
        night: bool,
    ) {
        let log_depth = 2.0 / (MAX_CAMERA_DISTANCE + 1.0).log2();
        let fog = fog_range(bounds.0, bounds.1);
        let u = Uniforms {
            view: to_f32(&scene_view(lens, y_offset)),
            proj: to_f32(proj),
            sky_unproject: to_f32(&lens.sky_unproject),
            // At night the fog is the client's fog colour at 18000.
            fog_color: if night {
                self.night_sky.fog_color
            } else {
                FOG_COLOR
            },
            params: [
                log_depth as f32,
                if lens.linear { 1.0 } else { 0.0 },
                size[0] as f32,
                size[1] as f32,
            ],
            fog: [fog[0], fog[1], LINE_WIDTH, if night { 1.0 } else { 0.0 }],
        };
        gpu.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
    }

    // One frame of `size`: the scene's parts in see_build's order, then the
    // overlays, resolved and copied into the next ring slot, which is mapped
    // when the GPU is done.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        renderer: &Renderer,
        draws: &Draws,
        lens: &Lens,
        y_offset: i32,
        bounds: ([i32; 3], [i32; 3]),
        size: [u32; 2],
        overlays: &OverlayGeometry,
        time: mcbench_render::live::TimeOfDay,
    ) -> InFlight {
        let night = time == mcbench_render::live::TimeOfDay::Night;
        let started = crate::now_ms();
        self.ensure(gpu, size);
        self.write_uniforms(gpu, lens, y_offset, bounds, size, &lens.proj, night);
        if night {
            self.night_sky.write(gpu, lens);
        }
        // At night the frame is cleared to the fog colour, as the client
        // clears it (its sky pass then draws over it).
        let clear = if night {
            let c = self.night_sky.fog_color;
            wgpu::Color {
                r: c[0] as f64,
                g: c[1] as f64,
                b: c[2] as f64,
                a: 1.0,
            }
        } else {
            wgpu::Color::TRANSPARENT
        };
        let fill =
            |data: &[[f32; 7]]| (!data.is_empty()).then(|| buffer(gpu, bytemuck::cast_slice(data)));
        let (both, front) = (fill(&overlays.both), fill(&overlays.front));
        let lines = (!overlays.lines.is_empty())
            .then(|| buffer(gpu, bytemuck::cast_slice(&overlays.lines)));
        let t = self.targets.as_ref().expect("targets made");
        let color_view = t.color.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let (view, resolve) = match &t.ms {
                Some(ms) => (ms, Some(&color_view)),
                None => (&color_view, None),
            };
            let store = if t.ms.is_some() {
                wgpu::StoreOp::Discard
            } else {
                wgpu::StoreOp::Store
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("live"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
            pass.set_scissor_rect(0, 0, size[0], size[1]);
            if night {
                self.night_sky.draw(&mut pass);
            }
            pass.set_bind_group(0, &self.scene_group, &[]);
            pass.set_bind_group(1, &renderer.block_atlas.group, &[]);
            pass.set_bind_group(3, &self.lightmaps, &[]);
            if !night {
                pass.set_pipeline(&self.pipes.sky);
                pass.draw(0..3, 0..1);
            }
            pass.set_pipeline(&self.pipes.blocks);
            for batches in &draws.blocks {
                renderer.draw_batches(&mut pass, batches);
            }
            if let Some(translucent) = &draws.translucent {
                pass.set_pipeline(&self.pipes.ordered);
                let groups: Vec<wgpu::BindGroup> = translucent
                    .batches
                    .buffers
                    .iter()
                    .map(|(buffer, _)| {
                        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("ordered quads"),
                            layout: &renderer.ordered_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: buffer.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: translucent.order.as_entire_binding(),
                                },
                            ],
                        })
                    })
                    .collect();
                for &(buffer, first, count) in &translucent.runs {
                    pass.set_bind_group(2, &groups[buffer], &[]);
                    pass.draw(6 * first..6 * (first + count), 0..1);
                }
            }
            if draws.solid.is_some() || draws.behind.is_some() {
                pass.set_bind_group(
                    1,
                    &renderer
                        .entity_atlas
                        .get()
                        .expect("the entity atlas is uploaded before entities are drawn")
                        .group,
                    &[],
                );
            }
            if let Some(solid) = draws.solid {
                pass.set_pipeline(&self.pipes.solid);
                renderer.draw_batches(&mut pass, solid);
            }
            if let Some(behind) = &draws.behind {
                pass.set_pipeline(&self.pipes.blocks);
                renderer.draw_batches(&mut pass, behind);
            }
            for (pipeline, data, count) in [
                (&self.fill_both, &both, overlays.both.len()),
                (&self.fill_front, &front, overlays.front.len()),
                (&self.line, &lines, overlays.lines.len()),
            ] {
                if let Some(data) = data {
                    pass.set_pipeline(pipeline);
                    pass.set_vertex_buffer(0, data.slice(..));
                    pass.draw(0..count as u32, 0..1);
                }
            }
        }
        let slot = self.next;
        self.next = (self.next + 1) % RING;
        let ring = &self.ring[slot];
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &t.color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: ring,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded(size[0])),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        let submission = gpu.queue.submit([encoder.finish()]);
        let mapped = Arc::new(AtomicBool::new(false));
        let flag = mapped.clone();
        ring.slice(..(padded(size[0]) * size[1]) as u64).map_async(
            wgpu::MapMode::Read,
            move |result| {
                if result.is_ok() {
                    flag.store(true, Ordering::SeqCst);
                }
            },
        );
        InFlight {
            slot,
            size,
            submission,
            mapped,
            started,
            submitted: crate::now_ms(),
        }
    }

    // A frame given up before its rows are read: its slot unmapped (a
    // mapping still on its way cancelled).
    pub fn release(&self, frame: &InFlight) {
        self.ring[frame.slot].unmap();
    }

    // A drawn frame's rows, unpadded, `Bgra8Unorm`; the slot unmapped.
    pub fn read(&self, frame: &InFlight) -> Result<Vec<u8>, DeviceError> {
        let [w, h] = frame.size;
        let ring = &self.ring[frame.slot];
        let row = (w * 4) as usize;
        let pad = padded(w) as usize;
        let mut out = Vec::with_capacity(row * h as usize);
        {
            let data = ring
                .slice(..(pad * h as usize) as u64)
                .get_mapped_range()
                .map_err(|e| DeviceError(format!("{e:?}")))?;
            for y in 0..h as usize {
                out.extend_from_slice(&data[y * pad..y * pad + row]);
            }
        }
        ring.unmap();
        Ok(out)
    }

    // The quad under `ndc` (the frame's normalized device coordinates) with
    // a frame's lens: the opaque and cutout quads of `blocks` drawn again
    // into one pixel (the projection scaled about the point by the frame's
    // `pixel` size, so the pixel fills the target). Answers the hit quad's
    // buffer (its index in `blocks`' buffers in order), its index there,
    // and its normal's axis (1 to 6); none where the sky is.
    #[allow(clippy::too_many_arguments)]
    pub fn pick(
        &self,
        gpu: &Gpu,
        renderer: &Renderer,
        blocks: &Batches,
        lens: &Lens,
        y_offset: i32,
        bounds: ([i32; 3], [i32; 3]),
        ndc: [f64; 2],
        pixel: [f64; 2],
    ) -> Result<Option<(usize, u32, u8)>, DeviceError> {
        // Clip space scaled by the frame's pixels about the point: x' = s (x - c w).
        let mut m = crate::camera::identity();
        m[0] = pixel[0];
        m[5] = pixel[1];
        m[12] = -pixel[0] * ndc[0];
        m[13] = -pixel[1] * ndc[1];
        let proj = crate::camera::multiply(&m, &lens.proj);
        self.write_uniforms(gpu, lens, y_offset, bounds, [1, 1], &proj, false);
        let ranges: Vec<(&wgpu::Buffer, u32, u32, u32)> = blocks
            .buffers
            .iter()
            .enumerate()
            .map(|(b, (buffer, n))| (buffer, 0, *n, b as u32))
            .collect();
        let texel = self.one_pixel(gpu, renderer, &self.pick, &self.pick_color, &ranges)?;
        let t: [u32; 4] = bytemuck::cast(texel);
        if t[0] == 0 {
            return Ok(None);
        }
        Ok(Some(((t[0] - 1) as usize, t[1], t[2] as u8)))
    }

    // BlueMap's map.terrainHeightAt (BlueMap:src/js/map/Map.js:432-455): a
    // ray from y 300 straight down, near 1 and far 300 (Map.js:435-440),
    // against the triangles of the one tile under it (Map.js:443-450),
    // front faces only, whatever their texture: the height of the first it
    // meets (Minecraft's), none when it meets none (the app's map has no
    // lowres levels, so Map.js:457-484 adds nothing: Map.js:486 answers
    // false). `ranges` are the tile's quads (buffer, first, count). Drawn
    // as one pixel of an orthographic camera looking down from (x, 300, z).
    #[allow(clippy::too_many_arguments)]
    pub fn height(
        &self,
        gpu: &Gpu,
        renderer: &Renderer,
        ranges: &[(&wgpu::Buffer, u32, u32)],
        y_offset: i32,
        x: f64,
        z: f64,
    ) -> Result<Option<f64>, DeviceError> {
        // The eye at (x, 300, z), Minecraft's heights, looking down -y with
        // -z up: the camera's world matrix's inverse.
        let world: Mat4 = [
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, x, 300.0, z, 1.0,
        ];
        let view = crate::camera::invert(&world);
        let proj = crate::camera::ortho(-0.001, 0.001, -0.001, 0.001, 1.0, 300.0);
        let lens = Lens {
            view,
            proj,
            sky_unproject: crate::camera::identity(),
            linear: true,
            eye: [x, 300.0, z],
        };
        self.write_uniforms(gpu, &lens, y_offset, ([0; 3], [0; 3]), [1, 1], &proj, false);
        let ranges: Vec<(&wgpu::Buffer, u32, u32, u32)> = ranges
            .iter()
            .map(|&(b, first, n)| (b, first, n, 0))
            .collect();
        let texel = self.one_pixel(gpu, renderer, &self.height, &self.height_color, &ranges)?;
        let t: [f32; 4] = bytemuck::cast(texel);
        Ok((t[1] != 0.0).then(|| t[0] as f64 + y_offset as f64))
    }

    // One pixel drawn by `pipeline` into `color` (cleared to zeros) with the
    // depth test, over each (buffer, first quad, count, instance) range of
    // quads, and read back: its 16 bytes.
    fn one_pixel(
        &self,
        gpu: &Gpu,
        renderer: &Renderer,
        pipeline: &wgpu::RenderPipeline,
        color: &wgpu::Texture,
        ranges: &[(&wgpu::Buffer, u32, u32, u32)],
    ) -> Result<[u8; 16], DeviceError> {
        let view = color.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("one pixel"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.pick_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.scene_group, &[]);
            pass.set_bind_group(1, &renderer.block_atlas.group, &[]);
            pass.set_pipeline(pipeline);
            for &(buffer, first, count, instance) in ranges {
                if count == 0 {
                    continue;
                }
                let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("quads"),
                    layout: &renderer.quads_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                });
                pass.set_bind_group(2, &group, &[]);
                pass.draw(6 * first..6 * (first + count), instance..instance + 1);
            }
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.pick_read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let slice = self.pick_read.slice(..16);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        gpu.wait()?;
        let mut texel = [0u8; 16];
        {
            let data = slice
                .get_mapped_range()
                .map_err(|e| DeviceError(format!("{e:?}")))?;
            texel.copy_from_slice(&data[..16]);
        }
        self.pick_read.unmap();
        Ok(texel)
    }
}

// The camera's view of the scene, whose heights are the page's (Minecraft's
// less the offset): the lens is Minecraft's.
fn scene_view(lens: &Lens, y_offset: i32) -> Mat4 {
    crate::camera::multiply(
        &lens.view,
        &crate::camera::translate([0.0, y_offset as f64, 0.0]),
    )
}

fn buffer(gpu: &Gpu, data: &[u8]) -> wgpu::Buffer {
    let b = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("overlays"),
        size: data.len() as u64,
        usage: wgpu::BufferUsages::VERTEX,
        mapped_at_creation: true,
    });
    b.slice(..)
        .get_mapped_range_mut()
        .expect("mapped at creation")
        .copy_from_slice(data);
    b.unmap();
    b
}

// The overlays in the page's coordinates: each box's fill (when it has
// one) and edges over whole blocks, min and max included; the marked
// block's cube.
pub fn overlay_geometry(
    overlays: &mcbench_render::live::Overlays,
    y_offset: i32,
) -> OverlayGeometry {
    let mut g = OverlayGeometry::default();
    let yo = y_offset as f32;
    let corner = |min: [f32; 3], max: [f32; 3], i: usize| {
        [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ]
    };
    // The six faces, counter-clockwise seen from outside, as corner indices.
    const FACES: [[usize; 4]; 6] = [
        [0, 4, 6, 2],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 2, 3, 1],
        [4, 5, 7, 6],
    ];
    const EDGES: [[usize; 2]; 12] = [
        [0, 1],
        [2, 3],
        [4, 5],
        [6, 7],
        [0, 2],
        [1, 3],
        [4, 6],
        [5, 7],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];
    let faces = |out: &mut Vec<[f32; 7]>, min: [f32; 3], max: [f32; 3], rgba: [f32; 4]| {
        for f in FACES {
            for k in [0, 1, 2, 0, 2, 3] {
                let p = corner(min, max, f[k]);
                out.push([p[0], p[1], p[2], rgba[0], rgba[1], rgba[2], rgba[3]]);
            }
        }
    };
    for b in &overlays.boxes {
        // The page's box, grown by 0.01 on every side, so its faces lie off
        // the selected blocks' faces and are not lost to them under the depth
        // test: web/bluemap-bridge.js boxMarker gives ExtrudeMarker the shape
        // x1 - 0.02 to x2 and z1 - 0.02 to z2 over y min to max + 1, and
        // ExtrudeMarker adds 0.01 to every point's x and z
        // (BlueMap:src/js/markers/ExtrudeMarker.js:163-164) and 0.01 below and
        // above (ExtrudeMarker.js:113, "offset by 0.01 to avoid z-fighting").
        const PAD: f32 = 0.01;
        let min = [
            b.min[0] as f32 - PAD,
            b.min[1] as f32 - yo - PAD,
            b.min[2] as f32 - PAD,
        ];
        let max = [
            b.max[0] as f32 + 1.0 + PAD,
            b.max[1] as f32 + 1.0 - yo + PAD,
            b.max[2] as f32 + 1.0 + PAD,
        ];
        if b.fill > 0.0 {
            faces(&mut g.both, min, max, [BONE, BONE, BONE, b.fill]);
        }
        if b.outline {
            for e in EDGES {
                let (a, z) = (corner(min, max, e[0]), corner(min, max, e[1]));
                for (end, side) in [
                    (0.0, -1.0),
                    (1.0, -1.0),
                    (1.0, 1.0),
                    (0.0, -1.0),
                    (1.0, 1.0),
                    (0.0, 1.0),
                ] {
                    g.lines.push([
                        a[0], a[1], a[2], z[0], z[1], z[2], BONE, BONE, BONE, 1.0, end, side,
                    ]);
                }
            }
        }
    }
    if let Some(m) = overlays.marked {
        let pad = (MARKED_SIZE - 1.0) / 2.0;
        let min = [m[0] as f32 - pad, m[1] as f32 - yo - pad, m[2] as f32 - pad];
        let max = [
            m[0] as f32 + 1.0 + pad,
            m[1] as f32 + 1.0 - yo + pad,
            m[2] as f32 + 1.0 + pad,
        ];
        faces(&mut g.front, min, max, [1.0, 1.0, 1.0, MARKED_OPACITY]);
    }
    g
}

// Each anchor projected into a frame of `size` (pixels from the top left),
// none behind the eye.
pub fn project(lens: &Lens, anchors: &[[f64; 3]], size: [u32; 2]) -> Vec<Option<[f32; 2]>> {
    let m = crate::camera::multiply(&lens.proj, &lens.view);
    anchors
        .iter()
        .map(|p| {
            let c =
                [0, 1, 2, 3].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r]);
            if c[3] <= 0.0 {
                return None;
            }
            let (x, y) = (c[0] / c[3], c[1] / c[3]);
            Some([
                ((x + 1.0) / 2.0 * size[0] as f64) as f32,
                ((1.0 - y) / 2.0 * size[1] as f64) as f32,
            ])
        })
        .collect()
}

fn buffer_of(gpu: &Gpu, data: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let b = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("night"),
        size: data.len() as u64,
        usage,
        mapped_at_creation: true,
    });
    b.slice(..)
        .get_mapped_range_mut()
        .expect("mapped at creation")
        .copy_from_slice(data);
    b.unmap();
    b
}

// What one sky draw's shaders read (live.wgsl SkyUniforms).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniforms {
    mvp: [f32; 16],
    color: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
}

impl NightSky {
    fn new(gpu: &Gpu, module: &wgpu::ShaderModule, night: &Night, samples: u32) -> NightSky {
        let device = &gpu.device;
        let group0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("night sky"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let group1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("night sky texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&group0), Some(&group1)],
            immediate_size: 0,
        });
        let attrs = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 12,
                shader_location: 1,
            },
        ];
        // BlendFunction SRC_ALPHA, ONE on colour (the celestial and stars
        // pipelines); the frame's alpha kept.
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = |label: &str, fs: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_night_sky"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 20,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attrs,
                    })],
                },
                // The client's pipelines cull (night.json pipelineState.cull).
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let disc = pipeline("night sky disc", "fs_night_sky", None);
        let stars = pipeline("stars", "fs_night_stars", Some(additive));
        let texture = pipeline("sun and moon", "fs_night_texture", Some(additive));
        // NEAREST, as the client's sun and moon textures are sampled.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let texture_group = |w: u32, h: u32, rgba: &[u8]| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sky texture"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &t,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky texture"),
                layout: &group1,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(
                            &t.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        };
        let draws = night
            .draws
            .iter()
            .map(|d| {
                let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sky draw"),
                    size: std::mem::size_of::<SkyUniforms>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sky draw"),
                    layout: &group0,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniforms.as_entire_binding(),
                    }],
                });
                let texture = match &d.texture {
                    Some((_, w, h, rgba)) => texture_group(*w, *h, rgba),
                    // The shaders without a texture read none: a texel stands in.
                    None => texture_group(1, 1, &[255; 4]),
                };
                NightDraw {
                    shader: d.shader,
                    vertices: buffer_of(
                        gpu,
                        bytemuck::cast_slice(&d.vertices),
                        wgpu::BufferUsages::VERTEX,
                    ),
                    count: d.vertices.len() as u32,
                    model_view: d.model_view.map(|v| v as f64),
                    color: d.color,
                    uniforms,
                    group,
                    texture,
                }
            })
            .collect();
        NightSky {
            disc,
            stars,
            texture,
            fog_color: night.fog_color,
            sky_end: night.sky_end,
            draws,
        }
    }

    // Each draw's matrix for a frame: the client's sky is drawn about the
    // eye, with the camera's rotation alone (the projection the frame's, or
    // see_build's 70 degree one for an orthographic lens, as its day sky).
    fn write(&self, gpu: &Gpu, lens: &Lens) {
        let mut rotation = lens.view;
        rotation[12] = 0.0;
        rotation[13] = 0.0;
        rotation[14] = 0.0;
        let proj = if lens.linear {
            crate::camera::multiply(
                &crate::camera::invert(&lens.sky_unproject),
                &crate::camera::invert(&rotation),
            )
        } else {
            lens.proj
        };
        for d in &self.draws {
            let mvp =
                crate::camera::multiply(&proj, &crate::camera::multiply(&rotation, &d.model_view));
            let u = SkyUniforms {
                mvp: to_f32(&mvp),
                color: d.color,
                fog_color: self.fog_color,
                fog: [self.sky_end, 0.0, 0.0, 0.0],
            };
            gpu.queue
                .write_buffer(&d.uniforms, 0, bytemuck::bytes_of(&u));
        }
    }

    fn draw(&self, pass: &mut wgpu::RenderPass) {
        for d in &self.draws {
            pass.set_pipeline(match d.shader {
                night::Shader::Sky => &self.disc,
                night::Shader::Stars => &self.stars,
                night::Shader::Texture => &self.texture,
            });
            pass.set_bind_group(0, &d.group, &[]);
            pass.set_bind_group(1, &d.texture, &[]);
            pass.set_vertex_buffer(0, d.vertices.slice(..));
            pass.draw(0..d.count, 0..1);
        }
    }
}
