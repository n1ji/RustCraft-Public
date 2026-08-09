use renderer::rhi::types::{RhiError, RhiResult};

/// Configuration is available on every target so callers can keep their
/// backend selection code platform-neutral.
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

/// Direct3D 12 is only exposed by this crate on Windows.
pub struct DirectX12Backend;

impl DirectX12Backend {
    pub fn initialize(_config: DirectX12BackendConfig) -> RhiResult<Self> {
        Err(RhiError::Backend(String::from(
            "Direct3D 12 is only available on Windows",
        )))
    }

    pub fn initialize_with_window<W>(
        _window: &W,
        _config: DirectX12BackendConfig,
    ) -> RhiResult<Self> {
        Err(RhiError::Backend(String::from(
            "Direct3D 12 is only available on Windows",
        )))
    }
}
