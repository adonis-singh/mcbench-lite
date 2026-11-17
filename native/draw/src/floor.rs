// The software floor on Linux (native/draw-floor/README.md): SwiftShader's
// Vulkan driver, shipped beside the helper, opened directly (no Vulkan
// loader, no other driver, no environment variable): its own
// vkGetInstanceProcAddr gives the entry points, and wgpu-hal takes the
// instance made on it. The `unsafe` here is the only unsafe code of the
// crate: loading a library and handing wgpu-hal a raw Vulkan instance.

use std::path::Path;

pub fn direct_vulkan(driver: &Path) -> Result<wgpu::Instance, String> {
    use ash::vk;
    use wgpu::hal::vulkan::Instance as HalInstance;
    let name = driver.display();
    // SAFETY: the library is the shipped Vulkan driver, loaded once; ash
    // keeps it loaded for as long as the entry (moved into the instance).
    let entry = unsafe { ash::Entry::load_from(driver) }.map_err(|e| format!("No graphics adapter: {name}: {e}"))?;
    // SAFETY: a call through the entry points just loaded.
    let version = unsafe { entry.try_enumerate_instance_version() }.map_err(|e| format!("No graphics adapter: {name}: vkEnumerateInstanceVersion: {e}"))?.unwrap_or(vk::API_VERSION_1_0);
    let flags = wgpu::InstanceFlags::from_build_config().with_env();
    let extensions = HalInstance::desired_extensions(&entry, version, flags).map_err(|e| format!("No graphics adapter: {name}: {e}"))?;
    let pointers: Vec<*const std::ffi::c_char> = extensions.iter().map(|e| e.as_ptr()).collect();
    let app = vk::ApplicationInfo::default().application_name(c"mcbench-draw").engine_name(c"wgpu-hal").api_version(if version < vk::API_VERSION_1_1 { vk::API_VERSION_1_0 } else { vk::API_VERSION_1_3 });
    // SAFETY: the create info and the extension names outlive the call.
    let raw = unsafe { entry.create_instance(&vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&pointers), None) }.map_err(|e| format!("No graphics adapter: {name}: vkCreateInstance: {e}"))?;
    // SAFETY: `raw` was made on `entry` with `extensions` enabled, at
    // `version`; wgpu-hal owns it from here and destroys it.
    let hal = unsafe { HalInstance::from_raw(entry, raw, version, 0, None, extensions, flags, Default::default(), false, None) }.map_err(|e| format!("No graphics adapter: {name}: {e}"))?;
    // SAFETY: a Vulkan hal instance for wgpu's Vulkan backend.
    Ok(unsafe { wgpu::Instance::from_hal::<wgpu::hal::api::Vulkan>(hal) })
}
