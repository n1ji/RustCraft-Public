use std::collections::HashMap;
use std::ffi::c_void;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::core::{Error, IUnknown, Interface, GUID, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, FreeLibrary, HANDLE, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_SHADER_MACRO,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

use renderer::rhi::command::CommandBuffer;
use renderer::rhi::types::*;

use super::{backend_error, DirectX12BackendConfig, RESOURCE_DESCRIPTOR_BLOCK};

type HResult = HRESULT;

pub(crate) type D3D12GetDebugInterfaceFn =
    unsafe extern "system" fn(riid: *const GUID, ppvdebug: *mut *mut c_void) -> HResult;

pub(crate) type D3D12CreateDeviceFn = unsafe extern "system" fn(
    adapter: *mut c_void,
    minimum_feature_level: D3D_FEATURE_LEVEL,
    riid: *const GUID,
    device: *mut *mut c_void,
) -> HResult;

pub(crate) type D3D12SerializeRootSignatureFn = unsafe extern "system" fn(
    root_signature: *const D3D12_ROOT_SIGNATURE_DESC,
    version: D3D_ROOT_SIGNATURE_VERSION,
    blob: *mut *mut c_void,
    error_blob: *mut *mut c_void,
) -> HResult;

pub(crate) type CreateDxgiFactory2Fn = unsafe extern "system" fn(
    flags: DXGI_CREATE_FACTORY_FLAGS,
    riid: *const GUID,
    factory: *mut *mut c_void,
) -> HResult;

pub(crate) type D3DCompileFn = unsafe extern "system" fn(
    source: *const c_void,
    source_size: usize,
    source_name: PCSTR,
    defines: *const D3D_SHADER_MACRO,
    include: *mut c_void,
    entry_point: PCSTR,
    target: PCSTR,
    flags1: u32,
    flags2: u32,
    code: *mut *mut c_void,
    errors: *mut *mut c_void,
) -> HResult;

pub(crate) struct NativeApi {
    d3d12_module: HMODULE,
    dxgi_module: HMODULE,
    d3dcompiler_module: HMODULE,
    pub(crate) d3d12_get_debug_interface: D3D12GetDebugInterfaceFn,
    pub(crate) d3d12_create_device: D3D12CreateDeviceFn,
    pub(crate) d3d12_serialize_root_signature: D3D12SerializeRootSignatureFn,
    pub(crate) create_dxgi_factory2: CreateDxgiFactory2Fn,
    pub(crate) d3d_compile: D3DCompileFn,
}

impl NativeApi {
    fn load() -> RhiResult<Self> {
        let mut modules = Vec::new();
        let result: RhiResult<Self> = (|| {
            let d3d12_module = load_system_library("d3d12.dll")?;
            modules.push(d3d12_module);
            let dxgi_module = load_system_library("dxgi.dll")?;
            modules.push(dxgi_module);
            let d3dcompiler_module = load_system_library("d3dcompiler_47.dll")?;
            modules.push(d3dcompiler_module);

            Ok(Self {
                d3d12_module,
                dxgi_module,
                d3dcompiler_module,
                d3d12_get_debug_interface: unsafe {
                    load_symbol(d3d12_module, "d3d12.dll", b"D3D12GetDebugInterface\0")?
                },
                d3d12_create_device: unsafe {
                    load_symbol(d3d12_module, "d3d12.dll", b"D3D12CreateDevice\0")?
                },
                d3d12_serialize_root_signature: unsafe {
                    load_symbol(d3d12_module, "d3d12.dll", b"D3D12SerializeRootSignature\0")?
                },
                create_dxgi_factory2: unsafe {
                    load_symbol(dxgi_module, "dxgi.dll", b"CreateDXGIFactory2\0")?
                },
                d3d_compile: unsafe {
                    load_symbol(d3dcompiler_module, "d3dcompiler_47.dll", b"D3DCompile\0")?
                },
            })
        })();

        if result.is_err() {
            unsafe {
                for module in modules.into_iter().rev() {
                    let _ = FreeLibrary(module);
                }
            }
        }
        result
    }
}

impl Drop for NativeApi {
    fn drop(&mut self) {
        unsafe {
            // NativeApi is the last NativeState field, so every COM object is
            // released before these module references are dropped.
            let _ = FreeLibrary(self.d3dcompiler_module);
            let _ = FreeLibrary(self.dxgi_module);
            let _ = FreeLibrary(self.d3d12_module);
        }
    }
}

fn load_system_library(name: &str) -> RhiResult<HMODULE> {
    let wide = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
            .map_err(|error| RhiError::Backend(format!("load {name}: {error}")))
    }
}

