use std::mem::ManuallyDrop;

use renderer::rhi::command::{Command, CommandBuffer, CommandEncoder};
use renderer::rhi::device::{CommandQueue, RenderDevice};
use renderer::rhi::types::*;
use windows::core::{Interface, BOOL};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};

use super::command;
use super::context::{self, NativeBindGroup, NativeBuffer, NativeSampler, NativeTexture};
use super::pipeline;
use super::shader;
use super::{backend_error, checked_u32, DirectX12Backend, MAX_BINDINGS};

fn initial_buffer_state(usage: BufferUsage) -> D3D12_RESOURCE_STATES {
    if usage.contains(BufferUsage::INDEX) {
        D3D12_RESOURCE_STATE_INDEX_BUFFER
    } else if usage.contains(BufferUsage::VERTEX) || usage.contains(BufferUsage::UNIFORM) {
        D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER
    } else if usage.contains(BufferUsage::STORAGE) {
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS
    } else {
        D3D12_RESOURCE_STATE_COMMON
    }
}

impl DirectX12Backend {
    pub(crate) fn create_buffer_native(&mut self, desc: BufferDesc) -> RhiResult<BufferHandle> {
        if desc.size == 0 {
            return Err(RhiError::InvalidDescription("buffer size must be non-zero"));
        }
        let resource_size = if desc.usage.contains(BufferUsage::UNIFORM) {
            u64::from(context::aligned_constant_size(desc.size)?)
        } else {
            desc.size
        };
        let (resource, state) = {
            let native = self.native_ref()?;
            if desc.memory == BufferMemory::DeviceLocal {
                let resource = context::create_committed_resource(
                    &native.device,
                    &context::buffer_resource_desc(resource_size)?,
                    if desc.usage.contains(BufferUsage::TRANSFER_DST) {
                        D3D12_RESOURCE_STATE_COPY_DEST
                    } else {
                        initial_buffer_state(desc.usage)
                    },
                    None,
                )?;
                context::set_resource_name(&resource, &desc.label);
                let state = if desc.usage.contains(BufferUsage::TRANSFER_DST) {
                    D3D12_RESOURCE_STATE_COPY_DEST
                } else {
                    initial_buffer_state(desc.usage)
                };
                (resource, state)
            } else {
                (
                    context::create_upload_buffer(&native.device, resource_size, &desc.label)?,
                    D3D12_RESOURCE_STATE_GENERIC_READ,
                )
            }
        };
        let handle = self.resources.create_buffer(desc.clone())?;
        let native = self.native_mut()?;
        native.buffers.insert(
            handle,
            NativeBuffer {
                resource,
                size: desc.size,
                resource_size,
                usage: desc.usage,
                state,
            },
        );
        Ok(handle)
    }

    pub(crate) fn create_texture_native(&mut self, desc: TextureDesc) -> RhiResult<TextureHandle> {
        let resource_desc = context::texture_resource_desc(&desc)?;
        let resource = {
            let native = self.native_ref()?;
            let clear = texture_clear_value(&desc)?;
            context::create_committed_resource(
                &native.device,
                &resource_desc,
                D3D12_RESOURCE_STATE_COMMON,
                clear.as_ref(),
            )?
        };
        context::set_resource_name(&resource, &desc.label);
        let (rtv, dsv) = {
            let native = self.native_mut()?;
            create_attachment_views(native, &resource, &desc)?
        };
        let handle = self.resources.create_texture(desc.clone())?;
        self.native_mut()?.textures.insert(
            handle,
            NativeTexture {
                resource,
                desc,
                state: D3D12_RESOURCE_STATE_COMMON,
                rtv,
                dsv,
            },
        );
        Ok(handle)
    }

    pub(crate) fn create_shader_native(&mut self, desc: ShaderDesc) -> RhiResult<ShaderHandle> {
        let native_shader = {
            let backend = &*self;
            shader::create_native_shader(backend, &desc)?
        };
        let handle = self.resources.create_shader(desc)?;
        self.native_mut()?.shaders.insert(handle, native_shader);
        Ok(handle)
    }

    pub(crate) fn create_pipeline_native(
        &mut self,
        desc: PipelineDesc,
    ) -> RhiResult<PipelineHandle> {
        let native_pipeline = pipeline::create_native_pipeline(self, &desc)?;
        let handle = self.resources.create_pipeline(desc)?;
        self.native_mut()?.pipelines.insert(handle, native_pipeline);
        Ok(handle)
    }

    pub(crate) fn create_sampler_native(&mut self, desc: SamplerDesc) -> RhiResult<SamplerHandle> {
        let handle = self.resources.create_sampler(desc.clone())?;
        self.native_mut()?
            .samplers
            .insert(handle, NativeSampler { desc });
        Ok(handle)
    }

    pub(crate) fn create_bind_group_native(
        &mut self,
        desc: BindGroupDesc,
    ) -> RhiResult<BindGroupHandle> {
        if desc.set != 0 {
            return Err(RhiError::InvalidDescription(
                "Direct3D 12 backend currently exposes descriptor set zero",
            ));
        }
        let pipeline_desc = self
            .resources
            .pipeline(desc.pipeline)
            .cloned()
            .ok_or(RhiError::InvalidHandle)?;
        let (resource_offset, sampler_offset) = {
            let native = self.native_mut()?;
            let resource_offset = native.allocate_resource_descriptors()?;
            let sampler_offset = native.allocate_sampler_descriptors()?;
            (resource_offset, sampler_offset)
        };
        write_bind_group_descriptors(self, &pipeline_desc, &desc, resource_offset, sampler_offset)?;
        let handle = self.resources.create_bind_group(desc.clone())?;
        self.native_mut()?.bind_groups.insert(
            handle,
            NativeBindGroup {
                desc,
                resource_offset,
                sampler_offset,
            },
        );
        Ok(handle)
    }

