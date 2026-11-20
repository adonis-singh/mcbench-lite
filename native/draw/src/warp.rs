// Buffers of quads for WARP, Direct3D 12's software adapter (gpu.rs
// quads_buffer): a committed buffer in a custom heap of cached
// (write-back) CPU memory, written through its own mapping, then given to
// wgpu. WARP reads wgpu's upload memory (write-combined) at a few MB a
// second in the Windows VM: the copy of pagoda2m's 130 MB of quads into
// GPU memory took 16 to 18 s before its first picture; drawing from
// cached memory needs no copy. The `unsafe` here is Direct3D 12 through
// wgpu-hal.

use windows::Win32::Graphics::Direct3D12 as d3d;
use windows::Win32::Graphics::Dxgi::Common as dxgi;

pub fn quads_buffer(
    gpu: &crate::gpu::Gpu,
    size: u64,
    label: &str,
    fill: crate::gpu::Fill,
) -> wgpu::Buffer {
    // SAFETY: the device is Direct3D 12 (checked by the caller); the raw
    // device is only used to create one resource here.
    let hal = unsafe { gpu.device.as_hal::<wgpu::hal::api::Dx12>() }.expect("a Direct3D 12 device");
    let raw = hal.raw_device();
    let heap = d3d::D3D12_HEAP_PROPERTIES {
        Type: d3d::D3D12_HEAP_TYPE_CUSTOM,
        CPUPageProperty: d3d::D3D12_CPU_PAGE_PROPERTY_WRITE_BACK,
        MemoryPoolPreference: d3d::D3D12_MEMORY_POOL_L0,
        CreationNodeMask: 0,
        VisibleNodeMask: 0,
    };
    let desc = d3d::D3D12_RESOURCE_DESC {
        Dimension: d3d::D3D12_RESOURCE_DIMENSION_BUFFER,
        Alignment: 0,
        Width: size,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: dxgi::DXGI_FORMAT_UNKNOWN,
        SampleDesc: dxgi::DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: d3d::D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: d3d::D3D12_RESOURCE_FLAG_NONE,
    };
    let mut resource: Option<d3d::ID3D12Resource> = None;
    // SAFETY: valid descriptions, an out pointer to an Option of the interface.
    unsafe {
        raw.CreateCommittedResource(
            &heap,
            d3d::D3D12_HEAP_FLAG_NONE,
            &desc,
            d3d::D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )
    }
    .expect("a committed buffer in cached memory");
    let resource = resource.expect("CreateCommittedResource gives the resource");
    let mut pointer = std::ptr::null_mut();
    // SAFETY: subresource 0 of a buffer in a CPU-visible heap.
    unsafe { resource.Map(0, None, Some(&mut pointer)) }.expect("the buffer maps");
    // SAFETY: the mapping covers the buffer's `size` bytes until Unmap,
    // and nothing else uses the buffer meanwhile.
    let bytes = unsafe { std::slice::from_raw_parts_mut(pointer as *mut u8, size as usize) };
    fill(&mut |at, data| bytes[at..at + data.len()].copy_from_slice(data));
    // SAFETY: the mapping made above.
    unsafe { resource.Unmap(0, None) };
    drop(hal);
    // SAFETY: a buffer resource of `size` bytes on this device.
    let hal_buffer = unsafe { wgpu::hal::dx12::Device::buffer_from_raw(resource, size) };
    // SAFETY: the hal buffer was made on this device, as the descriptor says.
    unsafe {
        gpu.device.create_buffer_from_hal::<wgpu::hal::api::Dx12>(
            hal_buffer,
            &wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: crate::gpu::QUADS,
                mapped_at_creation: false,
            },
        )
    }
}
