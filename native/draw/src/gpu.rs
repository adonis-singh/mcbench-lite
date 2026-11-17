// The wgpu side: the adapter and device, see_build's pipelines (sky, quads,
// entities, the downscale, and a surface pass for coverage masks), the two
// atlases, scenes uploaded in bounded buffers, and offscreen pictures read
// back. Nothing here opens a window; a window's swapchain can take the same
// pipelines (`Renderer::new` takes the colour format).

use crate::assets::Atlas;
use crate::camera::{fog_range, to_f32, Camera, Frame, MAX_CAMERA_DISTANCE};
use crate::scene::{Instance, INSTANCE_BYTES};
use std::sync::{Arc, Mutex};

pub const SAMPLES: u32 = 4;
pub const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SURFACE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
// Bytes of quads per buffer: the downlevel limit of a storage buffer binding (128 MiB).
pub const BATCH_BYTES: u64 = 128 << 20;
const FOG_COLOR: [f32; 4] = [0.74, 0.84, 0.94, 1.0];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view: [f32; 16],
    proj: [f32; 16],
    sky_unproject: [f32; 16],
    fog_color: [f32; 4],
    params: [f32; 4],
    fog: [f32; 4],
}

// Which adapter hello asks for (PROTOCOL.md): a hardware one, or the
// software floor (`software`, hello's `adapter: "software"`, for tests and
// CI), and on Linux the floor's driver library (hello's
// `assets.softwareVulkan`).
#[derive(Clone, Debug, Default)]
pub struct Choice {
    pub software: bool,
    pub software_vulkan: Option<std::path::PathBuf>,
}

// The backends hardware adapters are looked for on, in order, on this
// platform.
pub fn platform_order() -> Vec<(&'static str, wgpu::Backends)> {
    if cfg!(target_os = "macos") {
        vec![("metal", wgpu::Backends::METAL)]
    } else if cfg!(target_os = "windows") {
        vec![("dx12", wgpu::Backends::DX12), ("vulkan", wgpu::Backends::VULKAN)]
    } else {
        vec![("vulkan", wgpu::Backends::VULKAN), ("gl", wgpu::Backends::GL)]
    }
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AdapterReport {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    pub software: bool,
}

// What went wrong with the device: wgpu's lost callback and its errors.
#[derive(Default)]
pub struct Health {
    pub lost: Option<String>,
    pub error: Option<String>,
}

pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub health: Arc<Mutex<Health>>,
    // Buffers of quads written in place (mappable): on an adapter whose
    // memory is the CPU's (Apple's GPUs, SwiftShader), so no staging copy of
    // a world's quads is made beside them (on the M3 the staging was let go
    // late: a 3 million block world held 2.2 GB to upload 1 GB of quads).
    pub direct: bool,
}

impl Gpu {
    // The adapter (native/draw-floor/README.md): the first hardware one
    // (any device type but Cpu; a discrete GPU before an integrated one) of
    // the platform's backends in turn. With none, or when the floor is
    // asked for: on Linux the shipped SwiftShader opened directly (system
    // software drivers are passed over, so every machine without a GPU
    // draws with the same tested driver); on Windows wgpu's WARP; on macOS
    // an error. Then a device with the downlevel limits of its kind at the
    // adapter's texture sizes.
    pub fn open(choice: &Choice) -> Result<Gpu, String> {
        let mut reasons = Vec::new();
        if !choice.software {
            for (name, backends) in platform_order() {
                let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
                let mut adapters: Vec<wgpu::Adapter> = pollster::block_on(instance.enumerate_adapters(backends)).into_iter().filter(|a| a.get_info().device_type != wgpu::DeviceType::Cpu).collect();
                adapters.sort_by_key(|a| a.get_info().device_type != wgpu::DeviceType::DiscreteGpu);
                match adapters.into_iter().next() {
                    Some(adapter) => match Gpu::with(adapter) {
                        Ok(gpu) => return Ok(gpu),
                        Err(e) => reasons.push(format!("{name}: {e}")),
                    },
                    None => reasons.push(format!("{name}: no hardware adapter")),
                }
            }
        }
        Gpu::floor(choice).map_err(|e| if reasons.is_empty() { e } else { format!("{e} (hardware: {})", reasons.join("; ")) })
    }