    pub(crate) fn upload_buffer_native(
        &mut self,
        handle: BufferHandle,
        offset: u64,
        data: &[u8],
    ) -> RhiResult<()> {
        let (size, memory) = self
            .resources
            .buffer(handle)
            .map(|desc| (desc.size, desc.memory))
            .ok_or(RhiError::InvalidHandle)?;
        let data_size = u64::try_from(data.len())
            .map_err(|_| RhiError::InvalidDescription("buffer upload is too large"))?;
        if offset > size || data_size > size - offset {
            return Err(RhiError::InvalidDescription(
                "buffer upload exceeds the buffer description",
            ));
        }
        if data.is_empty() {
            return Ok(());
        }
        if memory == BufferMemory::DeviceLocal {
            return self.upload_device_local_buffer_native(handle, offset, data);
        }
        let resource = self
            .native_ref()?
            .buffers
            .get(&handle)
            .ok_or(RhiError::InvalidHandle)?
            .resource
            .clone();
        unsafe {
            let mut mapped = std::ptr::null_mut();
            resource
                .Map(0, None, Some(&mut mapped))
                .map_err(|error| backend_error("map Direct3D 12 upload buffer", error))?;
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                mapped.cast::<u8>().add(offset as usize),
                data.len(),
            );
            resource.Unmap(0, None);
        }
        Ok(())
    }

    fn upload_device_local_buffer_native(
        &mut self,
        destination: BufferHandle,
        destination_offset: u64,
        data: &[u8],
    ) -> RhiResult<()> {
        let staging = self.create_buffer_native(BufferDesc::new(
            data.len() as u64,
            BufferUsage::TRANSFER_SRC,
            "rhi.buffer-upload",
        ))?;
        if let Err(error) = self.upload_buffer_native(staging, 0, data) {
            let _ = self.destroy_buffer_native(staging);
            return Err(error);
        }
        let staging_resource = self
            .native_mut()?
            .buffers
            .remove(&staging)
            .ok_or(RhiError::InvalidHandle)?;
        self.resources.destroy_buffer(staging)?;
        self.native_mut()?
            .pending_buffer_uploads
            .push(context::PendingBufferUpload {
                staging: staging_resource,
                staging_handle: staging,
                destination,
                destination_offset,
                size: data.len() as u64,
            });
        Ok(())
    }

    pub(crate) fn upload_texture_native(
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
        self.upload_texture_region_native(handle, [0, 0], desc.extent, data, row_pitch)
    }

    pub(crate) fn upload_texture_region_native(
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
        validate_texture_upload(&desc, origin, extent, data, row_pitch)?;
        let bytes_per_pixel = texture_bytes_per_pixel(desc.format)?;
        let minimum_row_pitch = extent
            .width
            .checked_mul(bytes_per_pixel)
            .ok_or(RhiError::InvalidDescription("texture row pitch overflow"))?;
        let upload_row_pitch = align_up(minimum_row_pitch, D3D12_TEXTURE_DATA_PITCH_ALIGNMENT);
        let upload_size = u64::from(upload_row_pitch)
            .checked_mul(u64::from(extent.height))
            .ok_or(RhiError::InvalidDescription("texture upload size overflow"))?;

        self.wait_idle_native()?;
        let (device, queue, texture, old_state) = {
            let native = self.native_ref()?;
            let texture = native
                .textures
                .get(&handle)
                .ok_or(RhiError::InvalidHandle)?;
            (
                native.device.clone(),
                native.queue.clone(),
                texture.resource.clone(),
                texture.state,
            )
        };
        let staging = context::create_upload_buffer(&device, upload_size, "rhi.texture-upload")?;
        unsafe {
            let mut mapped = std::ptr::null_mut();
            staging
                .Map(0, None, Some(&mut mapped))
                .map_err(|error| backend_error("map texture upload buffer", error))?;
            let mapped = mapped.cast::<u8>();
            for row in 0..extent.height as usize {
                let source = data.as_ptr().add(row * row_pitch as usize);
                let destination = mapped.add(row * upload_row_pitch as usize);
                std::ptr::copy_nonoverlapping(source, destination, minimum_row_pitch as usize);
            }
            staging.Unmap(0, None);
        }

        let allocator = unsafe {
            device
                .CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(|error| backend_error("create texture upload allocator", error))?
        };
        let list = unsafe {
            device
                .CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
                    0,
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                    &allocator,
                    None::<&ID3D12PipelineState>,
                )
                .map_err(|error| backend_error("create texture upload command list", error))?
        };
        unsafe {
            if old_state != D3D12_RESOURCE_STATE_COPY_DEST {
                transition_resource(&list, &texture, old_state, D3D12_RESOURCE_STATE_COPY_DEST);
            }
            let mut source_location = D3D12_TEXTURE_COPY_LOCATION {
                pResource: ManuallyDrop::new(Some(staging.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: context::texture_format(desc.format)?,
                            Width: extent.width,
                            Height: extent.height,
                            Depth: 1,
                            RowPitch: upload_row_pitch,
                        },
                    },
                },
            };
            let mut destination_location = D3D12_TEXTURE_COPY_LOCATION {
                pResource: ManuallyDrop::new(Some(texture.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            list.CopyTextureRegion(
                &destination_location,
                origin[0],
                origin[1],
                0,
                &source_location,
                None,
            );
            let final_state = if desc.usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
                D3D12_RESOURCE_STATE_DEPTH_WRITE
            } else {
                D3D12_RESOURCE_STATE_ALL_SHADER_RESOURCE
            };
            if final_state != D3D12_RESOURCE_STATE_COPY_DEST {
                transition_resource(&list, &texture, D3D12_RESOURCE_STATE_COPY_DEST, final_state);
            }
            ManuallyDrop::drop(&mut source_location.pResource);
            ManuallyDrop::drop(&mut destination_location.pResource);
            list.Close()
                .map_err(|error| backend_error("close texture upload command list", error))?;
            let command_list: ID3D12CommandList = list
                .cast()
                .map_err(|error| backend_error("cast texture upload command list", error))?;
            queue.ExecuteCommandLists(&[Some(command_list)]);
        }
        self.wait_idle_native()?;
        if let Some(texture) = self.native_mut()?.textures.get_mut(&handle) {
            texture.state = if desc.usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
                D3D12_RESOURCE_STATE_DEPTH_WRITE
            } else {
                D3D12_RESOURCE_STATE_ALL_SHADER_RESOURCE
            };
        }
        Ok(())
    }

    pub(crate) fn wait_idle_native(&mut self) -> RhiResult<()> {
        self.native_mut()?.wait_idle()
    }

    pub(crate) fn destroy_buffer_native(&mut self, handle: BufferHandle) -> RhiResult<()> {
        self.resources.destroy_buffer(handle)?;
        let native = self.native_mut()?;
        // A queued upload has not reached the GPU yet. Dropping its staging
        // resource is safe when the destination is destroyed before command
        // recording starts.
        native
            .pending_buffer_uploads
            .retain(|upload| upload.destination != handle);
        if let Some(resource) = native.buffers.remove(&handle) {
            // Keep resources alive until the newest in-flight submission has
            // completed. This avoids the old device-wide idle wait while a
            // chunk mesh is replaced.
            if let Some(submission) = native.pending.last_mut() {
                submission.deferred_buffers.push(resource);
            }
        }
        Ok(())
    }

    pub(crate) fn destroy_texture_native(&mut self, handle: TextureHandle) -> RhiResult<()> {
        self.wait_idle_native()?;
        self.resources.destroy_texture(handle)?;
        self.native_mut()?.textures.remove(&handle);
        Ok(())
    }

    pub(crate) fn destroy_shader_native(&mut self, handle: ShaderHandle) -> RhiResult<()> {
        self.wait_idle_native()?;
        self.resources.destroy_shader(handle)?;
        self.native_mut()?.shaders.remove(&handle);
        Ok(())
    }

    pub(crate) fn destroy_pipeline_native(&mut self, handle: PipelineHandle) -> RhiResult<()> {
        self.wait_idle_native()?;
        self.resources.destroy_pipeline(handle)?;
        self.native_mut()?.pipelines.remove(&handle);
        Ok(())
    }

    pub(crate) fn destroy_sampler_native(&mut self, handle: SamplerHandle) -> RhiResult<()> {
        self.wait_idle_native()?;
        self.resources.destroy_sampler(handle)?;
        self.native_mut()?.samplers.remove(&handle);
        Ok(())
    }

    pub(crate) fn destroy_bind_group_native(&mut self, handle: BindGroupHandle) -> RhiResult<()> {
        self.wait_idle_native()?;
        self.resources.destroy_bind_group(handle)?;
        self.native_mut()?.bind_groups.remove(&handle);
        Ok(())
    }
}