unsafe fn load_symbol<T>(module: HMODULE, library: &str, name: &'static [u8]) -> RhiResult<T> {
    let address = GetProcAddress(module, PCSTR(name.as_ptr())).ok_or_else(|| {
        let name = String::from_utf8_lossy(name)
            .trim_end_matches('\0')
            .to_string();
        RhiError::Backend(format!("{library} does not export {name}"))
    })?;
    Ok(std::mem::transmute_copy(&address))
}

fn check_hresult(result: HResult, operation: &'static str) -> RhiResult<()> {
    if result.is_ok() {
        Ok(())
    } else {
        Err(backend_error(operation, Error::from_hresult(result)))
    }
}

#[derive(Clone)]
pub(crate) struct NativeBuffer {
    pub(crate) resource: ID3D12Resource,
    pub(crate) size: u64,
    pub(crate) resource_size: u64,
    pub(crate) state: D3D12_RESOURCE_STATES,
}

#[derive(Clone)]
pub(crate) struct NativeTexture {
    pub(crate) resource: ID3D12Resource,
    pub(crate) desc: TextureDesc,
    pub(crate) state: D3D12_RESOURCE_STATES,
    pub(crate) rtv: Option<D3D12_CPU_DESCRIPTOR_HANDLE>,
    pub(crate) dsv: Option<D3D12_CPU_DESCRIPTOR_HANDLE>,
}

pub(crate) struct NativeShader {
    pub(crate) bytecode: Vec<u8>,
    pub(crate) stage: ShaderStage,
    pub(crate) interface: ShaderInterface,
}

#[derive(Clone)]
pub(crate) struct NativeSampler {
    pub(crate) desc: SamplerDesc,
}

#[derive(Clone)]
pub(crate) struct NativePipeline {
    pub(crate) pipeline_state: ID3D12PipelineState,
    pub(crate) root_signature: ID3D12RootSignature,
    pub(crate) desc: PipelineDesc,
}

#[derive(Clone)]
pub(crate) struct NativeBindGroup {
    pub(crate) desc: BindGroupDesc,
    pub(crate) resource_offset: u32,
    pub(crate) sampler_offset: u32,
}

pub(crate) struct NativeSubmission {
    pub(crate) _allocator: ID3D12CommandAllocator,
    pub(crate) command_list: ID3D12GraphicsCommandList,
}

pub(crate) struct PendingSubmission {
    pub(crate) fence_value: u64,
    pub(crate) _submission: Box<NativeSubmission>,
}

#[derive(Clone)]
pub(crate) struct DescriptorHeapState {
    pub(crate) heap: ID3D12DescriptorHeap,
    pub(crate) cpu_start: D3D12_CPU_DESCRIPTOR_HANDLE,
    pub(crate) gpu_start: Option<D3D12_GPU_DESCRIPTOR_HANDLE>,
    pub(crate) increment: u32,
    pub(crate) capacity: u32,
    pub(crate) next: u32,
}

impl DescriptorHeapState {
    pub(crate) fn cpu(&self, index: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: self.cpu_start.ptr + (u64::from(index) * u64::from(self.increment)) as usize,
        }
    }

    pub(crate) fn gpu(&self, index: u32) -> Option<D3D12_GPU_DESCRIPTOR_HANDLE> {
        self.gpu_start.map(|start| D3D12_GPU_DESCRIPTOR_HANDLE {
            ptr: start.ptr + u64::from(index) * u64::from(self.increment),
        })
    }

    pub(crate) fn allocate(&mut self, count: u32) -> RhiResult<u32> {
        let end = self
            .next
            .checked_add(count)
            .ok_or(RhiError::InvalidDescription(
                "descriptor heap allocation overflow",
            ))?;
        if end > self.capacity {
            return Err(RhiError::Backend(String::from(
                "Direct3D 12 descriptor heap is exhausted",
            )));
        }
        let start = self.next;
        self.next = end;
        Ok(start)
    }
}