    #[cfg(target_os = "linux")]
    fn floor(choice: &Choice) -> Result<Gpu, String> {
        // Linux builds without a shipped floor (arm64 in this stage) get no
        // softwareVulkan: nothing else is searched for.
        let driver = choice.software_vulkan.as_ref().ok_or_else(|| format!("This machine has no graphics adapter, and this build for Linux {} ships no software renderer", std::env::consts::ARCH))?;
        let instance = crate::floor::direct_vulkan(driver)?;
        let adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN)).into_iter().next().ok_or_else(|| format!("No graphics adapter: {} offers no Vulkan device", driver.display()))?;
        Gpu::with(adapter)
    }

    #[cfg(target_os = "windows")]
    fn floor(_choice: &Choice) -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::DX12, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, force_fallback_adapter: true, compatible_surface: None, apply_limit_buckets: false }))
            .map_err(|e| format!("No graphics adapter: Direct3D 12 offers no WARP adapter: {e}"))?;
        Gpu::with(adapter)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    fn floor(choice: &Choice) -> Result<Gpu, String> {
        Err(if choice.software { "No software adapter on macOS: Metal or nothing".to_string() } else { "No Metal device".to_string() })
    }

    fn with(adapter: wgpu::Adapter) -> Result<Gpu, String> {
        let info = adapter.get_info();
        // The quads are read from storage buffers in the vertex shader.
        if !adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::VERTEX_STORAGE) {
            return Err(format!("{} ({:?}) cannot read storage buffers in a vertex shader", info.name, info.backend));
        }
        let base = wgpu::Limits::downlevel_defaults();
        let direct = matches!(info.device_type, wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::Cpu) && info.backend != wgpu::Backend::Dx12 && adapter.features().contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mcbench-draw"),
            required_features: if direct { wgpu::Features::MAPPABLE_PRIMARY_BUFFERS } else { wgpu::Features::empty() },
            required_limits: base.using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("No device on {}: {e}", info.name))?;
        let health = Arc::new(Mutex::new(Health::default()));
        let h = health.clone();
        device.on_uncaptured_error(Arc::new(move |e| {
            let mut h = h.lock().unwrap();
            if h.error.is_none() {
                h.error = Some(e.to_string());
            }
        }));
        let h = health.clone();
        device.set_device_lost_callback(move |reason, message| {
            h.lock().unwrap().lost = Some(format!("{reason:?}: {message}"));
        });
        Ok(Gpu { adapter, device, queue, info, health, direct })
    }

    pub fn report(&self) -> AdapterReport {
        let i = &self.info;
        AdapterReport {
            name: i.name.clone(),
            backend: format!("{:?}", i.backend),
            device_type: format!("{:?}", i.device_type),
            driver: i.driver.clone(),
            driver_info: i.driver_info.clone(),
            software: i.device_type == wgpu::DeviceType::Cpu,
        }
    }

    // Waits for the GPU; a lost device or an error wgpu reported is an
    // error naming it.
    // On Metal, the device's allocated size (MTLDevice.currentAllocatedSize):
    // the GPU memory its live resources hold, which the physical footprint
    // still counts for up to a second and more after they are let go.
    // None on the other backends.
    pub fn allocated(&self) -> Option<u64> {
        #[cfg(target_os = "macos")]
        {
            use objc2_metal::MTLDevice;
            // SAFETY: the hal device is only read, on this thread.
            let device = unsafe { self.device.as_hal::<wgpu::hal::api::Metal>() }?;
            Some(device.raw_device().currentAllocatedSize() as u64)
        }
        #[cfg(not(target_os = "macos"))]
        None
    }

    pub fn wait(&self) -> Result<(), DeviceError> {
        let polled = self.device.poll(wgpu::PollType::wait_indefinitely());
        self.check()?;
        polled.map(|_| ()).map_err(|e| DeviceError(format!("{e}")))
    }

    pub fn check(&self) -> Result<(), DeviceError> {
        let h = self.health.lock().unwrap();
        if let Some(lost) = &h.lost {
            return Err(DeviceError(lost.clone()));
        }
        if let Some(error) = &h.error {
            return Err(DeviceError(error.clone()));
        }
        Ok(())
    }
}