impl CommandQueue for DirectX12Backend {
    fn submit(&mut self, command_buffer: CommandBuffer) -> RhiResult<SubmissionId> {
        let token = context::command_buffer_token(&command_buffer)?;
        let commands = command_buffer.commands();
        let mut submission =
            unsafe { Box::from_raw(token as usize as *mut context::NativeSubmission) };
        let command_list: ID3D12CommandList = submission
            .command_list
            .cast()
            .map_err(|error| backend_error("cast Direct3D 12 command list", error))?;
        let deferred_buffers = std::mem::take(&mut submission._deferred_buffers);
        let fence_value = {
            let native = self.native_mut()?;
            unsafe {
                native.queue.ExecuteCommandLists(&[Some(command_list)]);
                native
                    .queue
                    .Signal(&native.fence, native.next_fence)
                    .map_err(|error| backend_error("signal Direct3D 12 submission", error))?;
            }
            update_native_resource_states(native, commands);
            let value = native.next_fence;
            native.next_fence = native.next_fence.saturating_add(1);
            native.pending.push(context::PendingSubmission {
                fence_value: value,
                _submission: submission,
                deferred_buffers,
            });
            if let Some(surface) = native.surface.as_mut() {
                if let Some(image_index) = surface.acquired_image {
                    if let Some(image_fence) = surface.fence_values.get_mut(image_index as usize) {
                        *image_fence = value;
                    }
                    surface.submitted = true;
                }
            }
            value
        };
        let id = SubmissionId(self.next_submission);
        self.next_submission = self.next_submission.saturating_add(1);
        let _ = fence_value;
        Ok(id)
    }

