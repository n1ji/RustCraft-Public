use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::{Arc, RwLock};

use ash::{vk, Entry};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use renderer::rhi::types::*;

use super::VulkanBackendConfig;

pub(crate) struct NativeBuffer {
    pub(crate) buffer: vk::Buffer,
    pub(crate) allocation: gpu_allocator::vulkan::Allocation,
}

pub(crate) struct NativeTexture {
    pub(crate) image: vk::Image,
    pub(crate) view: vk::ImageView,
    pub(crate) allocation: Option<gpu_allocator::vulkan::Allocation>,
    pub(crate) owns_view: bool,
    pub(crate) layout: vk::ImageLayout,
}

pub(crate) struct NativeShader {
    pub(crate) module: vk::ShaderModule,
    pub(crate) entry_point: CString,
    pub(crate) stage: ShaderStage,
    pub(crate) interface: ShaderInterface,
}

pub(crate) struct NativeSampler {
    pub(crate) sampler: vk::Sampler,
}

#[derive(Clone)]
pub(crate) struct NativeBindGroup {
    pub(crate) descriptor_set: vk::DescriptorSet,
    pub(crate) pipeline: PipelineHandle,
    pub(crate) set: u32,
}

#[derive(Clone)]
pub(crate) struct NativePipeline {
    pub(crate) pipeline: vk::Pipeline,
    pub(crate) layout: vk::PipelineLayout,
    pub(crate) set_layouts: Vec<vk::DescriptorSetLayout>,
    pub(crate) bind_point: vk::PipelineBindPoint,
}

/// Immutable native handles used while recording a command buffer.
///
/// The old encoder cloned every native resource map on every frame.  These
/// tables are copy-on-write instead: command encoders only clone the outer
/// `Arc`, while resource creation/destruction updates the affected table.
/// Texture layouts are kept separately because barriers mutate them after a
/// submission completes.
#[derive(Clone, Default)]
pub(crate) struct NativeResourceTables {
    pub(crate) texture_images: Arc<HashMap<TextureHandle, vk::Image>>,
    pub(crate) texture_views: Arc<HashMap<TextureHandle, vk::ImageView>>,
    /// Layouts change after every submitted barrier. Keep this table behind a
    /// lock so tracking a frame does not clone the entire texture map.
    pub(crate) texture_layouts: Arc<RwLock<HashMap<TextureHandle, vk::ImageLayout>>>,
    pub(crate) texture_descs: Arc<HashMap<TextureHandle, TextureDesc>>,
    pub(crate) buffer_objects: Arc<HashMap<BufferHandle, vk::Buffer>>,
    pub(crate) buffer_sizes: Arc<HashMap<BufferHandle, u64>>,
    pub(crate) pipelines: Arc<HashMap<PipelineHandle, NativePipeline>>,
    pub(crate) bind_groups: Arc<HashMap<BindGroupHandle, NativeBindGroup>>,
}

pub(crate) struct PendingSubmission {
    pub(crate) submission_id: u64,
    pub(crate) frame_slot: usize,
    /// Buffers replaced after this submission was queued stay alive until the
    /// fence signals.  This avoids a device-wide idle wait for dynamic meshes.
    pub(crate) deferred_buffers: Vec<NativeBuffer>,
}

pub(crate) struct PendingBufferUpload {
    pub(crate) staging: NativeBuffer,
    pub(crate) staging_handle: BufferHandle,
    pub(crate) destination: BufferHandle,
    pub(crate) destination_offset: u64,
    pub(crate) size: u64,
}

pub(crate) struct FrameSlot {
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) fence: vk::Fence,
    pub(crate) image_available: vk::Semaphore,
    pub(crate) in_flight: bool,
}