// The device was lost or failed: the helper cannot go on.
#[derive(Debug)]
pub struct DeviceError(pub String);

// What a request failed of: the device, or anything else.
#[derive(Debug)]
pub enum Failure {
    Device(String),
    Other(String),
}

impl From<String> for Failure {
    fn from(s: String) -> Failure {
        Failure::Other(s)
    }
}

impl From<&str> for Failure {
    fn from(s: &str) -> Failure {
        Failure::Other(s.to_string())
    }
}

impl From<DeviceError> for Failure {
    fn from(e: DeviceError) -> Failure {
        Failure::Device(e.0)
    }
}

// Instances in buffers of at most `per_buffer` each.
pub struct Batches {
    pub buffers: Vec<(wgpu::Buffer, u32)>,
    pub bytes: u64,
}

// A storage buffer of quads of `size` bytes, written by `fill` (given a
// writer of bytes at an offset) before the GPU reads it. Mapped at creation
// as a rule; on WARP (Direct3D 12's software adapter) a buffer in cached CPU
// memory instead (warp.rs: wgpu's upload memory is write-combined, which
// WARP reads at a few MB a second, so the copy of a 2 million block world's
// quads took 16 s before its first picture).
pub fn quads_buffer(gpu: &Gpu, size: u64, label: &str, fill: crate::gpu::Fill) -> wgpu::Buffer {
    #[cfg(windows)]
    if gpu.info.backend == wgpu::Backend::Dx12 && gpu.info.device_type == wgpu::DeviceType::Cpu {
        return crate::warp::quads_buffer(gpu, size, label, fill);
    }
    let usage = if gpu.direct { QUADS | wgpu::BufferUsages::MAP_WRITE } else { QUADS };
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: true });
    {
        let mut view = buffer.slice(..).get_mapped_range_mut().expect("a buffer mapped at creation");
        fill(&mut |at, data| view.slice(at..at + data.len()).copy_from_slice(data));
    }
    buffer.unmap();
    buffer
}

// A function writing a buffer's bytes through a writer of bytes at an offset.
pub type Fill<'a> = &'a mut dyn FnMut(&mut dyn FnMut(usize, &[u8]));

// What a buffer of quads is used as.
pub const QUADS: wgpu::BufferUsages = wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_SRC).union(wgpu::BufferUsages::COPY_DST);

impl Batches {
    pub fn per_buffer(gpu: &Gpu) -> usize {
        (gpu.device.limits().max_storage_buffer_binding_size.min(BATCH_BYTES) as usize / INSTANCE_BYTES).max(1)
    }

    // `count` instances from `items`, written straight into buffers.
    pub fn upload_iter<'a>(gpu: &Gpu, count: usize, items: impl Iterator<Item = &'a Instance>, label: &str) -> Batches {
        let per_buffer = Batches::per_buffer(gpu);
        let mut items = items;
        let mut buffers = Vec::new();
        let mut bytes = 0;
        let mut left = count;
        while left > 0 {
            let n = left.min(per_buffer);
            let size = (n * INSTANCE_BYTES) as u64;
            let buffer = quads_buffer(gpu, size, label, &mut |write| {
                // Through a small piece: the buffer's memory is written, never read.
                let mut piece: Vec<Instance> = Vec::with_capacity(8192);
                let mut at = 0usize;
                for k in 0..n {
                    piece.push(*items.next().expect("as many instances as counted"));
                    if piece.len() == piece.capacity() || k + 1 == n {
                        let data: &[u8] = bytemuck::cast_slice(&piece);
                        write(at, data);
                        at += data.len();
                        piece.clear();
                    }
                }
            });
            bytes += size;
            buffers.push((buffer, n as u32));
            left -= n;
        }
        Batches { buffers, bytes }
    }

    pub fn upload(gpu: &Gpu, instances: &[Instance], label: &str) -> Batches {
        Batches::upload_iter(gpu, instances.len(), instances.iter(), label)
    }
}

struct AtlasBinding {
    group: wgpu::BindGroup,
    bytes: u64,
}