pub(crate) struct SurfaceState {
    pub(crate) swapchain: IDXGISwapChain3,
    pub(crate) textures: Vec<TextureHandle>,
    pub(crate) fence_values: Vec<u64>,
    pub(crate) config: SurfaceConfig,
    pub(crate) acquired_image: Option<u32>,
    pub(crate) submitted: bool,
}

pub(crate) struct NativeState {
    pub(crate) device: ID3D12Device,
    pub(crate) queue: ID3D12CommandQueue,
    pub(crate) factory: IDXGIFactory4,
    pub(crate) fence: ID3D12Fence,
    pub(crate) fence_event: HANDLE,
    pub(crate) next_fence: u64,
    pub(crate) pending: Vec<PendingSubmission>,
    pub(crate) resource_heap: DescriptorHeapState,
    pub(crate) sampler_heap: DescriptorHeapState,
    pub(crate) rtv_heap: DescriptorHeapState,
    pub(crate) dsv_heap: DescriptorHeapState,
    pub(crate) buffers: HashMap<BufferHandle, NativeBuffer>,
    pub(crate) textures: HashMap<TextureHandle, NativeTexture>,
    pub(crate) shaders: HashMap<ShaderHandle, NativeShader>,
    pub(crate) pipelines: HashMap<PipelineHandle, NativePipeline>,
    pub(crate) samplers: HashMap<SamplerHandle, NativeSampler>,
    pub(crate) bind_groups: HashMap<BindGroupHandle, NativeBindGroup>,
    pub(crate) root_signature: ID3D12RootSignature,
    pub(crate) window: Option<HWND>,
    pub(crate) surface: Option<SurfaceState>,
    pub(crate) api: NativeApi,
}

impl Drop for NativeState {
    fn drop(&mut self) {
        let _ = self.wait_idle();
        unsafe {
            let _ = CloseHandle(self.fence_event);
        }
    }
}

impl NativeState {
    pub(crate) fn wait_idle(&mut self) -> RhiResult<()> {
        let value = self.next_fence;
        self.next_fence = self.next_fence.saturating_add(1);
        unsafe {
            self.queue
                .Signal(&self.fence, value)
                .map_err(|error| backend_error("signal Direct3D 12 idle fence", error))?;
            if self.fence.GetCompletedValue() < value {
                self.fence
                    .SetEventOnCompletion(value, self.fence_event)
                    .map_err(|error| backend_error("arm Direct3D 12 idle fence", error))?;
                let wait = WaitForSingleObject(self.fence_event, INFINITE);
                if wait.0 != 0 {
                    return Err(RhiError::Backend(format!(
                        "wait for Direct3D 12 idle fence returned {wait:?}"
                    )));
                }
            }
        }
        self.pending.clear();
        Ok(())
    }

    pub(crate) fn collect_completed(&mut self) {
        let completed = unsafe { self.fence.GetCompletedValue() };
        self.pending
            .retain(|submission| submission.fence_value > completed);
    }

    pub(crate) fn allocate_resource_descriptors(&mut self) -> RhiResult<u32> {
        self.resource_heap.allocate(RESOURCE_DESCRIPTOR_BLOCK)
    }

    pub(crate) fn allocate_sampler_descriptors(&mut self) -> RhiResult<u32> {
        self.sampler_heap.allocate(super::MAX_BINDINGS)
    }
}

pub(crate) fn window_handle<W>(window: &W) -> RhiResult<HWND>
where
    W: HasWindowHandle + ?Sized,
{
    let handle = window
        .window_handle()
        .map_err(|error| backend_error("get native window handle", error))?
        .as_raw();
    match handle {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as usize as *mut c_void)),
        _ => Err(RhiError::Backend(String::from(
            "Direct3D 12 requires a Win32 window handle",
        ))),
    }
}