pub(crate) struct NativeState {
    pub(crate) instance: Option<ash::Instance>,
    pub(crate) physical_device: vk::PhysicalDevice,
    pub(crate) surface_loader: Option<ash::khr::surface::Instance>,
    pub(crate) surface: Option<vk::SurfaceKHR>,
    pub(crate) owns_instance: bool,
    pub(crate) owns_surface: bool,
    pub(crate) owns_device: bool,
    pub(crate) owns_command_pool: bool,
    pub(crate) device: ash::Device,
    pub(crate) queue: vk::Queue,
    pub(crate) command_pool: vk::CommandPool,
    pub(crate) descriptor_pool: vk::DescriptorPool,
    pub(crate) allocator: Option<gpu_allocator::vulkan::Allocator>,
    pub(crate) buffers: HashMap<BufferHandle, NativeBuffer>,
    pub(crate) textures: HashMap<TextureHandle, NativeTexture>,
    pub(crate) shaders: HashMap<ShaderHandle, NativeShader>,
    pub(crate) pipelines: HashMap<PipelineHandle, NativePipeline>,
    pub(crate) samplers: HashMap<SamplerHandle, NativeSampler>,
    pub(crate) bind_groups: HashMap<BindGroupHandle, NativeBindGroup>,
    pub(crate) resource_tables: Arc<NativeResourceTables>,
    pub(crate) pending_submissions: Vec<PendingSubmission>,
    pub(crate) pending_buffer_uploads: Vec<PendingBufferUpload>,
    pub(crate) active_deferred_buffers: Vec<NativeBuffer>,
    pub(crate) frame_slots: Vec<FrameSlot>,
    pub(crate) next_frame_slot: usize,
    pub(crate) swapchain_loader: Option<ash::khr::swapchain::Device>,
    pub(crate) swapchain: Option<vk::SwapchainKHR>,
    pub(crate) swapchain_format: Option<TextureFormat>,
    pub(crate) swapchain_extent: vk::Extent2D,
    pub(crate) swapchain_config: Option<SurfaceConfig>,
    pub(crate) swapchain_textures: Vec<TextureHandle>,
    pub(crate) swapchain_render_finished: Vec<vk::Semaphore>,
    pub(crate) acquired_image: Option<u32>,
    pub(crate) acquired_frame_slot: Option<usize>,
    pub(crate) acquired_image_submitted: bool,
}

impl NativeState {
    pub(crate) fn allocator_mut(&mut self) -> &mut gpu_allocator::vulkan::Allocator {
        self.allocator
            .as_mut()
            .expect("native Vulkan state always owns an allocator")
    }

    pub(crate) fn destroy_buffer_resource(&mut self, resource: NativeBuffer) -> RhiResult<()> {
        unsafe {
            self.device.destroy_buffer(resource.buffer, None);
        }
        self.allocator_mut()
            .free(resource.allocation)
            .map_err(|error| RhiError::Backend(format!("free Vulkan buffer: {error:?}")))
    }

    pub(crate) fn add_texture_table(
        &mut self,
        handle: TextureHandle,
        image: vk::Image,
        view: vk::ImageView,
        layout: vk::ImageLayout,
        desc: TextureDesc,
    ) {
        let tables = Arc::make_mut(&mut self.resource_tables);
        Arc::make_mut(&mut tables.texture_images).insert(handle, image);
        Arc::make_mut(&mut tables.texture_views).insert(handle, view);
        tables
            .texture_layouts
            .write()
            .expect("Vulkan texture layout table is not poisoned")
            .insert(handle, layout);
        Arc::make_mut(&mut tables.texture_descs).insert(handle, desc);
    }

    pub(crate) fn remove_texture_table(&mut self, handle: TextureHandle) {
        let tables = Arc::make_mut(&mut self.resource_tables);
        Arc::make_mut(&mut tables.texture_images).remove(&handle);
        Arc::make_mut(&mut tables.texture_views).remove(&handle);
        tables
            .texture_layouts
            .write()
            .expect("Vulkan texture layout table is not poisoned")
            .remove(&handle);
        Arc::make_mut(&mut tables.texture_descs).remove(&handle);
    }

    pub(crate) fn update_texture_layout(&mut self, handle: TextureHandle, layout: vk::ImageLayout) {
        self.resource_tables
            .texture_layouts
            .write()
            .expect("Vulkan texture layout table is not poisoned")
            .insert(handle, layout);
    }

    pub(crate) fn add_buffer_table(&mut self, handle: BufferHandle, buffer: vk::Buffer, size: u64) {
        let tables = Arc::make_mut(&mut self.resource_tables);
        Arc::make_mut(&mut tables.buffer_objects).insert(handle, buffer);
        Arc::make_mut(&mut tables.buffer_sizes).insert(handle, size);
    }