fn bind_atlas(gpu: &Gpu, atlas_layout: &wgpu::BindGroupLayout, atlas: &Atlas, label: &str) -> AtlasBinding {
        let device = &gpu.device;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: atlas.side, height: atlas.side, depth_or_array_layers: 1 },
                mip_level_count: atlas.levels.len() as u32,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let mut bytes = 0;
            for (level, data) in atlas.levels.iter().enumerate() {
                let side = atlas.side >> level;
                bytes += data.len() as u64;
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    data,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side * 4), rows_per_image: Some(side) },
                    wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                );
            }
            // The block atlas: NEAREST within a level, LINEAR between levels,
            // repeating; the entity atlas: NEAREST, clamped, one level.
            let address = if atlas.mipmapped { wgpu::AddressMode::Repeat } else { wgpu::AddressMode::ClampToEdge };
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: if atlas.mipmapped { wgpu::MipmapFilterMode::Linear } else { wgpu::MipmapFilterMode::Nearest },
                address_mode_u: address,
                address_mode_v: address,
                ..Default::default()
            });
            let info = device.create_buffer(&wgpu::BufferDescriptor { label: Some("atlas info"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            gpu.queue.write_buffer(&info, 0, bytemuck::cast_slice(&[atlas.pixel[0], atlas.pixel[1], 0.0, 0.0]));
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: atlas_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: info.as_entire_binding() },
                ],
            });
            AtlasBinding { group, bytes }
        }

pub struct Renderer {
    sky: wgpu::RenderPipeline,
    blocks: wgpu::RenderPipeline,
    ordered: wgpu::RenderPipeline,
    ordered_layout: wgpu::BindGroupLayout,
    solid: wgpu::RenderPipeline,
    surface: std::sync::OnceLock<wgpu::RenderPipeline>,
    module: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    down: wgpu::RenderPipeline,
    down_layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    scene_group: wgpu::BindGroup,
    block_atlas: AtlasBinding,
    entity_atlas: std::sync::OnceLock<AtlasBinding>,
    atlas_layout: wgpu::BindGroupLayout,
    quads_layout: wgpu::BindGroupLayout,
    device: wgpu::Device,
    targets: Mutex<Vec<Targets>>,
}

// The textures and buffer one picture size draws into, kept for the next
// picture of that size (the two sizes used last, least recently used
// first).
struct Targets {
    key: (u32, u32, u32),
    ms: wgpu::TextureView,
    big: wgpu::TextureView,
    depth: wgpu::TextureView,
    small: wgpu::Texture,
    down_group: wgpu::BindGroup,
    readback: wgpu::Buffer,
    padded: u32,
}

fn texture(gpu: &Gpu, label: &str, width: u32, height: u32, format: wgpu::TextureFormat, samples: u32, usage: wgpu::TextureUsages) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: samples, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] })
}