    fn collect_completed(&mut self) -> RhiResult<()> {
        self.native_mut()?.collect_completed();
        Ok(())
    }

    fn wait_idle(&mut self) -> RhiResult<()> {
        self.wait_idle_native()
    }
}

impl RenderDevice for DirectX12Backend {
    fn capabilities(&self) -> DeviceCapabilities {
        self.capabilities
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> RhiResult<BufferHandle> {
        self.create_buffer_native(desc)
    }

    fn create_texture(&mut self, desc: TextureDesc) -> RhiResult<TextureHandle> {
        self.create_texture_native(desc)
    }

    fn create_shader(&mut self, desc: ShaderDesc) -> RhiResult<ShaderHandle> {
        self.create_shader_native(desc)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc) -> RhiResult<PipelineHandle> {
        self.create_pipeline_native(desc)
    }

    fn create_sampler(&mut self, desc: SamplerDesc) -> RhiResult<SamplerHandle> {
        self.create_sampler_native(desc)
    }

    fn create_bind_group(&mut self, desc: BindGroupDesc) -> RhiResult<BindGroupHandle> {
        self.create_bind_group_native(desc)
    }

    fn configure_surface(&mut self, config: SurfaceConfig) -> RhiResult<()> {
        configure_surface(self, config)
    }

    fn acquire_surface_frame(&mut self) -> RhiResult<SurfaceFrame> {
        acquire_surface_frame(self)
    }

    fn present(&mut self, frame: SurfaceFrame) -> RhiResult<()> {
        present_surface(self, frame)
    }

    fn upload_texture(
        &mut self,
        handle: TextureHandle,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()> {
        self.upload_texture_native(handle, data, row_pitch)
    }

    fn upload_texture_region(
        &mut self,
        handle: TextureHandle,
        origin: [u32; 2],
        extent: Extent2D,
        data: &[u8],
        row_pitch: u32,
    ) -> RhiResult<()> {
        self.upload_texture_region_native(handle, origin, extent, data, row_pitch)
    }

    fn upload_buffer(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> RhiResult<()> {
        self.upload_buffer_native(handle, offset, data)
    }

    fn destroy_buffer(&mut self, handle: BufferHandle) -> RhiResult<()> {
        self.destroy_buffer_native(handle)
    }

    fn destroy_texture(&mut self, handle: TextureHandle) -> RhiResult<()> {
        self.destroy_texture_native(handle)
    }

    fn destroy_shader(&mut self, handle: ShaderHandle) -> RhiResult<()> {
        self.destroy_shader_native(handle)
    }

    fn destroy_pipeline(&mut self, handle: PipelineHandle) -> RhiResult<()> {
        self.destroy_pipeline_native(handle)
    }

    fn destroy_sampler(&mut self, handle: SamplerHandle) -> RhiResult<()> {
        self.destroy_sampler_native(handle)
    }

    fn destroy_bind_group(&mut self, handle: BindGroupHandle) -> RhiResult<()> {
        self.destroy_bind_group_native(handle)
    }

    fn begin_commands(&mut self, label: &str) -> RhiResult<Box<dyn CommandEncoder>> {
        command::create_command_encoder(self, label)
    }
}

fn texture_clear_value(desc: &TextureDesc) -> RhiResult<Option<D3D12_CLEAR_VALUE>> {
    let format = context::texture_format(desc.format)?;
    if desc.usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
        Ok(Some(D3D12_CLEAR_VALUE {
            Format: format,
            Anonymous: D3D12_CLEAR_VALUE_0 {
                DepthStencil: D3D12_DEPTH_STENCIL_VALUE {
                    Depth: 1.0,
                    Stencil: 0,
                },
            },
        }))
    } else if desc.usage.contains(TextureUsage::COLOR_ATTACHMENT) {
        Ok(Some(D3D12_CLEAR_VALUE {
            Format: format,
            Anonymous: D3D12_CLEAR_VALUE_0 { Color: [0.0; 4] },
        }))
    } else {
        Ok(None)
    }
}

fn create_attachment_views(
    native: &mut context::NativeState,
    resource: &ID3D12Resource,
    desc: &TextureDesc,
) -> RhiResult<(
    Option<D3D12_CPU_DESCRIPTOR_HANDLE>,
    Option<D3D12_CPU_DESCRIPTOR_HANDLE>,
)> {
    let format = context::texture_format(desc.format)?;
    let rtv = if desc.usage.contains(TextureUsage::COLOR_ATTACHMENT) {
        let index = native.rtv_heap.allocate(1)?;
        let handle = native.rtv_heap.cpu(index);
        let view = D3D12_RENDER_TARGET_VIEW_DESC {
            Format: format,
            ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
            Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_RTV {
                    MipSlice: 0,
                    PlaneSlice: 0,
                },
            },
        };
        unsafe {
            native
                .device
                .CreateRenderTargetView(resource, Some(&view), handle)
        };
        Some(handle)
    } else {
        None
    };
    let dsv = if desc.usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
        let index = native.dsv_heap.allocate(1)?;
        let handle = native.dsv_heap.cpu(index);
        let view = D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: format,
            ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_DSV { MipSlice: 0 },
            },
        };
        unsafe {
            native
                .device
                .CreateDepthStencilView(resource, Some(&view), handle)
        };
        Some(handle)
    } else {
        None
    };
    Ok((rtv, dsv))
}