    pub(crate) fn remove_buffer_table(&mut self, handle: BufferHandle) {
        let tables = Arc::make_mut(&mut self.resource_tables);
        Arc::make_mut(&mut tables.buffer_objects).remove(&handle);
        Arc::make_mut(&mut tables.buffer_sizes).remove(&handle);
    }

    pub(crate) fn add_pipeline_table(&mut self, handle: PipelineHandle, pipeline: NativePipeline) {
        Arc::make_mut(&mut Arc::make_mut(&mut self.resource_tables).pipelines)
            .insert(handle, pipeline);
    }

    pub(crate) fn remove_pipeline_table(&mut self, handle: PipelineHandle) {
        Arc::make_mut(&mut Arc::make_mut(&mut self.resource_tables).pipelines).remove(&handle);
    }

    pub(crate) fn add_bind_group_table(
        &mut self,
        handle: BindGroupHandle,
        bind_group: NativeBindGroup,
    ) {
        Arc::make_mut(&mut Arc::make_mut(&mut self.resource_tables).bind_groups)
            .insert(handle, bind_group);
    }

    pub(crate) fn remove_bind_group_table(&mut self, handle: BindGroupHandle) {
        Arc::make_mut(&mut Arc::make_mut(&mut self.resource_tables).bind_groups).remove(&handle);
    }
}

impl Drop for NativeState {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
        }

        if let (Some(loader), Some(swapchain)) =
            (self.swapchain_loader.take(), self.swapchain.take())
        {
            unsafe {
                loader.destroy_swapchain(swapchain, None);
            }
        }
        unsafe {
            for semaphore in self.swapchain_render_finished.drain(..) {
                self.device.destroy_semaphore(semaphore, None);
            }
        }

        for (_, resource) in self.buffers.drain() {
            unsafe {
                self.device.destroy_buffer(resource.buffer, None);
            }
            if let Some(allocator) = self.allocator.as_mut() {
                let _ = allocator.free(resource.allocation);
            }
        }

        for (_, resource) in self.textures.drain() {
            if resource.owns_view {
                unsafe {
                    self.device.destroy_image_view(resource.view, None);
                }
            }
            if let Some(allocation) = resource.allocation {
                unsafe {
                    self.device.destroy_image(resource.image, None);
                }
                if let Some(allocator) = self.allocator.as_mut() {
                    let _ = allocator.free(allocation);
                }
            }
        }

        for (_, resource) in self.pipelines.drain() {
            unsafe {
                self.device.destroy_pipeline(resource.pipeline, None);
                self.device.destroy_pipeline_layout(resource.layout, None);
                for set_layout in resource.set_layouts {
                    self.device.destroy_descriptor_set_layout(set_layout, None);
                }
            }
        }

        for (_, resource) in self.shaders.drain() {
            unsafe {
                self.device.destroy_shader_module(resource.module, None);
            }
        }

        for (_, resource) in self.samplers.drain() {
            unsafe {
                self.device.destroy_sampler(resource.sampler, None);
            }
        }

        self.bind_groups.clear();
        unsafe {
            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
        }

        if !self.pending_submissions.is_empty() {
            unsafe {
                for submission in self.pending_submissions.drain(..) {
                    for resource in submission.deferred_buffers {
                        self.device.destroy_buffer(resource.buffer, None);
                        if let Some(allocator) = self.allocator.as_mut() {
                            let _ = allocator.free(resource.allocation);
                        }
                    }
                }
            }
        }

        let pending_uploads = std::mem::take(&mut self.pending_buffer_uploads);
        for upload in pending_uploads {
            let _ = self.destroy_buffer_resource(upload.staging);
        }
        let active_deferred_buffers = std::mem::take(&mut self.active_deferred_buffers);
        for resource in active_deferred_buffers {
            let _ = self.destroy_buffer_resource(resource);
        }

        unsafe {
            for slot in self.frame_slots.drain(..) {
                self.device
                    .free_command_buffers(self.command_pool, &[slot.command_buffer]);
                self.device.destroy_fence(slot.fence, None);
                self.device.destroy_semaphore(slot.image_available, None);
            }
        }

        if self.owns_command_pool {
            unsafe {
                self.device.destroy_command_pool(self.command_pool, None);
            }
        }

        // gpu-allocator releases device memory in its Drop implementation, so
        // it must be dropped before an owned VkDevice is destroyed.
        drop(self.allocator.take());

        if self.owns_device {
            unsafe {
                self.device.destroy_device(None);
            }
        }

        if self.owns_surface {
            if let (Some(surface_loader), Some(surface)) =
                (self.surface_loader.as_ref(), self.surface.take())
            {
                unsafe {
                    surface_loader.destroy_surface(surface, None);
                }
            }
        }

        if self.owns_instance {
            if let Some(instance) = self.instance.take() {
                unsafe {
                    instance.destroy_instance(None);
                }
            }
        }
    }
}

