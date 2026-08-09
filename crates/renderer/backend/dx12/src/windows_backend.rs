use renderer::core::ResourceManager;
use renderer::rhi::types::{DeviceCapabilities, RhiError, RhiResult};

#[path = "command.rs"]
mod command;
#[path = "context.rs"]
mod context;
#[path = "device.rs"]
mod device;
#[path = "pipeline.rs"]
mod pipeline;
#[path = "shader.rs"]
mod shader;

pub(crate) const MAX_BINDINGS: u32 = 16;
// Keep push constants outside the fixed b0..b15 CBV descriptor table.
pub(crate) const PUSH_CONSTANT_REGISTER: u32 = MAX_BINDINGS;
pub(crate) const RESOURCE_DESCRIPTOR_BLOCK: u32 = MAX_BINDINGS * 3;

#[derive(Clone, Debug)]
pub struct DirectX12BackendConfig {
    pub app_name: String,
    pub enable_debug_layer: bool,
    pub allow_warp: bool,
    pub frame_count: u32,
}

impl Default for DirectX12BackendConfig {
    fn default() -> Self {
        Self {
            app_name: String::from("RustCraft"),
            enable_debug_layer: cfg!(debug_assertions),
            allow_warp: false,
            frame_count: 2,
        }
    }
}

pub struct DirectX12Backend {
    pub(crate) config: DirectX12BackendConfig,
    pub(crate) resources: ResourceManager,
    pub(crate) capabilities: DeviceCapabilities,
    pub(crate) next_submission: u64,
    pub(crate) native: Option<context::NativeState>,
}

impl DirectX12Backend {
    pub fn initialize(config: DirectX12BackendConfig) -> RhiResult<Self> {
        if config.app_name.trim().is_empty() {
            return Err(RhiError::InvalidDescription(
                "backend application name must be non-empty",
            ));
        }
        if config.frame_count < 2 {
            return Err(RhiError::InvalidDescription(
                "Direct3D 12 requires at least two swapchain buffers",
            ));
        }
        if config.frame_count > 16 {
            return Err(RhiError::InvalidDescription(
                "Direct3D 12 supports at most sixteen swapchain buffers",
            ));
        }

        let native = context::create_native_state(&config)?;
        Ok(Self {
            config,
            resources: ResourceManager::new(),
            capabilities: DeviceCapabilities {
                supports_compute: true,
                supports_storage_images: true,
                supports_hdr: true,
                supports_present: true,
                ray_tracing: false,
                compute: true,
                multi_draw_indirect: true,
                hdr_render_targets: true,
                fsr3: false,
            },
            next_submission: 0,
            native: Some(native),
        })
    }

    pub fn initialize_with_window<W>(window: &W, config: DirectX12BackendConfig) -> RhiResult<Self>
    where
        W: raw_window_handle::HasWindowHandle + ?Sized,
    {
        let mut backend = Self::initialize(config)?;
        let hwnd = context::window_handle(window)?;
        backend
            .native
            .as_mut()
            .expect("native context was created")
            .window = Some(hwnd);
        Ok(backend)
    }

    pub fn config(&self) -> &DirectX12BackendConfig {
        &self.config
    }

    pub fn resource_manager(&self) -> &ResourceManager {
        &self.resources
    }

    pub(crate) fn native_ref(&self) -> RhiResult<&context::NativeState> {
        self.native.as_ref().ok_or_else(|| {
            RhiError::Backend(String::from("Direct3D 12 native context is unavailable"))
        })
    }

    pub(crate) fn native_mut(&mut self) -> RhiResult<&mut context::NativeState> {
        self.native.as_mut().ok_or_else(|| {
            RhiError::Backend(String::from("Direct3D 12 native context is unavailable"))
        })
    }
}

pub(crate) fn backend_error(context: &str, error: impl std::fmt::Display) -> RhiError {
    RhiError::Backend(format!("{context}: {error}"))
}

pub(crate) fn checked_u32(value: usize, message: &'static str) -> RhiResult<u32> {
    u32::try_from(value).map_err(|_| RhiError::InvalidDescription(message))
}