fn write_bind_group_descriptors(
    backend: &DirectX12Backend,
    pipeline_desc: &PipelineDesc,
    desc: &BindGroupDesc,
    resource_offset: u32,
    sampler_offset: u32,
) -> RhiResult<()> {
    let native = backend.native_ref()?;
    for entry in &desc.entries {
        if entry.binding >= MAX_BINDINGS {
            return Err(RhiError::InvalidDescription(
                "Direct3D 12 bind-group binding exceeds the fixed descriptor table",
            ));
        }
        let declared_type = pipeline_binding_type(native, pipeline_desc, desc.set, entry.binding);
        match (declared_type, entry.resource) {
            (
                Some(ShaderBindingType::UniformBuffer),
                BindingResource::Buffer {
                    buffer,
                    offset,
                    size,
                },
            )
            | (
                None,
                BindingResource::Buffer {
                    buffer,
                    offset,
                    size,
                },
            ) => {
                let native_buffer = native.buffers.get(&buffer).ok_or(RhiError::InvalidHandle)?;
                validate_buffer_binding(native_buffer.size, offset, size)?;
                let view_size = u64::from(context::aligned_constant_size(size)?);
                if !offset.is_multiple_of(256)
                    || offset > native_buffer.resource_size
                    || view_size > native_buffer.resource_size - offset
                {
                    return Err(RhiError::InvalidDescription(
                        "uniform-buffer binding is not aligned or exceeds the Direct3D 12 resource",
                    ));
                }
                let view = D3D12_CONSTANT_BUFFER_VIEW_DESC {
                    BufferLocation: unsafe { native_buffer.resource.GetGPUVirtualAddress() }
                        + offset,
                    SizeInBytes: u32::try_from(view_size).map_err(|_| {
                        RhiError::InvalidDescription("uniform-buffer binding is too large")
                    })?,
                };
                unsafe {
                    native.device.CreateConstantBufferView(
                        Some(&view),
                        native.resource_heap.cpu(resource_offset + entry.binding),
                    );
                }
            }
            (
                Some(ShaderBindingType::StorageBuffer),
                BindingResource::Buffer {
                    buffer,
                    offset,
                    size,
                },
            ) => {
                let native_buffer = native.buffers.get(&buffer).ok_or(RhiError::InvalidHandle)?;
                validate_buffer_binding(native_buffer.size, offset, size)?;
                let element_count = checked_u32(
                    usize::try_from(size / 4).unwrap_or(usize::MAX),
                    "storage buffer is too large",
                )?;
                let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_TYPELESS,
                    ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Buffer: D3D12_BUFFER_SRV {
                            FirstElement: offset / 4,
                            NumElements: element_count,
                            StructureByteStride: 0,
                            Flags: D3D12_BUFFER_SRV_FLAG_RAW,
                        },
                    },
                };
                unsafe {
                    native.device.CreateShaderResourceView(
                        &native_buffer.resource,
                        Some(&view),
                        native
                            .resource_heap
                            .cpu(resource_offset + MAX_BINDINGS + entry.binding),
                    );
                }
            }
            (Some(ShaderBindingType::StorageBuffer), _) => {
                return Err(RhiError::InvalidDescription(
                    "storage-buffer binding requires a buffer resource",
                ));
            }
            (Some(ShaderBindingType::UniformBuffer), _) => {
                return Err(RhiError::InvalidDescription(
                    "uniform-buffer binding requires a buffer resource",
                ));
            }
            (Some(ShaderBindingType::SampledTexture), BindingResource::Texture { texture })
            | (
                Some(ShaderBindingType::CombinedImageSampler),
                BindingResource::Texture { texture },
            ) => {
                write_texture_srv(
                    native,
                    resource_offset + MAX_BINDINGS + entry.binding,
                    texture,
                )?;
            }
            (
                Some(ShaderBindingType::CombinedImageSampler),
                BindingResource::CombinedImageSampler { texture, sampler },
            )
            | (None, BindingResource::CombinedImageSampler { texture, sampler }) => {
                write_texture_srv(
                    native,
                    resource_offset + MAX_BINDINGS + entry.binding,
                    texture,
                )?;
                write_sampler(native, sampler_offset + entry.binding, sampler)?;
            }
            (
                Some(ShaderBindingType::SampledTexture),
                BindingResource::CombinedImageSampler { texture, .. },
            ) => {
                write_texture_srv(
                    native,
                    resource_offset + MAX_BINDINGS + entry.binding,
                    texture,
                )?;
            }
            (
                Some(ShaderBindingType::StorageTexture),
                BindingResource::StorageTexture { texture },
            ) => {
                write_texture_uav(
                    native,
                    resource_offset + MAX_BINDINGS * 2 + entry.binding,
                    texture,
                )?;
            }
            (Some(ShaderBindingType::Sampler), BindingResource::Sampler { sampler }) => {
                write_sampler(native, sampler_offset + entry.binding, sampler)?;
            }
            (Some(ShaderBindingType::SampledTexture), _)
            | (Some(ShaderBindingType::StorageTexture), _)
            | (Some(ShaderBindingType::Sampler), _)
            | (Some(ShaderBindingType::CombinedImageSampler), _)
            | (Some(ShaderBindingType::AccelerationStructure), _) => {
                return Err(RhiError::InvalidDescription(
                    "bind-group resource type does not match the shader interface",
                ));
            }
            (None, BindingResource::Texture { texture }) => {
                write_texture_srv(
                    native,
                    resource_offset + MAX_BINDINGS + entry.binding,
                    texture,
                )?;
            }
            (None, BindingResource::StorageTexture { texture }) => {
                write_texture_uav(
                    native,
                    resource_offset + MAX_BINDINGS * 2 + entry.binding,
                    texture,
                )?;
            }
            (None, BindingResource::Sampler { sampler }) => {
                write_sampler(native, sampler_offset + entry.binding, sampler)?;
            }
        }
    }
    Ok(())
}

