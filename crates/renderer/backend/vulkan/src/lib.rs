pub mod shader;

mod command;
mod context;
mod device;
mod pipeline;
mod resources;
mod swapchain;

use ash::Entry;

use renderer::core::ResourceManager;
use renderer::rhi::types::DeviceCapabilities;

use context::NativeState;

#[derive(Clone, Debug)]
pub struct VulkanBackendConfig {
    pub app_name: String,
    pub enable_validation: bool,
}

impl Default for VulkanBackendConfig {
    fn default() -> Self {
        Self {
            app_name: String::from("RustCraft"),
            // Validation is an explicit diagnostic mode. Applications that
            // need it should set this field (the game exposes the same mode
            // through RUSTCRAFT_VULKAN_VALIDATION=1).
            enable_validation: false,
        }
    }
}

pub struct VulkanBackend {
    entry: Entry,
    config: VulkanBackendConfig,
    resources: ResourceManager,
    capabilities: DeviceCapabilities,
    next_submission: u64,
    native: Option<NativeState>,
}
