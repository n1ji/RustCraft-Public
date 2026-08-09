use std::collections::HashMap;
use std::ffi::CString;

use ash::vk::Handle;
use ash::{vk, Entry};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use renderer::core::ResourceManager;
use renderer::rhi::command::*;
use renderer::rhi::device::*;
use renderer::rhi::types::*;

use super::command::{image_barrier_state, VulkanCommandEncoder};
use super::context::{
    create_descriptor_pool, create_frame_slots, create_owned_context, NativeBuffer, NativeSampler,
    NativeShader, NativeState, NativeTexture,
};
use super::pipeline::create_native_pipeline;
use super::resources::{
    buffer_usage, create_native_bind_group, texture_bytes_per_pixel, texture_format,
    texture_subresource, texture_usage,
};
use super::shader;
use super::{VulkanBackend, VulkanBackendConfig};

impl VulkanBackend {
    pub fn initialize(config: VulkanBackendConfig) -> RhiResult<Self> {
        Self::initialize_with_capabilities(config, DeviceCapabilities::default())
    }

    pub fn initialize_with_capabilities(
        config: VulkanBackendConfig,
        capabilities: DeviceCapabilities,
    ) -> RhiResult<Self> {
        if config.app_name.trim().is_empty() {
            return Err(RhiError::InvalidDescription(
                "backend application name must be non-empty",
            ));
        }
        Ok(Self {
            entry: unsafe { Entry::load() }
                .map_err(|error| RhiError::Backend(format!("load Vulkan loader: {error}")))?,
            config,
            resources: ResourceManager::new(),
            capabilities,
            next_submission: 0,
            native: None,
        })
    }

    pub fn initialize_with_window<W>(window: &W, config: VulkanBackendConfig) -> RhiResult<Self>
    where
        W: HasDisplayHandle + HasWindowHandle + ?Sized,
    {
        let mut backend = Self::initialize(config)?;
        let (native, capabilities) = create_owned_context(&backend.entry, &backend.config, window)?;
        backend.capabilities = capabilities;
        backend.native = Some(native);
        Ok(backend)
    }

    pub fn attach_native_context(
        &mut self,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
    ) -> RhiResult<()> {
        if self.native.is_some() {
            return Err(RhiError::Message(String::from(
                "Vulkan native context is already attached",
            )));
        }
        let allocator =
            gpu_allocator::vulkan::Allocator::new(&gpu_allocator::vulkan::AllocatorCreateDesc {
                instance: instance.clone(),
                device: device.clone(),
                physical_device,
                debug_settings: Default::default(),
                buffer_device_address: self.capabilities.ray_tracing,
                allocation_sizes: Default::default(),
            })
            .map_err(|error| RhiError::Backend(format!("create Vulkan allocator: {error:?}")))?;
        let descriptor_pool = create_descriptor_pool(device, self.capabilities.ray_tracing)?;
        let frame_slots = create_frame_slots(device, command_pool)?;
        self.native = Some(NativeState {
            instance: Some(instance.clone()),
            physical_device,
            surface_loader: None,
            surface: None,
            owns_instance: false,
            owns_surface: false,
            owns_device: false,
            owns_command_pool: false,
            device: device.clone(),
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
            resource_tables: std::sync::Arc::new(Default::default()),
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
        });
        Ok(())
    }

    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn config(&self) -> &VulkanBackendConfig {
        &self.config
    }

    pub const fn requested_api_version(&self) -> u32 {
        vk::make_api_version(0, 1, 3, 0)
    }

    pub fn resource_manager(&self) -> &ResourceManager {
        &self.resources
    }

    pub(crate) fn native_buffer(&self, handle: BufferHandle) -> Option<vk::Buffer> {
        self.native
            .as_ref()?
            .buffers
            .get(&handle)
            .map(|resource| resource.buffer)
    }

    pub(crate) fn native_texture(
        &self,
        handle: TextureHandle,
    ) -> Option<(vk::Image, vk::ImageView)> {
        self.native
            .as_ref()?
            .textures
            .get(&handle)
            .map(|resource| (resource.image, resource.view))
    }

    pub(crate) fn native_sampler(&self, handle: SamplerHandle) -> Option<vk::Sampler> {
        self.native
            .as_ref()?
            .samplers
            .get(&handle)
            .map(|resource| resource.sampler)
    }

    pub(crate) fn import_external_texture(
        &mut self,
        image: vk::Image,
        view: vk::ImageView,
        desc: TextureDesc,
    ) -> RhiResult<TextureHandle> {
        let handle = self.resources.create_external_texture(desc.clone())?;
        let Some(native) = self.native.as_mut() else {
            let _ = self.resources.destroy_texture(handle);
            return Err(RhiError::Backend(String::from(
                "cannot import without an attached Vulkan context",
            )));
        };
        native.textures.insert(
            handle,
            NativeTexture {
                image,
                view,
                allocation: None,
                owns_view: false,
                layout: vk::ImageLayout::PRESENT_SRC_KHR,
            },
        );
        native.add_texture_table(handle, image, view, vk::ImageLayout::PRESENT_SRC_KHR, desc);
        Ok(handle)
    }