impl Renderer {
    pub fn new(gpu: &Gpu, block_atlas: &Atlas, target: wgpu::TextureFormat) -> Renderer {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("see_build"), source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let down_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("down"), source: wgpu::ShaderSource::Wgsl(include_str!("down.wgsl").into()) });
        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry { binding, visibility, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("scene"), entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)] });
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        // The quads, read by the vertex shader from a storage buffer (six
        // vertices a quad, no instancing: software rasterisers draw
        // instances one at a time).
        let quads_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quads"),
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let storage_entry = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None };
        // The translucent quads and their order for the shot's camera.
        let ordered_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("ordered quads"), entries: &[storage_entry(0), storage_entry(1)] });
        let down_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("down"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                uniform_entry(1, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&scene_layout), Some(&atlas_layout), Some(&quads_layout)], immediate_size: 0 });
        let ordered_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&scene_layout), Some(&atlas_layout), Some(&ordered_layout)], immediate_size: 0 });
        let down_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&down_layout)], immediate_size: 0 });
        // deepslate's state (Renderer.initialize): depth LEQUAL, blending
        // SRC_ALPHA, ONE_MINUS_SRC_ALPHA on colour and alpha alike, back
        // faces culled, counter-clockwise front faces. The page's solid
        // entities draw with blending off.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
        };
        let depth = |write: bool, compare: wgpu::CompareFunction| Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(write), depth_compare: Some(compare), stencil: Default::default(), bias: Default::default() });
        let pipeline = |lay: &wgpu::PipelineLayout, label: &str, vs: &str, fs: &str, buffers: &[Option<wgpu::VertexBufferLayout>], blend: Option<wgpu::BlendState>, depth: Option<wgpu::DepthStencilState>, cull: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(lay),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState { cull_mode: cull.then_some(wgpu::Face::Back), front_face: wgpu::FrontFace::Ccw, ..Default::default() },
                depth_stencil: depth,
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(fs), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: target, blend, write_mask: wgpu::ColorWrites::ALL })] }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&scene_layout)], immediate_size: 0 });
        let sky = pipeline(&sky_layout, "sky", "vs_sky", "fs_sky", &[], Some(blend), depth(false, wgpu::CompareFunction::Always), false);
        let blocks = pipeline(&layout, "quads", "vs_quad", "fs_quad", &[], Some(blend), depth(true, wgpu::CompareFunction::LessEqual), true);
        let ordered = pipeline(&ordered_pipeline_layout, "translucent quads", "vs_ordered", "fs_quad", &[], Some(blend), depth(true, wgpu::CompareFunction::LessEqual), true);
        let solid = pipeline(&layout, "solid entities", "vs_quad", "fs_quad", &[], None, depth(true, wgpu::CompareFunction::LessEqual), true);
        let down = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("down"),
            layout: Some(&down_pipeline_layout),
            vertex: wgpu::VertexState { module: &down_module, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &down_module, entry_point: Some("fs_down"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: COLOR, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: Some("uniforms"), size: std::mem::size_of::<Uniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let scene_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &scene_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }] });
        let block_atlas = bind_atlas(gpu, &atlas_layout, block_atlas, "block atlas");
        Renderer { sky, blocks, ordered, ordered_layout, solid, surface: std::sync::OnceLock::new(), module, layout, down, down_layout, uniforms, scene_group, block_atlas, entity_atlas: std::sync::OnceLock::new(), atlas_layout, quads_layout, device: gpu.device.clone(), targets: Mutex::new(Vec::new()) }
    }

    // The entity atlas on the GPU, uploaded the first time a picture draws
    // an entity.
    pub fn entity_atlas(&self, gpu: &Gpu, atlas: &Atlas) {
        if self.entity_atlas.get().is_none() {
            let _ = self.entity_atlas.set(bind_atlas(gpu, &self.atlas_layout, atlas, "entity atlas"));
        }
    }

    // Each buffer of quads bound and drawn as six vertices a quad.
    fn draw_batches(&self, pass: &mut wgpu::RenderPass, batches: &Batches) {
        for (buffer, count) in &batches.buffers {
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("quads"), layout: &self.quads_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }] });
            pass.set_bind_group(2, &group, &[]);
            pass.draw(0..6 * *count, 0..1);
        }
    }

    // The bytes of the picture targets the given sizes would make: those
    // not kept (the resolved supersampled picture, the picture and its
    // readback, and the multisampled colour and depth where the adapter
    // does not keep them in tile memory).
    pub fn new_targets_bytes(&self, gpu: &Gpu, sizes: &[(u32, u32)], supersample: u32) -> u64 {
        let list = self.targets.lock().unwrap();
        let mut seen: Vec<(u32, u32)> = Vec::new();
        let multisampled = if gpu.adapter.get_info().transient_saves_memory == Some(true) { 0 } else { SAMPLES as u64 * 8 };
        let mut bytes = 0;
        for &(w, h) in sizes {
            if seen.contains(&(w, h)) || list.iter().any(|t| t.key == (w, h, supersample)) {
                continue;
            }
            seen.push((w, h));
            let (bw, bh) = ((w * supersample) as u64, (h * supersample) as u64);
            bytes += bw * bh * (4 + multisampled) + (w * h * 4) as u64 + ((w * 4).div_ceil(256) * 256 * h) as u64;
        }
        bytes
    }

    // Lets the targets of the picture size drawn longest ago go (a large
    // shot's are 519 MB, or 82 MB where multisampled targets stay in tile
    // memory): made again by the next picture of that size. False when none
    // are kept.
    pub fn release_oldest_targets(&self) -> bool {
        let mut list = self.targets.lock().unwrap();
        if list.is_empty() {
            return false;
        }
        list.remove(0);
        true
    }

    pub fn atlas_bytes(&self) -> u64 {
        self.block_atlas.bytes + self.entity_atlas.get().map_or(0, |a| a.bytes)
    }

    fn write_uniforms(&self, gpu: &Gpu, frame: &Frame, fog: [f32; 2]) {
        let log_depth = 2.0 / (MAX_CAMERA_DISTANCE + 1.0).log2();
        let u = Uniforms {
            view: to_f32(&frame.view),
            proj: to_f32(&frame.proj),
            sky_unproject: to_f32(&frame.sky_unproject),
            fog_color: FOG_COLOR,
            params: [log_depth as f32, if frame.ortho { 1.0 } else { 0.0 }, 0.0, 0.0],
            fog: [fog[0], fog[1], 0.0, 0.0],
        };
        gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
    }

    fn targets(&self, gpu: &Gpu, width: u32, height: u32, supersample: u32) -> usize {
        let mut list = self.targets.lock().unwrap();
        let key = (width, height, supersample);
        if let Some(i) = list.iter().position(|t| t.key == key) {
            let t = list.remove(i);
            list.push(t);
            return list.len() - 1;
        }
        let (bw, bh) = (width * supersample, height * supersample);
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        // The multisampled colour and depth live within the scene pass (on
        // Apple's GPUs in tile memory, taking no memory); the pass resolves
        // into `big`.
        let transient = attachment | wgpu::TextureUsages::TRANSIENT_ATTACHMENT;
        let ms = texture(gpu, "ms", bw, bh, COLOR, SAMPLES, transient).create_view(&Default::default());
        let big = texture(gpu, "big", bw, bh, COLOR, 1, attachment | wgpu::TextureUsages::TEXTURE_BINDING).create_view(&Default::default());
        let depth = texture(gpu, "depth", bw, bh, DEPTH, SAMPLES, transient).create_view(&Default::default());
        let small = texture(gpu, "small", width, height, COLOR, 1, attachment | wgpu::TextureUsages::COPY_SRC);
        let factor = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("factor"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        gpu.queue.write_buffer(&factor, 0, bytemuck::cast_slice(&[supersample, 0, 0, 0]));
        let down_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.down_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&big) }, wgpu::BindGroupEntry { binding: 1, resource: factor.as_entire_binding() }],
        });
        let padded = (width * 4).div_ceil(256) * 256;
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: (padded * height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        if list.len() >= 2 {
            list.remove(0);
        }
        list.push(Targets { key, ms, big, depth, small, down_group, readback, padded });
        list.len() - 1
    }

    // The scene pass into a picture size's targets: cleared, then the sky
    // (from `Upto::Sky`), then blocks and entities (`Upto::All`), resolved
    // into `big` by the hardware. Where a pixel's samples are alike (every
    // flat colour) the resolve is exact on every adapter; on an edge,
    // adapters may round the average of its samples a level apart (within
    // the across-backends thresholds over the verifier's pictures). Storing
    // the samples for the downscale to resolve instead cost a 2 million
    // block world's shot 4 to 9 ms on the M3 (its samples written out of
    // tile memory and read back).
    fn record_scene(&self, encoder: &mut wgpu::CommandEncoder, t: &Targets, draws: &Draws, upto: Upto) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("picture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &t.ms, depth_slice: None, resolve_target: Some(&t.big), ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Discard } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
            ..Default::default()
        });
        if upto == Upto::Clear {
            return;
        }
        pass.set_bind_group(0, &self.scene_group, &[]);
        pass.set_bind_group(1, &self.block_atlas.group, &[]);
        pass.set_pipeline(&self.sky);
        pass.draw(0..3, 0..1);
        if upto == Upto::Sky {
            return;
        }
        pass.set_pipeline(&self.blocks);
        for batches in &draws.blocks {
            self.draw_batches(&mut pass, batches);
        }
        if let Some(translucent) = &draws.translucent {
            pass.set_pipeline(&self.ordered);
            let groups: Vec<wgpu::BindGroup> = translucent
                .batches
                .buffers
                .iter()
                .map(|(buffer, _)| {
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("ordered quads"),
                        layout: &self.ordered_layout,
                        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: translucent.order.as_entire_binding() }],
                    })
                })
                .collect();
            for &(buffer, first, count) in &translucent.runs {
                pass.set_bind_group(2, &groups[buffer], &[]);
                pass.draw(6 * first..6 * (first + count), 0..1);
            }
        }
        if draws.solid.is_some() || draws.behind.is_some() {
            pass.set_bind_group(1, &self.entity_atlas.get().expect("the entity atlas is uploaded before entities are drawn").group, &[]);
        }
        if let Some(solid) = draws.solid {
            pass.set_pipeline(&self.solid);
            self.draw_batches(&mut pass, solid);
        }
        if let Some(behind) = &draws.behind {
            pass.set_pipeline(&self.blocks);
            self.draw_batches(&mut pass, behind);
        }
    }

    // The supersampled picture scaled down into `small`, and copied out.
    fn record_down(&self, encoder: &mut wgpu::CommandEncoder, t: &Targets) {
        let small_view = t.small.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("down"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &small_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
            ..Default::default()
        });
        pass.set_pipeline(&self.down);
        pass.set_bind_group(0, &t.down_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn record_copy(&self, encoder: &mut wgpu::CommandEncoder, t: &Targets) {
        let (width, height) = (t.key.0, t.key.1);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &t.small, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &t.readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(t.padded), rows_per_image: Some(height) } },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }

    fn read_out(&self, gpu: &Gpu, t: &Targets) -> Result<Vec<u8>, DeviceError> {
        let (width, height) = (t.key.0, t.key.1);
        let slice = t.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        gpu.wait()?;
        let row = (width * 4) as usize;
        let mut out = Vec::with_capacity(row * height as usize);
        {
            let data = slice.get_mapped_range().map_err(|e| DeviceError(format!("{e:?}")))?;
            for y in 0..height as usize {
                out.extend_from_slice(&data[y * t.padded as usize..y * t.padded as usize + row]);
            }
        }
        t.readback.unmap();
        Ok(out)
    }

    // One shot offscreen: drawn at `supersample` times the size with 4x
    // multisampling, scaled down on the GPU, read back as RGBA (alpha as
    // the page's drawing buffer holds it). `draws` are the scene's parts in
    // order.
    #[allow(clippy::too_many_arguments)]
    pub fn picture(&self, gpu: &Gpu, camera: &Camera, bounds: ([i32; 3], [i32; 3]), width: u32, height: u32, supersample: u32, draws: &Draws) -> Result<(Vec<u8>, PictureTimes), DeviceError> {
        let t0 = crate::now_ms();
        let (bw, bh) = (width * supersample, height * supersample);
        self.write_uniforms(gpu, &camera.frame(bw as f64 / bh as f64), fog_range(bounds.0, bounds.1));
        let index = self.targets(gpu, width, height, supersample);
        let list = self.targets.lock().unwrap();
        let t = &list[index];
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        self.record_scene(&mut encoder, t, draws, Upto::All);
        self.record_down(&mut encoder, t);
        self.record_copy(&mut encoder, t);
        gpu.queue.submit([encoder.finish()]);
        gpu.wait()?;
        let draw_ms = crate::now_ms() - t0;
        let t1 = crate::now_ms();
        let out = self.read_out(gpu, t)?;
        Ok((out, PictureTimes { draw_ms, readback_ms: crate::now_ms() - t1 }))
    }

    // The same picture's work submitted and waited for one part at a time,
    // each part's milliseconds by name: where a picture's time goes (the
    // shot command's --phases). The picture is the same.
    #[allow(clippy::too_many_arguments)]
    pub fn phases(&self, gpu: &Gpu, camera: &Camera, bounds: ([i32; 3], [i32; 3]), width: u32, height: u32, supersample: u32, draws: &Draws) -> Result<(Vec<u8>, Phases), DeviceError> {
        let mut out = Vec::new();
        let mut t = crate::now_ms();
        let mut lap = |name: &'static str, out: &mut Vec<(&'static str, f64)>| {
            let now = crate::now_ms();
            out.push((name, crate::round2(now - t)));
            t = now;
        };
        let (bw, bh) = (width * supersample, height * supersample);
        self.write_uniforms(gpu, &camera.frame(bw as f64 / bh as f64), fog_range(bounds.0, bounds.1));
        gpu.wait()?;
        lap("uniforms", &mut out);
        let index = self.targets(gpu, width, height, supersample);
        gpu.wait()?;
        lap("targets", &mut out);
        let list = self.targets.lock().unwrap();
        let targets = &list[index];
        for (name, upto) in [("clear and resolve", Upto::Clear), ("sky", Upto::Sky), ("scene", Upto::All)] {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            self.record_scene(&mut encoder, targets, draws, upto);
            gpu.queue.submit([encoder.finish()]);
            gpu.wait()?;
            lap(name, &mut out);
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        self.record_down(&mut encoder, targets);
        gpu.queue.submit([encoder.finish()]);
        gpu.wait()?;
        lap("downscale", &mut out);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        self.record_copy(&mut encoder, targets);
        gpu.queue.submit([encoder.finish()]);
        gpu.wait()?;
        lap("copy", &mut out);
        let pixels = self.read_out(gpu, targets)?;
        lap("map", &mut out);
        Ok((pixels, out))
    }

    // The surface each pixel shows (view depth, normal axis code), one
    // sample a pixel at the shot's size: what coverage masks compare.
    pub fn surfaces(&self, gpu: &Gpu, camera: &Camera, bounds: ([i32; 3], [i32; 3]), width: u32, height: u32, blocks: &[&Batches]) -> Result<Vec<[f32; 4]>, DeviceError> {
        self.write_uniforms(gpu, &camera.frame(width as f64 / height as f64), fog_range(bounds.0, bounds.1));
        let target = texture(gpu, "surface", width, height, SURFACE, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let depth = texture(gpu, "surface depth", width, height, DEPTH, 1, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let pipeline = self.surface.get_or_init(|| {
            gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("surface"),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState { module: &self.module, entry_point: Some("vs_quad"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), front_face: wgpu::FrontFace::Ccw, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &self.module, entry_point: Some("fs_surface"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: SURFACE, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
                multiview_mask: None,
                cache: None,
            })
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let view = target.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("surfaces"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth_view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.scene_group, &[]);
            pass.set_bind_group(1, &self.block_atlas.group, &[]);
            pass.set_pipeline(pipeline);
            for batches in blocks {
                self.draw_batches(&mut pass, batches);
            }
        }
        let padded = (width * 16).div_ceil(256) * 256;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("surface readback"), size: (padded * height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) } },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.wait()?;
        let data = buffer.slice(..).get_mapped_range().map_err(|e| DeviceError(format!("{e:?}")))?;
        let mut out = Vec::with_capacity((width * height) as usize);
        for y in 0..height as usize {
            let row: &[f32] = bytemuck::cast_slice(&data[y * padded as usize..y * padded as usize + width as usize * 16]);
            out.extend(row.as_chunks::<4>().0.iter().map(|p| [p[0], p[1], p[2], p[3]]));
        }
        Ok(out)
    }
}

// Each part of a picture's work and its milliseconds.
pub type Phases = Vec<(&'static str, f64)>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Upto {
    Clear,
    Sky,
    All,
}

// What one picture draws, in order: the blocks (block atlas), the solid
// entity quads and the shot's blended and facing ones back to front
// (entity atlas).
pub struct Draws<'a> {
    pub blocks: Vec<&'a Batches>,
    // The translucent block quads in the order for the shot's camera.
    pub translucent: Option<Ordered<'a>>,
    pub solid: Option<&'a Batches>,
    pub behind: Option<Batches>,
}

// Quads drawn in an order: `order` holds, run after run, indices into one
// of `batches`' buffers; each run is (buffer, first place in `order`,
// count).
pub struct Ordered<'a> {
    pub batches: &'a Batches,
    pub order: &'a wgpu::Buffer,
    pub runs: Vec<(usize, u32, u32)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PictureTimes {
    pub draw_ms: f64,
    pub readback_ms: f64,
}