pub(crate) fn create_descriptor_pool(
    device: &ash::Device,
    include_acceleration_structure: bool,
) -> RhiResult<vk::DescriptorPool> {
    let mut pool_sizes = vec![
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(4096),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(4096),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(4096),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_IMAGE)
            .descriptor_count(4096),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::SAMPLER)
            .descriptor_count(4096),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(4096),
    ];
    if include_acceleration_structure {
        pool_sizes.push(
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                .descriptor_count(256),
        );
    }
    unsafe {
        device
            .create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                    .max_sets(4096)
                    .pool_sizes(&pool_sizes),
                None,
            )
            .map_err(|error| RhiError::Backend(format!("create Vulkan descriptor pool: {error:?}")))
    }
}

pub(crate) fn create_frame_slots(
    device: &ash::Device,
    command_pool: vk::CommandPool,
) -> RhiResult<Vec<FrameSlot>> {
    const FRAME_SLOT_COUNT: usize = 3;
    let command_buffers = unsafe {
        device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(FRAME_SLOT_COUNT as u32),
            )
            .map_err(|error| {
                RhiError::Backend(format!("allocate Vulkan frame command buffers: {error:?}"))
            })?
    };
    let mut slots: Vec<FrameSlot> = Vec::with_capacity(FRAME_SLOT_COUNT);
    for &command_buffer in &command_buffers {
        let image_available =
            match unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) } {
                Ok(semaphore) => semaphore,
                Err(error) => {
                    unsafe {
                        device.free_command_buffers(command_pool, &command_buffers);
                        for slot in slots.drain(..) {
                            device.destroy_fence(slot.fence, None);
                            device.destroy_semaphore(slot.image_available, None);
                        }
                    }
                    return Err(RhiError::Backend(format!(
                        "create Vulkan frame semaphore: {error:?}"
                    )));
                }
            };
        let fence = match unsafe {
            device.create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )
        } {
            Ok(fence) => fence,
            Err(error) => {
                unsafe {
                    device.destroy_semaphore(image_available, None);
                    device.free_command_buffers(command_pool, &command_buffers);
                    for slot in slots.drain(..) {
                        device.destroy_fence(slot.fence, None);
                        device.destroy_semaphore(slot.image_available, None);
                    }
                }
                return Err(RhiError::Backend(format!(
                    "create Vulkan frame fence: {error:?}"
                )));
            }
        };
        slots.push(FrameSlot {
            command_buffer,
            fence,
            image_available,
            in_flight: false,
        });
    }
    Ok(slots)
}