fn pipeline_binding_type(
    native: &context::NativeState,
    pipeline: &PipelineDesc,
    set: u32,
    binding: u32,
) -> Option<ShaderBindingType> {
    let shaders = [
        pipeline.vertex_shader,
        pipeline.fragment_shader,
        pipeline.geometry_shader,
        pipeline.tessellation_control_shader,
        pipeline.tessellation_evaluation_shader,
        pipeline.compute_shader,
    ];
    shaders.into_iter().flatten().find_map(|handle| {
        native
            .shaders
            .get(&handle)
            .and_then(|shader| shader.interface.binding(set, binding))
            .map(|binding| binding.ty)
    })
}

fn validate_buffer_binding(buffer_size: u64, offset: u64, size: u64) -> RhiResult<()> {
    if offset > buffer_size || size > buffer_size - offset || size == 0 {
        return Err(RhiError::InvalidDescription(
            "bind-group buffer range exceeds the buffer",
        ));
    }
    Ok(())
}

fn write_texture_srv(
    native: &context::NativeState,
    descriptor_index: u32,
    texture: TextureHandle,
) -> RhiResult<()> {
    let texture = native
        .textures
        .get(&texture)
        .ok_or(RhiError::InvalidHandle)?;
    let mut format = context::texture_format(texture.desc.format)?;
    if texture.desc.usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
        format = DXGI_FORMAT_R32_FLOAT;
    }
    let (dimension, view) = if texture.desc.array_layers > 1 {
        (
            D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
            D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                    MostDetailedMip: 0,
                    MipLevels: texture.desc.mip_levels,
                    FirstArraySlice: 0,
                    ArraySize: texture.desc.array_layers,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        )
    } else {
        (
            D3D12_SRV_DIMENSION_TEXTURE2D,
            D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: texture.desc.mip_levels,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        )
    };
    let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: dimension,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: view,
    };
    unsafe {
        native.device.CreateShaderResourceView(
            &texture.resource,
            Some(&view),
            native.resource_heap.cpu(descriptor_index),
        );
    }
    Ok(())
}

fn write_texture_uav(
    native: &context::NativeState,
    descriptor_index: u32,
    texture: TextureHandle,
) -> RhiResult<()> {
    let texture = native
        .textures
        .get(&texture)
        .ok_or(RhiError::InvalidHandle)?;
    let view = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: context::texture_format(texture.desc.format)?,
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_UAV {
                MipSlice: 0,
                PlaneSlice: 0,
            },
        },
    };
    unsafe {
        native.device.CreateUnorderedAccessView(
            &texture.resource,
            None::<&ID3D12Resource>,
            Some(&view),
            native.resource_heap.cpu(descriptor_index),
        );
    }
    Ok(())
}

fn write_sampler(
    native: &context::NativeState,
    descriptor_index: u32,
    sampler: SamplerHandle,
) -> RhiResult<()> {
    let sampler = native
        .samplers
        .get(&sampler)
        .ok_or(RhiError::InvalidHandle)?;
    let desc = D3D12_SAMPLER_DESC {
        Filter: if sampler.desc.linear_filter {
            D3D12_FILTER_MIN_MAG_MIP_LINEAR
        } else {
            D3D12_FILTER_MIN_MAG_MIP_POINT
        },
        AddressU: if sampler.desc.repeat_u {
            D3D12_TEXTURE_ADDRESS_MODE_WRAP
        } else {
            D3D12_TEXTURE_ADDRESS_MODE_CLAMP
        },
        AddressV: if sampler.desc.repeat_v {
            D3D12_TEXTURE_ADDRESS_MODE_WRAP
        } else {
            D3D12_TEXTURE_ADDRESS_MODE_CLAMP
        },
        AddressW: if sampler.desc.repeat_w {
            D3D12_TEXTURE_ADDRESS_MODE_WRAP
        } else {
            D3D12_TEXTURE_ADDRESS_MODE_CLAMP
        },
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        // This is a regular (non-comparison) sampler.  D3D12 ignores the
        // comparison function for this filter, but NEVER is the neutral
        // value and avoids the debug-layer warning about ALWAYS.
        ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
        BorderColor: [0.0; 4],
        MinLOD: 0.0,
        MaxLOD: f32::MAX,
    };
    unsafe {
        native
            .device
            .CreateSampler(&desc, native.sampler_heap.cpu(descriptor_index));
    }
    Ok(())
}