pub(crate) fn create_native_state(config: &DirectX12BackendConfig) -> RhiResult<NativeState> {
    let api = NativeApi::load()?;
    unsafe {
        if config.enable_debug_layer {
            let mut debug: Option<ID3D12Debug> = None;
            let result = (api.d3d12_get_debug_interface)(
                &ID3D12Debug::IID,
                (&mut debug as *mut Option<ID3D12Debug>) as *mut *mut c_void,
            );
            if result.is_ok() {
                if let Some(debug) = debug {
                    debug.EnableDebugLayer();
                }
            }
        }

        let factory_flags = if config.enable_debug_layer {
            DXGI_CREATE_FACTORY_DEBUG
        } else {
            DXGI_CREATE_FACTORY_FLAGS(0)
        };
        let mut factory: Option<IDXGIFactory4> = None;
        check_hresult(
            (api.create_dxgi_factory2)(
                factory_flags,
                &IDXGIFactory4::IID,
                (&mut factory as *mut Option<IDXGIFactory4>) as *mut *mut c_void,
            ),
            "create DXGI factory",
        )?;
        let factory = factory
            .ok_or_else(|| RhiError::Backend(String::from("DXGI returned a null factory")))?;
        let adapter = select_adapter(&factory, config.allow_warp, api.d3d12_create_device)?;
        let adapter_unknown = adapter
            .as_ref()
            .map(|adapter| adapter.cast::<IUnknown>())
            .transpose()
            .map_err(|error| backend_error("cast Direct3D 12 adapter", error))?;
        let adapter_ptr = adapter_unknown
            .as_ref()
            .map_or(std::ptr::null_mut(), |adapter| adapter.as_raw());
        let mut device: Option<ID3D12Device> = None;
        check_hresult(
            (api.d3d12_create_device)(
                adapter_ptr,
                D3D_FEATURE_LEVEL_11_0,
                &ID3D12Device::IID,
                (&mut device as *mut Option<ID3D12Device>) as *mut *mut c_void,
            ),
            "create Direct3D 12 device",
        )?;
        let device = device
            .ok_or_else(|| RhiError::Backend(String::from("Direct3D 12 returned a null device")))?;

        let queue = device
            .CreateCommandQueue::<ID3D12CommandQueue>(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                Priority: D3D12_COMMAND_QUEUE_PRIORITY_NORMAL.0,
                Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
                NodeMask: 0,
            })
            .map_err(|error| backend_error("create Direct3D 12 command queue", error))?;
        let fence = device
            .CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE)
            .map_err(|error| backend_error("create Direct3D 12 fence", error))?;

        let resource_heap =
            create_descriptor_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 65536, true)?;
        // Shader-visible sampler heaps are limited to 2048 descriptors by
        // D3D12.  Requesting 4096 makes CreateDescriptorHeap return
        // E_INVALIDARG before the backend can finish initialization.
        let sampler_heap =
            create_descriptor_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, 2048, true)?;
        let rtv_heap =
            create_descriptor_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, 4096, false)?;
        let dsv_heap =
            create_descriptor_heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_DSV, 1024, false)?;
        let root_signature = create_root_signature(&device, api.d3d12_serialize_root_signature)?;
        let fence_event = CreateEventW(None, false, false, PCWSTR::null())
            .map_err(|error| backend_error("create Direct3D 12 fence event", error))?;

        Ok(NativeState {
            device,
            queue,
            factory,
            fence,
            fence_event,
            next_fence: 1,
            pending: Vec::new(),
            resource_heap,
            sampler_heap,
            rtv_heap,
            dsv_heap,
            buffers: HashMap::new(),
            textures: HashMap::new(),
            shaders: HashMap::new(),
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            bind_groups: HashMap::new(),
            root_signature,
            window: None,
            surface: None,
            api,
        })
    }
}