    pub(crate) fn upload_buffer(
        &mut self,
        handle: BufferHandle,
        offset: u64,
        data: &[u8],
    ) -> RhiResult<()> {
        let (buffer_size, memory) = self
            .resources
            .buffer(handle)
            .map(|desc| (desc.size, desc.memory))
            .ok_or(RhiError::InvalidHandle)?;
        let data_size = u64::try_from(data.len())
            .map_err(|_| RhiError::InvalidDescription("buffer upload is too large"))?;
        if offset > buffer_size || data_size > buffer_size - offset {
            return Err(RhiError::InvalidDescription(
                "buffer upload exceeds the buffer description",
            ));
        }
        if data.is_empty() {
            return Ok(());
        }
        if memory == BufferMemory::DeviceLocal {
            return self.upload_device_local_buffer(handle, offset, data);
        }
        let Some(native) = self.native.as_mut() else {
            return Err(RhiError::Backend(String::from(
                "cannot upload without an attached Vulkan context",
            )));
        };
        let Some(resource) = native.buffers.get_mut(&handle) else {
            return Err(RhiError::InvalidHandle);
        };
        let Some(mapped) = resource.allocation.mapped_ptr() else {
            return Err(RhiError::Backend(String::from(
                "Vulkan buffer is not host visible",
            )));
        };
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                mapped.as_ptr().cast::<u8>().add(offset as usize),
                data.len(),
            );
        }
        Ok(())
    }

    fn upload_device_local_buffer(
        &mut self,
        destination: BufferHandle,
        destination_offset: u64,
        data: &[u8],
    ) -> RhiResult<()> {
        let staging = <Self as RenderDevice>::create_buffer(
            self,
            BufferDesc::new(
                data.len() as u64,
                BufferUsage::TRANSFER_SRC,
                "rhi.buffer-upload",
            ),
        )?;
        if let Err(error) = self.upload_buffer(staging, 0, data) {
            let _ = <Self as RenderDevice>::destroy_buffer(self, staging);
            return Err(error);
        }
        let staging_resource = self
            .native
            .as_mut()
            .and_then(|native| {
                native.remove_buffer_table(staging);
                native.buffers.remove(&staging)
            })
            .ok_or(RhiError::InvalidHandle)?;
        self.resources.destroy_buffer(staging)?;
        self.native
            .as_mut()
            .ok_or_else(|| RhiError::Backend(String::from("Vulkan context disappeared")))?
            .pending_buffer_uploads
            .push(super::context::PendingBufferUpload {
                staging: staging_resource,
                staging_handle: staging,
                destination,
                destination_offset,
                size: data.len() as u64,
            });
        Ok(())
    }

    pub(crate) fn upload_texture(
        &mut self,
        handle: TextureHandle,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()> {
        let desc = self
            .resources
            .texture(handle)
            .cloned()
            .ok_or(RhiError::InvalidHandle)?;
        if desc.mip_levels != 1 || desc.array_layers != 1 {
            return Err(RhiError::InvalidDescription(
                "texture upload currently targets one mip and one array layer",
            ));
        }
        let bytes_per_pixel = texture_bytes_per_pixel(desc.format);
        let minimum_row_pitch = desc
            .extent
            .width
            .checked_mul(bytes_per_pixel)
            .ok_or(RhiError::InvalidDescription("texture row pitch overflow"))?;
        if row_pitch < minimum_row_pitch || row_pitch % bytes_per_pixel != 0 {
            return Err(RhiError::InvalidDescription("invalid texture row pitch"));
        }
        let required_size = (row_pitch as usize)
            .checked_mul(desc.extent.height as usize)
            .ok_or(RhiError::InvalidDescription("texture upload size overflow"))?;
        if data.len() < required_size {
            return Err(RhiError::InvalidDescription(
                "texture upload data is smaller than the texture extent",
            ));
        }
        let staging = <Self as RenderDevice>::create_buffer(
            self,
            BufferDesc::new(
                required_size as u64,
                BufferUsage::TRANSFER_SRC,
                "rhi.texture-upload",
            ),
        )?;
        let result = self
            .upload_buffer(staging, 0, &data[..required_size])
            .and_then(|_| {
                self.copy_buffer_to_texture(staging, handle, row_pitch, [0, 0], desc.extent, &desc)
            });
        let destroy_result = <Self as RenderDevice>::destroy_buffer(self, staging);
        result.and(destroy_result)
    }

    pub(crate) fn upload_texture_region(
        &mut self,
        handle: TextureHandle,
        origin: [u32; 2],
        extent: Extent2D,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()> {
        let desc = self
            .resources
            .texture(handle)
            .cloned()
            .ok_or(RhiError::InvalidHandle)?;
        if extent.is_empty()
            || origin[0].checked_add(extent.width).is_none()
            || origin[1].checked_add(extent.height).is_none()
            || origin[0] + extent.width > desc.extent.width
            || origin[1] + extent.height > desc.extent.height
        {
            return Err(RhiError::InvalidDescription(
                "texture upload region is outside the texture extent",
            ));
        }
        let bytes_per_pixel = texture_bytes_per_pixel(desc.format);
        let minimum_row_pitch = extent
            .width
            .checked_mul(bytes_per_pixel)
            .ok_or(RhiError::InvalidDescription("texture row pitch overflow"))?;
        if row_pitch < minimum_row_pitch || row_pitch % bytes_per_pixel != 0 {
            return Err(RhiError::InvalidDescription("invalid texture row pitch"));
        }
        let required_size = (row_pitch as usize)
            .checked_mul(extent.height as usize)
            .ok_or(RhiError::InvalidDescription("texture upload size overflow"))?;
        if data.len() < required_size {
            return Err(RhiError::InvalidDescription(
                "texture upload data is smaller than the upload region",
            ));
        }
        let staging = <Self as RenderDevice>::create_buffer(
            self,
            BufferDesc::new(
                required_size as u64,
                BufferUsage::TRANSFER_SRC,
                "rhi.texture-region-upload",
            ),
        )?;
        let result = self
            .upload_buffer(staging, 0, &data[..required_size])
            .and_then(|_| {
                self.copy_buffer_to_texture(staging, handle, row_pitch, origin, extent, &desc)
            });
        let destroy_result = <Self as RenderDevice>::destroy_buffer(self, staging);
        result.and(destroy_result)
    }

    fn copy_buffer_to_texture(
        &mut self,
        staging: BufferHandle,
        texture: TextureHandle,
        row_pitch: u32,
        origin: [u32; 2],
        extent: Extent2D,
        desc: &TextureDesc,
    ) -> RhiResult<()> {
        let final_state = if desc.usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
            ResourceState::DepthAttachment
        } else {
            ResourceState::ShaderRead
        };
        let (_, final_access, final_layout) = image_barrier_state(final_state);
        let subresource = texture_subresource(desc);
        let (device, queue, command_pool, source, image, old_layout) = {
            let Some(native) = self.native.as_ref() else {
                return Err(RhiError::Backend(String::from(
                    "cannot upload without an attached Vulkan context",
                )));
            };
            let Some(source) = native.buffers.get(&staging).map(|resource| resource.buffer) else {
                return Err(RhiError::InvalidHandle);
            };
            let Some((image, old_layout)) = native
                .textures
                .get(&texture)
                .map(|resource| (resource.image, resource.layout))
            else {
                return Err(RhiError::InvalidHandle);
            };
            (
                native.device.clone(),
                native.queue,
                native.command_pool,
                source,
                image,
                old_layout,
            )
        };
        let command_buffer = unsafe {
            device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(command_pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(|error| {
                    RhiError::Backend(format!("allocate texture upload command buffer: {error:?}"))
                })?
        }[0];
        let result = (|| {
            unsafe {
                device
                    .begin_command_buffer(
                        command_buffer,
                        &vk::CommandBufferBeginInfo::default()
                            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                    )
                    .map_err(|error| {
                        RhiError::Backend(format!("begin texture upload command buffer: {error:?}"))
                    })?;
                let (src_stage, src_access) = image_layout_state_for_upload(old_layout);
                device.cmd_pipeline_barrier(
                    command_buffer,
                    src_stage,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[vk::ImageMemoryBarrier {
                        src_access_mask: src_access,
                        dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                        old_layout,
                        new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        image,
                        subresource_range: subresource,
                        ..Default::default()
                    }],
                );
                device.cmd_copy_buffer_to_image(
                    command_buffer,
                    source,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy {
                        buffer_row_length: row_pitch / texture_bytes_per_pixel(desc.format),
                        image_subresource: vk::ImageSubresourceLayers {
                            aspect_mask: subresource.aspect_mask,
                            layer_count: 1,
                            ..Default::default()
                        },
                        image_offset: vk::Offset3D {
                            x: origin[0] as i32,
                            y: origin[1] as i32,
                            z: 0,
                        },
                        image_extent: vk::Extent3D {
                            width: extent.width,
                            height: extent.height,
                            depth: 1,
                        },
                        ..Default::default()
                    }],
                );
                device.cmd_pipeline_barrier(
                    command_buffer,
                    vk::PipelineStageFlags::TRANSFER,
                    image_barrier_state(final_state).0,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[vk::ImageMemoryBarrier {
                        src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                        dst_access_mask: final_access,
                        old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        new_layout: final_layout,
                        image,
                        subresource_range: subresource,
                        ..Default::default()
                    }],
                );
                device.end_command_buffer(command_buffer).map_err(|error| {
                    RhiError::Backend(format!("end texture upload command buffer: {error:?}"))
                })?;
                let fence = device
                    .create_fence(&vk::FenceCreateInfo::default(), None)
                    .map_err(|error| {
                        RhiError::Backend(format!("create upload fence: {error:?}"))
                    })?;
                let submit_result = device
                    .queue_submit(
                        queue,
                        &[vk::SubmitInfo::default()
                            .command_buffers(std::slice::from_ref(&command_buffer))],
                        fence,
                    )
                    .map_err(|error| RhiError::Backend(format!("submit texture upload: {error:?}")))
                    .and_then(|_| {
                        device
                            .wait_for_fences(&[fence], true, u64::MAX)
                            .map_err(|error| {
                                RhiError::Backend(format!("wait texture upload: {error:?}"))
                            })
                    });
                device.destroy_fence(fence, None);
                submit_result?;
            }
            Ok(())
        })();
        unsafe { device.free_command_buffers(command_pool, &[command_buffer]) };
        if result.is_ok() {
            if let Some(native) = self.native.as_mut() {
                if let Some(resource) = native.textures.get_mut(&texture) {
                    resource.layout = final_layout;
                }
                native.update_texture_layout(texture, final_layout);
            }
        }
        result
    }
}