fn validate_texture_upload(
    desc: &TextureDesc,
    origin: [u32; 2],
    extent: Extent2D,
    data: &[u8],
    row_pitch: u32,
) -> RhiResult<()> {
    if desc.mip_levels != 1 || desc.array_layers != 1 {
        return Err(RhiError::InvalidDescription(
            "Direct3D 12 texture uploads currently target one mip and one array layer",
        ));
    }
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
    let bytes_per_pixel = texture_bytes_per_pixel(desc.format)?;
    let minimum_row_pitch = extent
        .width
        .checked_mul(bytes_per_pixel)
        .ok_or(RhiError::InvalidDescription("texture row pitch overflow"))?;
    if row_pitch < minimum_row_pitch || !row_pitch.is_multiple_of(bytes_per_pixel) {
        return Err(RhiError::InvalidDescription("invalid texture row pitch"));
    }
    let required = usize::try_from(row_pitch)
        .ok()
        .and_then(|pitch| pitch.checked_mul(extent.height as usize))
        .ok_or(RhiError::InvalidDescription("texture upload size overflow"))?;
    if data.len() < required {
        return Err(RhiError::InvalidDescription(
            "texture upload data is smaller than the upload region",
        ));
    }
    Ok(())
}

fn texture_bytes_per_pixel(format: TextureFormat) -> RhiResult<u32> {
    Ok(match format {
        TextureFormat::R8Unorm => 1,
        TextureFormat::R16Float => 2,
        TextureFormat::R8G8B8A8Unorm
        | TextureFormat::Rgba8Unorm
        | TextureFormat::R8G8B8A8Srgb
        | TextureFormat::Rgba8Srgb
        | TextureFormat::B8G8R8A8Srgb
        | TextureFormat::R16G16Float
        | TextureFormat::D24UnormS8Uint
        | TextureFormat::D32Float => 4,
        TextureFormat::R16G16B16A16Float | TextureFormat::Rgba16Float => 8,
    })
}

fn align_up(value: u32, alignment: u32) -> u32 {
    value.saturating_add(alignment - 1) / alignment * alignment
}

fn configure_surface(backend: &mut DirectX12Backend, config: SurfaceConfig) -> RhiResult<()> {
    if config.extent.is_empty() {
        return Err(RhiError::InvalidDescription(
            "surface extent must be non-zero",
        ));
    }
    if !matches!(
        config.format,
        TextureFormat::B8G8R8A8Srgb
            | TextureFormat::R8G8B8A8Srgb
            | TextureFormat::R8G8B8A8Unorm
            | TextureFormat::Rgba8Unorm
    ) {
        return Err(RhiError::InvalidDescription(
            "Direct3D 12 swapchains currently support 8-bit RGBA formats",
        ));
    }
    if backend
        .native_ref()?
        .surface
        .as_ref()
        .is_some_and(|surface| surface.config == config)
    {
        return Ok(());
    }

    let old_surface = {
        backend.wait_idle_native()?;
        backend.native_mut()?.surface.take()
    };
    if let Some(old_surface) = old_surface {
        for handle in old_surface.textures {
            backend.native_mut()?.textures.remove(&handle);
            let _ = backend.resources.destroy_texture(handle);
        }
    }

    let (factory, queue, device, hwnd, frame_count) = {
        let native = backend.native_ref()?;
        (
            native.factory.clone(),
            native.queue.clone(),
            native.device.clone(),
            native.window.ok_or_else(|| {
                RhiError::Backend(String::from(
                    "Direct3D 12 surface configuration requires a Win32 window",
                ))
            })?,
            backend.config.frame_count,
        )
    };
    let dxgi_format = context::texture_format(config.format)?;
    let swapchain_format = context::swapchain_format(config.format)?;
    let swapchain_desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: config.extent.width,
        Height: config.extent.height,
        Format: swapchain_format,
        Stereo: BOOL(0),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: frame_count,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: 0,
    };
    let swapchain = unsafe {
        factory
            .CreateSwapChainForHwnd::<_, _>(
                &queue,
                hwnd,
                &swapchain_desc,
                None,
                None::<&IDXGIOutput>,
            )
            .map_err(|error| {
                let debug_messages = context::debug_layer_messages(&device);
                if debug_messages.is_empty() {
                    backend_error("create Direct3D 12 swapchain", error)
                } else {
                    backend_error(
                        "create Direct3D 12 swapchain",
                        format!("{error}; debug layer: {}", debug_messages.join(" | ")),
                    )
                }
            })?
            .cast::<IDXGISwapChain3>()
            .map_err(|error| backend_error("cast Direct3D 12 swapchain", error))?
    };

    let mut native_textures = Vec::with_capacity(frame_count as usize);
    let texture_desc = TextureDesc {
        extent: config.extent,
        format: config.format,
        usage: TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_DST | TextureUsage::PRESENT,
        mip_levels: 1,
        array_layers: 1,
        label: String::from("dx12.swapchain-image"),
    };
    for index in 0..frame_count {
        let resource = unsafe {
            swapchain
                .GetBuffer::<ID3D12Resource>(index)
                .map_err(|error| backend_error("get Direct3D 12 swapchain buffer", error))?
        };
        let handle = backend
            .resources
            .create_external_texture(texture_desc.clone())?;
        let rtv = {
            let native = backend.native_mut()?;
            let descriptor_index = native.rtv_heap.allocate(1)?;
            let rtv = native.rtv_heap.cpu(descriptor_index);
            let view = D3D12_RENDER_TARGET_VIEW_DESC {
                Format: dxgi_format,
                ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
                Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 {
                    Texture2D: D3D12_TEX2D_RTV {
                        MipSlice: 0,
                        PlaneSlice: 0,
                    },
                },
            };
            unsafe { device.CreateRenderTargetView(&resource, Some(&view), rtv) };
            rtv
        };
        native_textures.push((
            handle,
            NativeTexture {
                resource,
                desc: texture_desc.clone(),
                state: D3D12_RESOURCE_STATE_COMMON,
                rtv: Some(rtv),
                dsv: None,
            },
        ));
    }
    let handles = native_textures
        .iter()
        .map(|(handle, _)| *handle)
        .collect::<Vec<_>>();
    let native = backend.native_mut()?;
    for (handle, texture) in native_textures {
        native.textures.insert(handle, texture);
    }
    native.surface = Some(context::SurfaceState {
        swapchain,
        textures: handles,
        fence_values: vec![0; frame_count as usize],
        config,
        acquired_image: None,
        submitted: false,
    });
    Ok(())
}