unsafe fn select_adapter(
    factory: &IDXGIFactory4,
    allow_warp: bool,
    create_device: D3D12CreateDeviceFn,
) -> RhiResult<Option<IDXGIAdapter1>> {
    let mut index = 0;
    let mut first_hardware = None;
    while let Ok(adapter) = factory.EnumAdapters1(index) {
        let desc = adapter
            .GetDesc1()
            .map_err(|error| backend_error("query DXGI adapter", error))?;
        let software = desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0;
        if !software {
            let mut test_device: Option<ID3D12Device> = None;
            let adapter_unknown = adapter
                .cast::<IUnknown>()
                .map_err(|error| backend_error("cast DXGI adapter", error))?;
            let result = create_device(
                adapter_unknown.as_raw(),
                D3D_FEATURE_LEVEL_11_0,
                &ID3D12Device::IID,
                (&mut test_device as *mut Option<ID3D12Device>) as *mut *mut c_void,
            );
            if result.is_ok() && test_device.is_some() {
                first_hardware = Some(adapter);
                break;
            }
        }
        index += 1;
    }
    if first_hardware.is_some() {
        return Ok(first_hardware);
    }
    if allow_warp {
        let warp: IDXGIAdapter = factory
            .EnumWarpAdapter()
            .map_err(|error| backend_error("select WARP adapter", error))?;
        return Ok(Some(
            warp.cast()
                .map_err(|error| backend_error("cast WARP adapter", error))?,
        ));
    }
    Err(RhiError::Backend(String::from(
        "no Direct3D 12 hardware adapter is available",
    )))
}

unsafe fn create_descriptor_heap(
    device: &ID3D12Device,
    heap_type: D3D12_DESCRIPTOR_HEAP_TYPE,
    capacity: u32,
    shader_visible: bool,
) -> RhiResult<DescriptorHeapState> {
    let flags = if shader_visible {
        D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
    } else {
        D3D12_DESCRIPTOR_HEAP_FLAG_NONE
    };
    let heap = device
        .CreateDescriptorHeap::<ID3D12DescriptorHeap>(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: heap_type,
            NumDescriptors: capacity,
            Flags: flags,
            NodeMask: 0,
        })
        .map_err(|error| backend_error("create Direct3D 12 descriptor heap", error))?;
    let gpu_start = shader_visible.then(|| heap.GetGPUDescriptorHandleForHeapStart());
    Ok(DescriptorHeapState {
        cpu_start: heap.GetCPUDescriptorHandleForHeapStart(),
        gpu_start,
        increment: device.GetDescriptorHandleIncrementSize(heap_type),
        capacity,
        next: 0,
        heap,
    })
}

pub(crate) fn debug_layer_messages(device: &ID3D12Device) -> Vec<String> {
    let Ok(info_queue) = device.cast::<ID3D12InfoQueue>() else {
        return Vec::new();
    };

    let count = unsafe { info_queue.GetNumStoredMessages() };
    let mut messages = Vec::new();
    for index in 0..count {
        let mut byte_length = 0usize;
        if unsafe { info_queue.GetMessage(index, None, &mut byte_length) }.is_err()
            || byte_length == 0
        {
            continue;
        }

        // The returned D3D12_MESSAGE contains pointers into the same buffer,
        // so keep the allocation alive while copying its description.
        let word_count = byte_length.div_ceil(std::mem::size_of::<u64>());
        let mut storage = vec![0u64; word_count];
        let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        if unsafe { info_queue.GetMessage(index, Some(message), &mut byte_length) }.is_err() {
            continue;
        }

        let message = unsafe { &*message };
        if message.pDescription.is_null() || message.DescriptionByteLength == 0 {
            continue;
        }
        let description = unsafe {
            std::slice::from_raw_parts(message.pDescription, message.DescriptionByteLength)
        };
        messages.push(
            String::from_utf8_lossy(description)
                .trim_end_matches('\0')
                .to_string(),
        );
    }
    messages
}