pub(crate) fn create_owned_context<W>(
    entry: &Entry,
    config: &VulkanBackendConfig,
    window: &W,
) -> RhiResult<(NativeState, DeviceCapabilities)>
where
    W: HasDisplayHandle + HasWindowHandle + ?Sized,
{
    let app_name = CString::new(config.app_name.as_str())
        .map_err(|_| RhiError::InvalidDescription("Vulkan application name contains nul"))?;
    let display_handle = window
        .display_handle()
        .map_err(|error| RhiError::Backend(format!("get display handle: {error}")))?
        .as_raw();
    let window_handle = window
        .window_handle()
        .map_err(|error| RhiError::Backend(format!("get window handle: {error}")))?
        .as_raw();
    let required_extensions =
        ash_window::enumerate_required_extensions(display_handle).map_err(|error| {
            RhiError::Backend(format!("enumerate Vulkan window extensions: {error:?}"))
        })?;

    let layer_name = CString::new("VK_LAYER_KHRONOS_validation").expect("static layer name");
    let enable_validation = config.enable_validation
        && unsafe { entry.enumerate_instance_layer_properties() }
            .map(|layers| {
                layers.iter().any(|layer| unsafe {
                    CStr::from_ptr(layer.layer_name.as_ptr()) == layer_name.as_c_str()
                })
            })
            .unwrap_or(false);
    let layer_names = enable_validation
        .then_some(vec![layer_name.as_ptr()])
        .unwrap_or_default();
    let app_info = vk::ApplicationInfo::default()
        .application_name(&app_name)
        .application_version(1)
        .engine_name(&app_name)
        .engine_version(1)
        .api_version(vk::make_api_version(0, 1, 3, 0));
    let instance_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(required_extensions)
        .enabled_layer_names(&layer_names);
    let instance = unsafe { entry.create_instance(&instance_info, None) }
        .map_err(|error| RhiError::Backend(format!("create Vulkan instance: {error:?}")))?;
    let surface_loader = ash::khr::surface::Instance::new(entry, &instance);
    let surface = match unsafe {
        ash_window::create_surface(entry, &instance, display_handle, window_handle, None)
    } {
        Ok(surface) => surface,
        Err(error) => {
            unsafe { instance.destroy_instance(None) };
            return Err(RhiError::Backend(format!(
                "create Vulkan surface: {error:?}"
            )));
        }
    };
    let (physical_device, queue_family, capabilities) =
        match select_physical_device(&instance, &surface_loader, surface) {
            Ok(selection) => selection,
            Err(error) => {
                unsafe {
                    surface_loader.destroy_surface(surface, None);
                    instance.destroy_instance(None);
                }
                return Err(error);
            }
        };
    let queue_priority = [1.0_f32];
    let queue_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family)
        .queue_priorities(&queue_priority);
    // The built-in alpha-tested shaders use OpDemoteToHelperInvocation.  It
    // is an optional Vulkan 1.3 feature, so select_physical_device filters out
    // devices that cannot enable it before we create the logical device.
    let mut vulkan13_features = vk::PhysicalDeviceVulkan13Features::default()
        .dynamic_rendering(true)
        .shader_demote_to_helper_invocation(true);
    let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];
    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(std::slice::from_ref(&queue_info))
        .enabled_extension_names(&device_extensions)
        .push_next(&mut vulkan13_features);
    let device = match unsafe { instance.create_device(physical_device, &device_info, None) } {
        Ok(device) => device,
        Err(error) => {
            unsafe {
                surface_loader.destroy_surface(surface, None);
                instance.destroy_instance(None);
            }
            return Err(RhiError::Backend(format!(
                "create Vulkan device: {error:?}"
            )));
        }
    };
    let queue = unsafe { device.get_device_queue(queue_family, 0) };
    let command_pool = match unsafe {
        device.create_command_pool(
            &vk::CommandPoolCreateInfo::default()
                .queue_family_index(queue_family)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
            None,
        )
    } {
        Ok(command_pool) => command_pool,
        Err(error) => {
            unsafe {
                device.destroy_device(None);
                surface_loader.destroy_surface(surface, None);
                instance.destroy_instance(None);
            }
            return Err(RhiError::Backend(format!(
                "create Vulkan command pool: {error:?}"
            )));
        }
    };
    let allocator =
        match gpu_allocator::vulkan::Allocator::new(&gpu_allocator::vulkan::AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        }) {
            Ok(allocator) => allocator,
            Err(error) => {
                unsafe {
                    device.destroy_command_pool(command_pool, None);
                    device.destroy_device(None);
                    surface_loader.destroy_surface(surface, None);
                    instance.destroy_instance(None);
                }
                return Err(RhiError::Backend(format!(
                    "create Vulkan allocator: {error:?}"
                )));
            }
        };
    let descriptor_pool = match create_descriptor_pool(&device, capabilities.ray_tracing) {
        Ok(pool) => pool,
        Err(error) => {
            unsafe {
                device.destroy_command_pool(command_pool, None);
                device.destroy_device(None);
                surface_loader.destroy_surface(surface, None);
                instance.destroy_instance(None);
            }
            return Err(error);
        }
    };
    let frame_slots = match create_frame_slots(&device, command_pool) {
        Ok(slots) => slots,
        Err(error) => {
            unsafe {
                device.destroy_descriptor_pool(descriptor_pool, None);
                device.destroy_command_pool(command_pool, None);
                device.destroy_device(None);
                surface_loader.destroy_surface(surface, None);
                instance.destroy_instance(None);
            }
            return Err(error);
        }
    };

    Ok((
        NativeState {
            instance: Some(instance),
            physical_device,
            surface_loader: Some(surface_loader),
            surface: Some(surface),
            owns_instance: true,
            owns_surface: true,
            owns_device: true,
            owns_command_pool: true,
            device,
            queue,
            command_pool,
            descriptor_pool,
            allocator: Some(allocator),
            buffers: HashMap::new(),
            textures: HashMap::new(),
            shaders: HashMap::new(),
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            bind_groups: HashMap::new(),
            resource_tables: Arc::new(Default::default()),
            pending_submissions: Vec::new(),
            pending_buffer_uploads: Vec::new(),
            active_deferred_buffers: Vec::new(),
            frame_slots,
            next_frame_slot: 0,
            swapchain_loader: None,
            swapchain: None,
            swapchain_format: None,
            swapchain_extent: vk::Extent2D::default(),
            swapchain_config: None,
            swapchain_textures: Vec::new(),
            swapchain_render_finished: Vec::new(),
            acquired_image: None,
            acquired_frame_slot: None,
            acquired_image_submitted: false,
        },
        capabilities,
    ))
}