fn update_native_resource_states(native: &mut context::NativeState, commands: &[Command]) {
    for command in commands {
        let Command::Barrier(barrier) = command else {
            continue;
        };
        match barrier.resource {
            ResourceHandle::Buffer(handle) => {
                if let Some(buffer) = native.buffers.get_mut(&handle) {
                    buffer.state = context::resource_state(barrier.after);
                }
            }
            ResourceHandle::Texture(handle) => {
                if let Some(texture) = native.textures.get_mut(&handle) {
                    texture.state = context::resource_state(barrier.after);
                }
            }
            ResourceHandle::Shader(_)
            | ResourceHandle::Pipeline(_)
            | ResourceHandle::Sampler(_)
            | ResourceHandle::BindGroup(_)
            | ResourceHandle::RenderTarget(_) => {}
        }
    }
}

fn acquire_surface_frame(backend: &mut DirectX12Backend) -> RhiResult<SurfaceFrame> {
    let (image_index, texture, wait_value, fence, fence_event) = {
        let native = backend.native_ref()?;
        let surface = native.surface.as_ref().ok_or_else(|| {
            RhiError::Backend(String::from("Direct3D 12 surface is not configured"))
        })?;
        if surface.acquired_image.is_some() {
            return Err(RhiError::Message(String::from(
                "a Direct3D 12 surface image is already acquired",
            )));
        }
        let image_index = unsafe { surface.swapchain.GetCurrentBackBufferIndex() };
        let texture = surface
            .textures
            .get(image_index as usize)
            .copied()
            .ok_or_else(|| {
                RhiError::Backend(String::from("Direct3D 12 returned an invalid image index"))
            })?;
        let wait_value = surface
            .fence_values
            .get(image_index as usize)
            .copied()
            .ok_or_else(|| {
                RhiError::Backend(String::from("Direct3D 12 returned an invalid image index"))
            })?;
        (
            image_index,
            texture,
            wait_value,
            native.fence.clone(),
            native.fence_event,
        )
    };
    if wait_value != 0 {
        unsafe {
            if fence.GetCompletedValue() < wait_value {
                fence
                    .SetEventOnCompletion(wait_value, fence_event)
                    .map_err(|error| backend_error("arm Direct3D 12 frame fence", error))?;
                let wait = WaitForSingleObject(fence_event, INFINITE);
                if wait.0 != 0 {
                    return Err(RhiError::Backend(format!(
                        "wait for Direct3D 12 frame fence returned {wait:?}"
                    )));
                }
            }
        }
    }
    let native = backend.native_mut()?;
    let surface = native
        .surface
        .as_mut()
        .ok_or_else(|| RhiError::Backend(String::from("Direct3D 12 surface is not configured")))?;
    surface.acquired_image = Some(image_index);
    surface.submitted = false;
    Ok(SurfaceFrame {
        texture,
        image_index,
    })
}

fn present_surface(backend: &mut DirectX12Backend, frame: SurfaceFrame) -> RhiResult<()> {
    let native = backend.native_mut()?;
    let surface = native
        .surface
        .as_mut()
        .ok_or_else(|| RhiError::Backend(String::from("Direct3D 12 surface is not configured")))?;
    if surface.acquired_image != Some(frame.image_index)
        || surface.textures.get(frame.image_index as usize).copied() != Some(frame.texture)
    {
        return Err(RhiError::InvalidHandle);
    }
    if !surface.submitted {
        return Err(RhiError::Message(String::from(
            "Direct3D 12 surface frame was presented without a submission",
        )));
    }
    unsafe {
        surface
            .swapchain
            .Present(if surface.config.vsync { 1 } else { 0 }, DXGI_PRESENT(0))
            .ok()
            .map_err(|error| backend_error("present Direct3D 12 swapchain", error))?;
    }
    surface.acquired_image = None;
    surface.submitted = false;
    Ok(())
}

unsafe fn transition_resource(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    if before == after {
        return;
    }
    let transition = D3D12_RESOURCE_TRANSITION_BARRIER {
        pResource: ManuallyDrop::new(Some(resource.clone())),
        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
        StateBefore: before,
        StateAfter: after,
    };
    let mut barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(transition),
        },
    };
    list.ResourceBarrier(std::slice::from_ref(&barrier));
    ManuallyDrop::drop(&mut barrier.Anonymous.Transition);
}