fn image_layout_state_for_upload(
    layout: vk::ImageLayout,
) -> (vk::PipelineStageFlags, vk::AccessFlags) {
    match layout {
        vk::ImageLayout::UNDEFINED => (
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        vk::ImageLayout::PRESENT_SRC_KHR => (
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::PipelineStageFlags::FRAGMENT_SHADER | vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::AccessFlags::SHADER_READ,
        ),
        _ => (
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        ),
    }
}

impl CommandQueue for VulkanBackend {
    fn submit(&mut self, command_buffer: CommandBuffer) -> RhiResult<SubmissionId> {
        if let Some(native) = self.native.as_mut() {
            if let Some(raw) = command_buffer.native() {
                let raw_command_buffer = vk::CommandBuffer::from_raw(raw);
                let frame_slot = native.acquired_frame_slot.ok_or_else(|| {
                    RhiError::Backend(String::from(
                        "submit Vulkan command buffer without an acquired frame slot",
                    ))
                })?;
                let slot = native.frame_slots.get(frame_slot).ok_or_else(|| {
                    RhiError::Backend(String::from("Vulkan frame slot index is invalid"))
                })?;
                if slot.command_buffer != raw_command_buffer {
                    return Err(RhiError::Backend(String::from(
                        "Vulkan command buffer does not belong to the acquired frame slot",
                    )));
                }
                let fence = slot.fence;
                let image_available = native.acquired_image.map(|_| slot.image_available);
                let render_finished = native.acquired_image.and_then(|image_index| {
                    native
                        .swapchain_render_finished
                        .get(image_index as usize)
                        .copied()
                });
                // A frame has at most one acquire wait and one render-finished
                // signal. Keep these as stack slices; allocating three small
                // Vecs here was visible at uncapped redraw rates.
                let wait_semaphores = image_available
                    .as_ref()
                    .map_or(&[][..], std::slice::from_ref);
                let signal_semaphores = render_finished
                    .as_ref()
                    .map_or(&[][..], std::slice::from_ref);
                let wait_stage = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
                let wait_stages = if wait_semaphores.is_empty() {
                    &[][..]
                } else {
                    std::slice::from_ref(&wait_stage)
                };
                unsafe {
                    native
                        .device
                        .reset_fences(std::slice::from_ref(&fence))
                        .map_err(|error| {
                            RhiError::Backend(format!("reset Vulkan frame fence: {error:?}"))
                        })?;
                }
                let submit_result = unsafe {
                    native.device.queue_submit(
                        native.queue,
                        &[vk::SubmitInfo::default()
                            .wait_semaphores(&wait_semaphores)
                            .wait_dst_stage_mask(&wait_stages)
                            .command_buffers(std::slice::from_ref(&raw_command_buffer))
                            .signal_semaphores(&signal_semaphores)],
                        fence,
                    )
                };
                if let Err(error) = submit_result {
                    return Err(RhiError::Backend(format!(
                        "submit Vulkan command buffer: {error:?}"
                    )));
                }
                if let Some(slot) = native.frame_slots.get_mut(frame_slot) {
                    slot.in_flight = true;
                }
                native.acquired_image_submitted = render_finished.is_some();
                for command in command_buffer.commands() {
                    if let Command::Barrier(ResourceBarrier {
                        resource: ResourceHandle::Texture(handle),
                        after,
                        ..
                    }) = command
                    {
                        let layout = image_barrier_state(*after).2;
                        if let Some(texture) = native.textures.get_mut(handle) {
                            texture.layout = layout;
                        }
                        native.update_texture_layout(*handle, layout);
                    }
                }
                native
                    .pending_submissions
                    .push(super::context::PendingSubmission {
                        submission_id: self.next_submission.wrapping_add(1),
                        frame_slot,
                        deferred_buffers: std::mem::take(&mut native.active_deferred_buffers),
                    });
                native.acquired_frame_slot = None;
            }
        }
        self.next_submission = self.next_submission.wrapping_add(1);
        Ok(SubmissionId(self.next_submission))
    }

    fn collect_completed(&mut self) -> RhiResult<()> {
        let Some(native) = self.native.as_mut() else {
            return Ok(());
        };
        let mut pending = Vec::with_capacity(native.pending_submissions.len());
        let submissions = std::mem::take(&mut native.pending_submissions);
        for submission in submissions {
            let fence = native
                .frame_slots
                .get(submission.frame_slot)
                .ok_or_else(|| RhiError::Backend(String::from("Vulkan frame slot is invalid")))?
                .fence;
            let ready = unsafe { native.device.get_fence_status(fence) }
                .map_err(|error| RhiError::Backend(format!("query Vulkan fence: {error:?}")))?;
            if ready {
                if let Some(slot) = native.frame_slots.get_mut(submission.frame_slot) {
                    slot.in_flight = false;
                }
                unsafe {
                    for resource in submission.deferred_buffers {
                        native.device.destroy_buffer(resource.buffer, None);
                        native
                            .allocator_mut()
                            .free(resource.allocation)
                            .map_err(|error| {
                                RhiError::Backend(format!("free deferred Vulkan buffer: {error:?}"))
                            })?;
                    }
                }
            } else {
                pending.push(submission);
            }
        }
        native.pending_submissions = pending;
        Ok(())
    }

    fn wait_idle(&mut self) -> RhiResult<()> {
        let Some(native) = self.native.as_mut() else {
            return Ok(());
        };
        unsafe {
            native.device.device_wait_idle().map_err(|error| {
                RhiError::Backend(format!("wait Vulkan device idle: {error:?}"))
            })?;
            let submissions = std::mem::take(&mut native.pending_submissions);
            for submission in submissions {
                for resource in submission.deferred_buffers {
                    native.device.destroy_buffer(resource.buffer, None);
                    native
                        .allocator_mut()
                        .free(resource.allocation)
                        .map_err(|error| {
                            RhiError::Backend(format!("free deferred Vulkan buffer: {error:?}"))
                        })?;
                }
            }
        }
        Ok(())
    }
}

impl RenderDevice for VulkanBackend {
    fn capabilities(&self) -> DeviceCapabilities {
        self.capabilities
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> RhiResult<BufferHandle> {
        let handle = self.resources.create_buffer(desc.clone())?;
        let Some(native) = self.native.as_mut() else {
            return Ok(handle);
        };
        let usage = buffer_usage(desc.usage);
        if usage.is_empty() {
            let _ = self.resources.destroy_buffer(handle);
            return Err(RhiError::InvalidDescription(
                "buffer usage must not be empty",
            ));
        }
        let usage = usage
            | if desc.memory == BufferMemory::DeviceLocal {
                vk::BufferUsageFlags::TRANSFER_DST
            } else {
                vk::BufferUsageFlags::empty()
            };
        let buffer = match unsafe {
            native.device.create_buffer(
                &vk::BufferCreateInfo::default().size(desc.size).usage(usage),
                None,
            )
        } {
            Ok(buffer) => buffer,
            Err(error) => {
                let _ = self.resources.destroy_buffer(handle);
                return Err(RhiError::Backend(format!(
                    "create Vulkan buffer: {error:?}"
                )));
            }
        };
        let requirements = unsafe { native.device.get_buffer_memory_requirements(buffer) };
        let name = if desc.label.is_empty() {
            "rhi-buffer"
        } else {
            desc.label.as_str()
        };
        let allocation =
            match native
                .allocator_mut()
                .allocate(&gpu_allocator::vulkan::AllocationCreateDesc {
                    name,
                    requirements,
                    location: match desc.memory {
                        BufferMemory::Upload => gpu_allocator::MemoryLocation::CpuToGpu,
                        BufferMemory::DeviceLocal => gpu_allocator::MemoryLocation::GpuOnly,
                    },
                    linear: true,
                    allocation_scheme: gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
                }) {
                Ok(allocation) => allocation,
                Err(error) => {
                    unsafe { native.device.destroy_buffer(buffer, None) };
                    let _ = self.resources.destroy_buffer(handle);
                    return Err(RhiError::Backend(format!(
                        "allocate Vulkan buffer: {error:?}"
                    )));
                }
            };
        if let Err(error) = unsafe {
            native
                .device
                .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
        } {
            let _ = native.allocator_mut().free(allocation);
            unsafe { native.device.destroy_buffer(buffer, None) };
            let _ = self.resources.destroy_buffer(handle);
            return Err(RhiError::Backend(format!("bind Vulkan buffer: {error:?}")));
        }
        native
            .buffers
            .insert(handle, NativeBuffer { buffer, allocation });
        native.add_buffer_table(handle, buffer, desc.size);
        Ok(handle)
    }

    fn create_texture(&mut self, desc: TextureDesc) -> RhiResult<TextureHandle> {
        let handle = self.resources.create_texture(desc.clone())?;
        let Some(native) = self.native.as_mut() else {
            return Ok(handle);
        };
        let usage = texture_usage(desc.usage);
        if usage.is_empty() {
            let _ = self.resources.destroy_texture(handle);
            return Err(RhiError::InvalidDescription(
                "texture usage must not be empty",
            ));
        }
        let image = match unsafe {
            native.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(texture_format(desc.format))
                    .extent(vk::Extent3D {
                        width: desc.extent.width,
                        height: desc.extent.height,
                        depth: 1,
                    })
                    .mip_levels(desc.mip_levels)
                    .array_layers(desc.array_layers)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
        } {
            Ok(image) => image,
            Err(error) => {
                let _ = self.resources.destroy_texture(handle);
                return Err(RhiError::Backend(format!("create Vulkan image: {error:?}")));
            }
        };
        let requirements = unsafe { native.device.get_image_memory_requirements(image) };
        let allocation =
            match native
                .allocator_mut()
                .allocate(&gpu_allocator::vulkan::AllocationCreateDesc {
                    name: if desc.label.is_empty() {
                        "rhi-texture"
                    } else {
                        desc.label.as_str()
                    },
                    requirements,
                    location: gpu_allocator::MemoryLocation::GpuOnly,
                    linear: false,
                    allocation_scheme: gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
                }) {
                Ok(allocation) => allocation,
                Err(error) => {
                    unsafe { native.device.destroy_image(image, None) };
                    let _ = self.resources.destroy_texture(handle);
                    return Err(RhiError::Backend(format!(
                        "allocate Vulkan image: {error:?}"
                    )));
                }
            };
        if let Err(error) = unsafe {
            native
                .device
                .bind_image_memory(image, allocation.memory(), allocation.offset())
        } {
            let _ = native.allocator_mut().free(allocation);
            unsafe { native.device.destroy_image(image, None) };
            let _ = self.resources.destroy_texture(handle);
            return Err(RhiError::Backend(format!("bind Vulkan image: {error:?}")));
        }
        let view = match unsafe {
            native.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(if desc.array_layers > 1 {
                        vk::ImageViewType::TYPE_2D_ARRAY
                    } else {
                        vk::ImageViewType::TYPE_2D
                    })
                    .format(texture_format(desc.format))
                    .subresource_range(texture_subresource(&desc)),
                None,
            )
        } {
            Ok(view) => view,
            Err(error) => {
                let _ = native.allocator_mut().free(allocation);
                unsafe { native.device.destroy_image(image, None) };
                let _ = self.resources.destroy_texture(handle);
                return Err(RhiError::Backend(format!(
                    "create Vulkan image view: {error:?}"
                )));
            }
        };
        native.textures.insert(
            handle,
            NativeTexture {
                image,
                view,
                allocation: Some(allocation),
                owns_view: true,
                layout: vk::ImageLayout::UNDEFINED,
            },
        );
        native.add_texture_table(handle, image, view, vk::ImageLayout::UNDEFINED, desc);
        Ok(handle)
    }

    fn create_shader(&mut self, desc: ShaderDesc) -> RhiResult<ShaderHandle> {
        let spirv = if self.native.is_some() {
            match &desc.code {
                ShaderCode::Binary { bytes, .. } => bytes.clone(),
                ShaderCode::Source(source) => {
                    shader::compile_source_to_spirv(source, desc.stage, &desc.entry_point)?
                }
            }
        } else {
            Vec::new()
        };
        let handle = self.resources.create_shader(desc.clone())?;
        let Some(native) = self.native.as_mut() else {
            return Ok(handle);
        };
        if spirv.len() % 4 != 0 {
            let _ = self.resources.destroy_shader(handle);
            return Err(RhiError::InvalidDescription(
                "SPIR-V code is not word aligned",
            ));
        }
        let entry_point = CString::new(desc.entry_point.as_str()).map_err(|_| {
            let _ = self.resources.destroy_shader(handle);
            RhiError::InvalidDescription("shader entry point contains nul")
        })?;
        let words = spirv
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .collect::<Vec<_>>();
        let module = match unsafe {
            native.device.create_shader_module(
                &vk::ShaderModuleCreateInfo {
                    code_size: spirv.len(),
                    p_code: words.as_ptr(),
                    ..Default::default()
                },
                None,
            )
        } {
            Ok(module) => module,
            Err(error) => {
                let _ = self.resources.destroy_shader(handle);
                return Err(RhiError::Backend(format!(
                    "create Vulkan shader: {error:?}"
                )));
            }
        };
        native.shaders.insert(
            handle,
            NativeShader {
                module,
                entry_point,
                stage: desc.stage,
                interface: desc.interface,
            },
        );
        Ok(handle)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc) -> RhiResult<PipelineHandle> {
        let handle = self.resources.create_pipeline(desc.clone())?;
        let Some(native) = self.native.as_mut() else {
            return Ok(handle);
        };
        let pipeline = match create_native_pipeline(&native.device, &native.shaders, &desc) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                let _ = self.resources.destroy_pipeline(handle);
                return Err(error);
            }
        };
        native.pipelines.insert(handle, pipeline.clone());
        native.add_pipeline_table(handle, pipeline);
        Ok(handle)
    }

    fn create_sampler(&mut self, desc: SamplerDesc) -> RhiResult<SamplerHandle> {
        let handle = self.resources.create_sampler(desc.clone())?;
        let Some(native) = self.native.as_mut() else {
            return Ok(handle);
        };
        let address_mode = |repeat| {
            if repeat {
                vk::SamplerAddressMode::REPEAT
            } else {
                vk::SamplerAddressMode::CLAMP_TO_EDGE
            }
        };
        let filter = if desc.linear_filter {
            vk::Filter::LINEAR
        } else {
            vk::Filter::NEAREST
        };
        let sampler = match unsafe {
            native.device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(filter)
                    .min_filter(filter)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(address_mode(desc.repeat_u))
                    .address_mode_v(address_mode(desc.repeat_v))
                    .address_mode_w(address_mode(desc.repeat_w))
                    .max_lod(1.0),
                None,
            )
        } {
            Ok(sampler) => sampler,
            Err(error) => {
                let _ = self.resources.destroy_sampler(handle);
                return Err(RhiError::Backend(format!(
                    "create Vulkan sampler: {error:?}"
                )));
            }
        };
        native.samplers.insert(handle, NativeSampler { sampler });
        Ok(handle)
    }

    fn create_bind_group(&mut self, desc: BindGroupDesc) -> RhiResult<BindGroupHandle> {
        let handle = self.resources.create_bind_group(desc.clone())?;
        let Some(native) = self.native.as_mut() else {
            return Ok(handle);
        };
        let bind_group = match create_native_bind_group(native, &self.resources, &desc) {
            Ok(bind_group) => bind_group,
            Err(error) => {
                let _ = self.resources.destroy_bind_group(handle);
                return Err(error);
            }
        };
        native.bind_groups.insert(handle, bind_group.clone());
        native.add_bind_group_table(handle, bind_group);
        Ok(handle)
    }

    fn configure_surface(&mut self, config: SurfaceConfig) -> RhiResult<()> {
        VulkanBackend::configure_surface(self, config)
    }

    fn acquire_surface_frame(&mut self) -> RhiResult<SurfaceFrame> {
        VulkanBackend::acquire_surface_frame(self)
    }

    fn present(&mut self, frame: SurfaceFrame) -> RhiResult<()> {
        VulkanBackend::present(self, frame)
    }

    fn upload_texture(
        &mut self,
        handle: TextureHandle,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()> {
        VulkanBackend::upload_texture(self, handle, data, row_pitch)
    }

    fn upload_texture_region(
        &mut self,
        handle: TextureHandle,
        origin: [u32; 2],
        extent: Extent2D,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()> {
        VulkanBackend::upload_texture_region(self, handle, origin, extent, data, row_pitch)
    }

    fn upload_buffer(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> RhiResult<()> {
        VulkanBackend::upload_buffer(self, handle, offset, data)
    }

    fn destroy_buffer(&mut self, handle: BufferHandle) -> RhiResult<()> {
        self.resources.validate_buffer_destruction(handle)?;
        let _ = self.resources.destroy_buffer(handle)?;
        if let Some(native) = self.native.as_mut() {
            let pending_uploads = std::mem::take(&mut native.pending_buffer_uploads);
            let mut retained_uploads = Vec::with_capacity(pending_uploads.len());
            let mut cleanup_error = None;
            for upload in pending_uploads {
                if upload.destination == handle {
                    if let Err(error) = native.destroy_buffer_resource(upload.staging) {
                        cleanup_error.get_or_insert(error);
                    }
                } else {
                    retained_uploads.push(upload);
                }
            }
            native.pending_buffer_uploads = retained_uploads;
            if let Some(resource) = native.buffers.remove(&handle) {
                native.remove_buffer_table(handle);
                if let Some(submission) = native.pending_submissions.last_mut() {
                    submission.deferred_buffers.push(resource);
                } else {
                    unsafe { native.device.destroy_buffer(resource.buffer, None) };
                    native
                        .allocator_mut()
                        .free(resource.allocation)
                        .map_err(|error| {
                            RhiError::Backend(format!("free Vulkan buffer: {error:?}"))
                        })?;
                }
            }
            if let Some(error) = cleanup_error {
                return Err(error);
            }
        }
        Ok(())
    }

    fn destroy_texture(&mut self, handle: TextureHandle) -> RhiResult<()> {
        self.resources.validate_texture_destruction(handle)?;
        self.wait_idle()?;
        let _ = self.resources.destroy_texture(handle)?;
        if let Some(native) = self.native.as_mut() {
            if let Some(resource) = native.textures.remove(&handle) {
                native.remove_texture_table(handle);
                if resource.owns_view {
                    unsafe { native.device.destroy_image_view(resource.view, None) };
                }
                if let Some(allocation) = resource.allocation {
                    unsafe { native.device.destroy_image(resource.image, None) };
                    native.allocator_mut().free(allocation).map_err(|error| {
                        RhiError::Backend(format!("free Vulkan image: {error:?}"))
                    })?;
                }
            }
        }
        Ok(())
    }

    fn destroy_shader(&mut self, handle: ShaderHandle) -> RhiResult<()> {
        self.resources.validate_shader_destruction(handle)?;
        self.wait_idle()?;
        let _ = self.resources.destroy_shader(handle)?;
        if let Some(native) = self.native.as_mut() {
            if let Some(resource) = native.shaders.remove(&handle) {
                unsafe { native.device.destroy_shader_module(resource.module, None) };
            }
        }
        Ok(())
    }

    fn destroy_pipeline(&mut self, handle: PipelineHandle) -> RhiResult<()> {
        self.resources.validate_pipeline_destruction(handle)?;
        self.wait_idle()?;
        let _ = self.resources.destroy_pipeline(handle)?;
        if let Some(native) = self.native.as_mut() {
            if let Some(resource) = native.pipelines.remove(&handle) {
                native.remove_pipeline_table(handle);
                unsafe {
                    native.device.destroy_pipeline(resource.pipeline, None);
                    native.device.destroy_pipeline_layout(resource.layout, None);
                    for set_layout in resource.set_layouts {
                        native
                            .device
                            .destroy_descriptor_set_layout(set_layout, None);
                    }
                }
            }
        }
        Ok(())
    }

    fn destroy_sampler(&mut self, handle: SamplerHandle) -> RhiResult<()> {
        self.resources.validate_sampler_destruction(handle)?;
        self.wait_idle()?;
        let _ = self.resources.destroy_sampler(handle)?;
        if let Some(native) = self.native.as_mut() {
            if let Some(resource) = native.samplers.remove(&handle) {
                unsafe { native.device.destroy_sampler(resource.sampler, None) };
            }
        }
        Ok(())
    }

    fn destroy_bind_group(&mut self, handle: BindGroupHandle) -> RhiResult<()> {
        if self.resources.bind_group(handle).is_none() {
            return Err(RhiError::InvalidHandle);
        }
        self.wait_idle()?;
        let _ = self.resources.destroy_bind_group(handle)?;
        if let Some(native) = self.native.as_mut() {
            if let Some(group) = native.bind_groups.remove(&handle) {
                native.remove_bind_group_table(handle);
                unsafe {
                    native
                        .device
                        .free_descriptor_sets(native.descriptor_pool, &[group.descriptor_set])
                        .map_err(|error| {
                            RhiError::Backend(format!("free Vulkan descriptor set: {error:?}"))
                        })?;
                }
            }
        }
        Ok(())
    }

    fn begin_commands(&mut self, label: &str) -> RhiResult<Box<dyn CommandEncoder>> {
        let Some(native) = self.native.as_mut() else {
            return Ok(Box::new(RecordingCommandEncoder::new(label)));
        };
        let (device, frame_slot, fence, needs_wait) = {
            if native.frame_slots.is_empty() {
                return Err(RhiError::Backend(String::from(
                    "Vulkan context has no reusable frame slots",
                )));
            }
            let frame_slot = if let Some(frame_slot) = native.acquired_frame_slot {
                frame_slot
            } else {
                let frame_slot = native.next_frame_slot % native.frame_slots.len();
                native.next_frame_slot = native.next_frame_slot.wrapping_add(1);
                native.acquired_frame_slot = Some(frame_slot);
                frame_slot
            };
            let slot = native.frame_slots.get(frame_slot).ok_or_else(|| {
                RhiError::Backend(String::from("Vulkan frame slot index is invalid"))
            })?;
            (
                native.device.clone(),
                frame_slot,
                slot.fence,
                slot.in_flight,
            )
        };
        if needs_wait {
            unsafe {
                device
                    .wait_for_fences(&[fence], true, u64::MAX)
                    .map_err(|error| {
                        RhiError::Backend(format!("wait Vulkan frame fence: {error:?}"))
                    })?;
            }
            self.collect_completed()?;
        }
        let native = self.native.as_mut().ok_or_else(|| {
            RhiError::Backend(String::from(
                "Vulkan context disappeared while beginning commands",
            ))
        })?;
        let command_buffer = native
            .frame_slots
            .get(frame_slot)
            .ok_or_else(|| RhiError::Backend(String::from("Vulkan frame slot index is invalid")))?
            .command_buffer;
        unsafe {
            native
                .device
                .reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty())
                .map_err(|error| {
                    RhiError::Backend(format!("reset Vulkan command buffer: {error:?}"))
                })?;
        }
        if let Err(error) = unsafe {
            native.device.begin_command_buffer(
                command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
        } {
            return Err(RhiError::Backend(format!(
                "begin Vulkan command buffer: {error:?}"
            )));
        }
        let pending_uploads = std::mem::take(&mut native.pending_buffer_uploads);
        let mut upload_commands = Vec::with_capacity(pending_uploads.len());
        for upload in pending_uploads {
            let destination_buffer = native
                .resource_tables
                .buffer_objects
                .get(&upload.destination)
                .copied()
                .ok_or(RhiError::InvalidHandle)?;
            let destination_size = native
                .resource_tables
                .buffer_sizes
                .get(&upload.destination)
                .copied()
                .ok_or(RhiError::InvalidHandle)?;
            let destination_end = upload
                .destination_offset
                .checked_add(upload.size)
                .ok_or(RhiError::InvalidDescription("buffer upload range overflow"))?;
            if upload.size == 0 || destination_end > destination_size {
                return Err(RhiError::InvalidDescription(
                    "buffer upload exceeds the destination buffer",
                ));
            }
            unsafe {
                native.device.cmd_copy_buffer(
                    command_buffer,
                    upload.staging.buffer,
                    destination_buffer,
                    &[vk::BufferCopy {
                        src_offset: 0,
                        dst_offset: upload.destination_offset,
                        size: upload.size,
                    }],
                );
                native.device.cmd_pipeline_barrier(
                    command_buffer,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[vk::BufferMemoryBarrier {
                        src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                        dst_access_mask: vk::AccessFlags::MEMORY_READ,
                        buffer: destination_buffer,
                        offset: upload.destination_offset,
                        size: upload.size,
                        ..Default::default()
                    }],
                    &[],
                );
            }
            upload_commands.push(Command::CopyBuffer {
                source: upload.staging_handle,
                destination: upload.destination,
                source_offset: 0,
                destination_offset: upload.destination_offset,
                size: upload.size,
            });
            native.active_deferred_buffers.push(upload.staging);
        }
        Ok(Box::new(VulkanCommandEncoder {
            device: native.device.clone(),
            command_buffer,
            label: label.to_string(),
            commands: upload_commands,
            render_pass_open: false,
            bound_pipeline: None,
            bound_pipeline_handle: None,
            resource_tables: std::sync::Arc::clone(&native.resource_tables),
            texture_layouts: HashMap::new(),
        }))
    }
}