fn select_physical_device(
    instance: &ash::Instance,
    surface_loader: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> RhiResult<(vk::PhysicalDevice, u32, DeviceCapabilities)> {
    let devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(|error| RhiError::Backend(format!("enumerate Vulkan devices: {error:?}")))?;
    for physical_device in devices {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        if properties.api_version < vk::make_api_version(0, 1, 3, 0)
            || !device_supports_extension(instance, physical_device, ash::khr::swapchain::NAME)?
        {
            continue;
        }
        let queue_family = unsafe {
            instance
                .get_physical_device_queue_family_properties(physical_device)
                .iter()
                .enumerate()
                .find_map(|(index, family)| {
                    let graphics = family.queue_flags.contains(vk::QueueFlags::GRAPHICS);
                    let present = surface_loader
                        .get_physical_device_surface_support(physical_device, index as u32, surface)
                        .ok()?;
                    (graphics && present).then_some(index as u32)
                })
        };
        let Some(queue_family) = queue_family else {
            continue;
        };
        let features = unsafe { instance.get_physical_device_features(physical_device) };
        let mut vulkan13_features = vk::PhysicalDeviceVulkan13Features::default();
        let mut supported_features =
            vk::PhysicalDeviceFeatures2::default().push_next(&mut vulkan13_features);
        unsafe {
            instance.get_physical_device_features2(physical_device, &mut supported_features);
        }
        if vulkan13_features.shader_demote_to_helper_invocation != vk::TRUE {
            continue;
        }
        return Ok((
            physical_device,
            queue_family,
            DeviceCapabilities {
                supports_compute: true,
                supports_storage_images: true,
                supports_hdr: true,
                supports_present: true,
                ray_tracing: false,
                compute: true,
                multi_draw_indirect: features.multi_draw_indirect == vk::TRUE,
                hdr_render_targets: true,
                fsr3: false,
            },
        ));
    }
    Err(RhiError::Backend(String::from(
        "no Vulkan 1.3 device with graphics and present queues was found",
    )))
}

fn device_supports_extension(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    name: &CStr,
) -> RhiResult<bool> {
    let extensions = unsafe { instance.enumerate_device_extension_properties(physical_device) }
        .map_err(|error| RhiError::Backend(format!("enumerate Vulkan extensions: {error:?}")))?;
    Ok(extensions
        .iter()
        .any(|extension| unsafe { CStr::from_ptr(extension.extension_name.as_ptr()) == name }))
}
