//! Direct3D 12 implementation of RustCraft's renderer RHI.
//!
//! The backend deliberately keeps the public surface identical to the Vulkan
//! backend.  Built-in GLSL modules are compiled to SPIR-V first and lowered to
//! Shader Model 5.1 HLSL at this boundary, so the game and renderer core do
//! not need a second shader or resource-binding vocabulary.

#[cfg(windows)]
mod windows_backend;

#[cfg(not(windows))]
mod unsupported;

#[cfg(windows)]
pub use windows_backend::{DirectX12Backend, DirectX12BackendConfig};

#[cfg(not(windows))]
pub use unsupported::{DirectX12Backend, DirectX12BackendConfig};