unsafe fn create_root_signature(
    device: &ID3D12Device,
    serialize_root_signature: D3D12SerializeRootSignatureFn,
) -> RhiResult<ID3D12RootSignature> {
    let ranges = [
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
            NumDescriptors: super::MAX_BINDINGS,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        },
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: super::MAX_BINDINGS,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: super::MAX_BINDINGS,
        },
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
            NumDescriptors: super::MAX_BINDINGS,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: super::MAX_BINDINGS * 2,
        },
    ];
    let sampler_range = D3D12_DESCRIPTOR_RANGE {
        RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,
        NumDescriptors: super::MAX_BINDINGS,
        BaseShaderRegister: 0,
        RegisterSpace: 0,
        OffsetInDescriptorsFromTableStart: 0,
    };
    let parameters = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: ranges.len() as u32,
                    pDescriptorRanges: ranges.as_ptr(),
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: &sampler_range,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Constants: D3D12_ROOT_CONSTANTS {
                    ShaderRegister: super::PUSH_CONSTANT_REGISTER,
                    RegisterSpace: 0,
                    Num32BitValues: 16,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
    ];
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: parameters.len() as u32,
        pParameters: parameters.as_ptr(),
        NumStaticSamplers: 0,
        pStaticSamplers: std::ptr::null(),
        Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
    };
    let mut blob: Option<ID3DBlob> = None;
    let mut error_blob: Option<ID3DBlob> = None;
    let result = serialize_root_signature(
        &desc,
        D3D_ROOT_SIGNATURE_VERSION_1,
        (&mut blob as *mut Option<ID3DBlob>) as *mut *mut c_void,
        (&mut error_blob as *mut Option<ID3DBlob>) as *mut *mut c_void,
    );
    if result.is_err() {
        let message = error_blob
            .as_ref()
            .map(|blob| unsafe { blob_message(blob) })
            .unwrap_or_else(|| Error::from_hresult(result).to_string());
        return Err(backend_error(
            "serialize Direct3D 12 root signature",
            message,
        ));
    }
    let blob = blob.ok_or_else(|| {
        RhiError::Backend(String::from(
            "Direct3D 12 root-signature serialization returned no blob",
        ))
    })?;
    let bytes =
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
    device
        .CreateRootSignature::<ID3D12RootSignature>(0, bytes)
        .map_err(|error| backend_error("create Direct3D 12 root signature", error))
}

unsafe fn blob_message(blob: &ID3DBlob) -> String {
    let bytes =
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_string()
}

pub(crate) fn resource_state(state: ResourceState) -> D3D12_RESOURCE_STATES {
    match state {
        ResourceState::Undefined | ResourceState::Present => D3D12_RESOURCE_STATE_COMMON,
        ResourceState::CopySrc => D3D12_RESOURCE_STATE_COPY_SOURCE,
        ResourceState::CopyDst => D3D12_RESOURCE_STATE_COPY_DEST,
        ResourceState::VertexBuffer | ResourceState::UniformBuffer => {
            D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER
        }
        ResourceState::IndexBuffer => D3D12_RESOURCE_STATE_INDEX_BUFFER,
        ResourceState::StorageRead | ResourceState::StorageWrite => {
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS
        }
        ResourceState::ShaderRead => D3D12_RESOURCE_STATE_ALL_SHADER_RESOURCE,
        ResourceState::ColorAttachment => D3D12_RESOURCE_STATE_RENDER_TARGET,
        ResourceState::DepthAttachment => D3D12_RESOURCE_STATE_DEPTH_WRITE,
    }
}

pub(crate) fn texture_format(format: TextureFormat) -> RhiResult<DXGI_FORMAT> {
    Ok(match format {
        TextureFormat::R8Unorm => DXGI_FORMAT_R8_UNORM,
        TextureFormat::R8G8B8A8Unorm | TextureFormat::Rgba8Unorm => DXGI_FORMAT_R8G8B8A8_UNORM,
        TextureFormat::R8G8B8A8Srgb | TextureFormat::Rgba8Srgb => DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        TextureFormat::B8G8R8A8Srgb => DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        TextureFormat::R16G16Float => DXGI_FORMAT_R16G16_FLOAT,
        TextureFormat::R16Float => DXGI_FORMAT_R16_FLOAT,
        TextureFormat::R16G16B16A16Float | TextureFormat::Rgba16Float => {
            DXGI_FORMAT_R16G16B16A16_FLOAT
        }
        TextureFormat::D24UnormS8Uint => DXGI_FORMAT_D24_UNORM_S8_UINT,
        TextureFormat::D32Float => DXGI_FORMAT_D32_FLOAT,
    })
}

pub(crate) fn swapchain_format(format: TextureFormat) -> RhiResult<DXGI_FORMAT> {
    Ok(match format {
        // Flip-model swapchains accept the UNORM variant at creation time.
        // The corresponding SRGB RTV is created below so rendering keeps the
        // requested color-space conversion.
        TextureFormat::R8G8B8A8Srgb | TextureFormat::Rgba8Srgb => DXGI_FORMAT_R8G8B8A8_UNORM,
        TextureFormat::B8G8R8A8Srgb => DXGI_FORMAT_B8G8R8A8_UNORM,
        _ => texture_format(format)?,
    })
}

pub(crate) fn texture_resource_desc(desc: &TextureDesc) -> RhiResult<D3D12_RESOURCE_DESC> {
    if desc.extent.is_empty() || desc.mip_levels == 0 || desc.array_layers == 0 {
        return Err(RhiError::InvalidDescription(
            "Direct3D 12 texture extent and subresources must be non-zero",
        ));
    }
    Ok(D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: u64::from(desc.extent.width),
        Height: desc.extent.height,
        DepthOrArraySize: u16::try_from(desc.array_layers)
            .map_err(|_| RhiError::InvalidDescription("texture array is too large"))?,
        MipLevels: u16::try_from(desc.mip_levels)
            .map_err(|_| RhiError::InvalidDescription("texture mip count is too large"))?,
        Format: texture_format(desc.format)?,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: (if desc.usage.contains(TextureUsage::STORAGE) {
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
        } else {
            D3D12_RESOURCE_FLAG_NONE
        }) | if desc.usage.contains(TextureUsage::COLOR_ATTACHMENT) {
            D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
        } else if desc.usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
            D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL
        } else {
            D3D12_RESOURCE_FLAG_NONE
        },
    })
}

pub(crate) fn resource_label(label: &str) -> Option<Vec<u16>> {
    if label.is_empty() {
        return None;
    }
    Some(label.encode_utf16().chain(std::iter::once(0)).collect())
}

pub(crate) fn set_resource_name(resource: &ID3D12Resource, label: &str) {
    if let Some(name) = resource_label(label) {
        unsafe {
            let _ = resource.SetName(PCWSTR(name.as_ptr()));
        }
    }
}

pub(crate) fn aligned_constant_size(size: u64) -> RhiResult<u32> {
    let size = size.max(1);
    let aligned = size.checked_add(255).ok_or(RhiError::InvalidDescription(
        "constant buffer size overflow",
    ))? & !255;
    u32::try_from(aligned).map_err(|_| RhiError::InvalidDescription("constant buffer is too large"))
}

pub(crate) fn create_committed_resource(
    device: &ID3D12Device,
    desc: &D3D12_RESOURCE_DESC,
    initial_state: D3D12_RESOURCE_STATES,
    clear_value: Option<&D3D12_CLEAR_VALUE>,
) -> RhiResult<ID3D12Resource> {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
    };
    let mut resource = None;
    unsafe {
        device
            .CreateCommittedResource::<ID3D12Resource>(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                desc,
                initial_state,
                clear_value.map(std::ptr::from_ref),
                &mut resource,
            )
            .map_err(|error| backend_error("create Direct3D 12 resource", error))?;
    }
    resource.ok_or_else(|| RhiError::Backend(String::from("Direct3D 12 returned a null resource")))
}

pub(crate) fn create_upload_buffer(
    device: &ID3D12Device,
    size: u64,
    label: &str,
) -> RhiResult<ID3D12Resource> {
    if size == 0 {
        return Err(RhiError::InvalidDescription("buffer size must be non-zero"));
    }
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Alignment: 0,
        Width: size,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: D3D12_RESOURCE_FLAG_NONE,
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_UPLOAD,
        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
    };
    let mut resource = None;
    unsafe {
        device
            .CreateCommittedResource::<ID3D12Resource>(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut resource,
            )
            .map_err(|error| backend_error("create Direct3D 12 upload buffer", error))?;
    }
    let resource = resource.ok_or_else(|| {
        RhiError::Backend(String::from("Direct3D 12 returned a null upload buffer"))
    })?;
    set_resource_name(&resource, label);
    Ok(resource)
}

pub(crate) fn command_buffer_token(command_buffer: &CommandBuffer) -> RhiResult<u64> {
    command_buffer.native().ok_or_else(|| {
        RhiError::Backend(String::from(
            "Direct3D 12 command buffer has no native command list",
        ))
    })
}
