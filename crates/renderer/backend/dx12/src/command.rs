use std::collections::HashMap;
use std::mem::ManuallyDrop;

use renderer::rhi::command::*;
use renderer::rhi::types::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_UINT;

use super::context::{self, NativeBindGroup, NativeBuffer, NativePipeline, NativeTexture};
use super::pipeline;
use super::{backend_error, DirectX12Backend};

pub(crate) struct DirectX12CommandEncoder {
    allocator: windows::Win32::Graphics::Direct3D12::ID3D12CommandAllocator,
    command_list: windows::Win32::Graphics::Direct3D12::ID3D12GraphicsCommandList,
    resource_heap: context::DescriptorHeapState,
    sampler_heap: context::DescriptorHeapState,
    buffers: HashMap<BufferHandle, NativeBuffer>,
    textures: HashMap<TextureHandle, NativeTexture>,
    pipelines: HashMap<PipelineHandle, NativePipeline>,
    bind_groups: HashMap<BindGroupHandle, NativeBindGroup>,
    label: String,
    commands: Vec<Command>,
    render_pass_open: bool,
    pipeline_bound: Option<PipelineHandle>,
    deferred_buffers: Vec<NativeBuffer>,
}

pub(crate) fn create_command_encoder(
    backend: &mut DirectX12Backend,
    label: &str,
) -> RhiResult<Box<dyn CommandEncoder>> {
    let pending_uploads = {
        let native = backend.native_mut()?;
        std::mem::take(&mut native.pending_buffer_uploads)
    };
    let native = backend.native_ref()?;
    let allocator = unsafe {
        native
            .device
            .CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
            .map_err(|error| backend_error("create Direct3D 12 command allocator", error))?
    };
    let command_list = unsafe {
        native
            .device
            .CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None::<&ID3D12PipelineState>,
            )
            .map_err(|error| backend_error("create Direct3D 12 command list", error))?
    };
    let mut encoder = DirectX12CommandEncoder {
        allocator,
        command_list,
        resource_heap: native.resource_heap.clone(),
        sampler_heap: native.sampler_heap.clone(),
        buffers: native.buffers.clone(),
        textures: native.textures.clone(),
        pipelines: native.pipelines.clone(),
        bind_groups: native.bind_groups.clone(),
        label: label.to_string(),
        commands: Vec::new(),
        render_pass_open: false,
        pipeline_bound: None,
        deferred_buffers: Vec::new(),
    };
    for upload in pending_uploads {
        encoder.record_pending_buffer_upload(upload)?;
    }
    Ok(Box::new(encoder))
}

impl DirectX12CommandEncoder {
    fn require_render_pass(&self) -> RhiResult<()> {
        if self.render_pass_open {
            Ok(())
        } else {
            Err(RhiError::Message(String::from(
                "draw commands require an open render pass",
            )))
        }
    }

    fn require_no_render_pass(&self) -> RhiResult<()> {
        if self.render_pass_open {
            Err(RhiError::Message(String::from(
                "the command is not valid inside a render pass",
            )))
        } else {
            Ok(())
        }
    }

    fn resource_texture(&self, handle: TextureHandle) -> RhiResult<&NativeTexture> {
        self.textures.get(&handle).ok_or(RhiError::InvalidHandle)
    }

    fn resource_buffer(&self, handle: BufferHandle) -> RhiResult<&NativeBuffer> {
        self.buffers.get(&handle).ok_or(RhiError::InvalidHandle)
    }

    fn bound_native_pipeline(&self) -> RhiResult<&NativePipeline> {
        self.pipeline_bound
            .and_then(|handle| self.pipelines.get(&handle))
            .ok_or_else(|| RhiError::Message(String::from("no Direct3D 12 pipeline is bound")))
    }

    fn transition(
        &mut self,
        resource: ResourceHandle,
        _requested_before: ResourceState,
        requested_after: ResourceState,
    ) -> RhiResult<()> {
        let (native_resource, actual_before, slot) = match resource {
            ResourceHandle::Texture(handle) => {
                let texture = self.textures.get(&handle).ok_or(RhiError::InvalidHandle)?;
                (
                    texture.resource.clone(),
                    texture.state,
                    ResourceSlot::Texture(handle),
                )
            }
            ResourceHandle::Buffer(handle) => {
                let buffer = self.buffers.get(&handle).ok_or(RhiError::InvalidHandle)?;
                (
                    buffer.resource.clone(),
                    buffer.state,
                    ResourceSlot::Buffer(handle),
                )
            }
            _ => {
                return Err(RhiError::InvalidDescription(
                    "Direct3D 12 barriers only support buffers and textures",
                ))
            }
        };
        let requested_after = context::resource_state(requested_after);
        if actual_before != requested_after {
            unsafe {
                let transition = D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: ManuallyDrop::new(Some(native_resource)),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: actual_before,
                    StateAfter: requested_after,
                };
                let mut barrier = D3D12_RESOURCE_BARRIER {
                    Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                    Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                    Anonymous: D3D12_RESOURCE_BARRIER_0 {
                        Transition: ManuallyDrop::new(transition),
                    },
                };
                self.command_list
                    .ResourceBarrier(std::slice::from_ref(&barrier));
                ManuallyDrop::drop(&mut barrier.Anonymous.Transition);
            }
        }
        match slot {
            ResourceSlot::Texture(handle) => {
                if let Some(texture) = self.textures.get_mut(&handle) {
                    texture.state = requested_after;
                }
            }
            ResourceSlot::Buffer(handle) => {
                if let Some(buffer) = self.buffers.get_mut(&handle) {
                    buffer.state = requested_after;
                }
            }
        }
        Ok(())
    }

    fn set_descriptor_heaps(&self) {
        unsafe {
            self.command_list.SetDescriptorHeaps(&[
                Some(self.resource_heap.heap.clone()),
                Some(self.sampler_heap.heap.clone()),
            ]);
        }
    }

    fn record_pending_buffer_upload(
        &mut self,
        upload: context::PendingBufferUpload,
    ) -> RhiResult<()> {
        let destination = self.resource_buffer(upload.destination)?.clone();
        let destination_end = upload
            .destination_offset
            .checked_add(upload.size)
            .ok_or(RhiError::InvalidDescription("buffer upload range overflow"))?;
        if upload.size == 0 || destination_end > destination.size {
            return Err(RhiError::InvalidDescription(
                "buffer upload exceeds the destination buffer",
            ));
        }
        self.transition(
            ResourceHandle::Buffer(upload.destination),
            ResourceState::Undefined,
            ResourceState::CopyDst,
        )?;
        unsafe {
            self.command_list.CopyBufferRegion(
                &destination.resource,
                upload.destination_offset,
                &upload.staging.resource,
                0,
                upload.size,
            );
        }
        let destination_state = if destination.usage.contains(BufferUsage::INDEX) {
            ResourceState::IndexBuffer
        } else if destination.usage.contains(BufferUsage::VERTEX) {
            ResourceState::VertexBuffer
        } else if destination.usage.contains(BufferUsage::UNIFORM) {
            ResourceState::UniformBuffer
        } else if destination.usage.contains(BufferUsage::STORAGE) {
            ResourceState::StorageRead
        } else {
            ResourceState::CopyDst
        };
        self.transition(
            ResourceHandle::Buffer(upload.destination),
            ResourceState::CopyDst,
            destination_state,
        )?;
        self.commands.push(Command::CopyBuffer {
            source: upload.staging_handle,
            destination: upload.destination,
            source_offset: 0,
            destination_offset: upload.destination_offset,
            size: upload.size,
        });
        if destination_state != ResourceState::CopyDst {
            self.commands.push(Command::Barrier(ResourceBarrier {
                resource: ResourceHandle::Buffer(upload.destination),
                before: ResourceState::CopyDst,
                after: destination_state,
            }));
        }
        self.deferred_buffers.push(upload.staging);
        Ok(())
    }

    fn clear_color(&self, texture: &NativeTexture, clear: ClearValue) -> RhiResult<()> {
        let Some(rtv) = texture.rtv else {
            return Err(RhiError::Backend(String::from(
                "color attachment has no Direct3D 12 RTV",
            )));
        };
        let ClearValue::Color(bits) = clear else {
            return Err(RhiError::InvalidDescription(
                "color attachment requires a color clear value",
            ));
        };
        let color = bits.map(f32::from_bits);
        unsafe { self.command_list.ClearRenderTargetView(rtv, &color, None) };
        Ok(())
    }

    fn clear_depth(&self, texture: &NativeTexture, clear: ClearValue) -> RhiResult<()> {
        let Some(dsv) = texture.dsv else {
            return Err(RhiError::Backend(String::from(
                "depth attachment has no Direct3D 12 DSV",
            )));
        };
        let ClearValue::DepthStencil { depth, stencil } = clear else {
            return Err(RhiError::InvalidDescription(
                "depth attachment requires a depth/stencil clear value",
            ));
        };
        unsafe {
            self.command_list.ClearDepthStencilView(
                dsv,
                D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL,
                f32::from_bits(depth),
                u8::try_from(stencil).unwrap_or(u8::MAX),
                None,
            )
        };
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum ResourceSlot {
    Texture(TextureHandle),
    Buffer(BufferHandle),
}

impl CommandEncoder for DirectX12CommandEncoder {
    fn barrier(&mut self, barrier: ResourceBarrier) -> RhiResult<()> {
        self.require_no_render_pass()?;
        self.transition(barrier.resource, barrier.before, barrier.after)?;
        self.commands.push(Command::Barrier(barrier));
        Ok(())
    }

    fn begin_render_pass(&mut self, desc: RenderPassDesc) -> RhiResult<()> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "a render pass is already open",
            )));
        }
        if desc.colors.is_empty() && desc.depth.is_none() {
            return Err(RhiError::InvalidDescription(
                "render pass must contain an attachment",
            ));
        }
        let mut colors = Vec::with_capacity(desc.colors.len());
        let mut extent = None;
        for attachment in &desc.colors {
            let texture = self.resource_texture(attachment.texture)?;
            let rtv = texture
                .rtv
                .ok_or_else(|| RhiError::Backend(String::from("color attachment has no RTV")))?;
            extent.get_or_insert(texture.desc.extent);
            if extent != Some(texture.desc.extent) {
                return Err(RhiError::InvalidDescription(
                    "render pass attachments must have matching extents",
                ));
            }
            colors.push(rtv);
        }
        let depth = if let Some(attachment) = desc.depth {
            let texture = self.resource_texture(attachment.texture)?;
            let dsv = texture
                .dsv
                .ok_or_else(|| RhiError::Backend(String::from("depth attachment has no DSV")))?;
            extent.get_or_insert(texture.desc.extent);
            if extent != Some(texture.desc.extent) {
                return Err(RhiError::InvalidDescription(
                    "render pass attachments must have matching extents",
                ));
            }
            Some((dsv, attachment))
        } else {
            None
        };
        let extent = extent.ok_or(RhiError::InvalidDescription(
            "render pass must contain an attachment",
        ))?;
        let viewport = [D3D12_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: extent.width as f32,
            Height: extent.height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        }];
        let scissor = [RECT {
            left: 0,
            top: 0,
            right: extent.width as i32,
            bottom: extent.height as i32,
        }];
        unsafe {
            self.command_list.RSSetViewports(&viewport);
            self.command_list.RSSetScissorRects(&scissor);
            self.command_list.OMSetRenderTargets(
                colors.len() as u32,
                (!colors.is_empty()).then_some(colors.as_ptr()),
                false,
                depth.as_ref().map(|(dsv, _)| std::ptr::from_ref(dsv)),
            );
        }
        for attachment in &desc.colors {
            if attachment.load == LoadOp::Clear {
                self.clear_color(self.resource_texture(attachment.texture)?, attachment.clear)?;
            }
        }
        if let Some((_, attachment)) = depth {
            if attachment.load == LoadOp::Clear {
                self.clear_depth(self.resource_texture(attachment.texture)?, attachment.clear)?;
            }
        }
        self.render_pass_open = true;
        self.commands.push(Command::BeginRenderPass(desc));
        Ok(())
    }

    fn end_render_pass(&mut self) -> RhiResult<()> {
        if !self.render_pass_open {
            return Err(RhiError::Message(String::from("no render pass is open")));
        }
        self.render_pass_open = false;
        self.commands.push(Command::EndRenderPass);
        Ok(())
    }

    fn copy_texture(&mut self, source: TextureHandle, destination: TextureHandle) -> RhiResult<()> {
        self.require_no_render_pass()?;
        let source_resource = self.resource_texture(source)?.resource.clone();
        let destination_resource = self.resource_texture(destination)?.resource.clone();
        unsafe {
            self.command_list
                .CopyResource(&destination_resource, &source_resource)
        };
        self.commands.push(Command::CopyTexture {
            source,
            destination,
        });
        Ok(())
    }

    fn copy_buffer(
        &mut self,
        source: BufferHandle,
        destination: BufferHandle,
        source_offset: u64,
        destination_offset: u64,
        size: u64,
    ) -> RhiResult<()> {
        self.require_no_render_pass()?;
        let source_resource = self.resource_buffer(source)?.clone();
        let destination_resource = self.resource_buffer(destination)?.clone();
        let source_end = source_offset
            .checked_add(size)
            .ok_or(RhiError::InvalidDescription("buffer copy range overflow"))?;
        let destination_end = destination_offset
            .checked_add(size)
            .ok_or(RhiError::InvalidDescription("buffer copy range overflow"))?;
        if size == 0
            || source_end > source_resource.size
            || destination_end > destination_resource.size
        {
            return Err(RhiError::InvalidDescription(
                "buffer copy range exceeds a buffer",
            ));
        }

        // Upload heaps begin in GENERIC_READ, while device-local resources
        // begin in COPY_DEST. Make the copy states explicit before issuing the
        // region copy; the staging resource is retired only after the queue is
        // idle by the caller.
        self.transition(
            ResourceHandle::Buffer(source),
            ResourceState::Undefined,
            ResourceState::CopySrc,
        )?;
        self.transition(
            ResourceHandle::Buffer(destination),
            ResourceState::Undefined,
            ResourceState::CopyDst,
        )?;
        unsafe {
            self.command_list.CopyBufferRegion(
                &destination_resource.resource,
                destination_offset,
                &source_resource.resource,
                source_offset,
                size,
            );
        }

        let destination_state = if destination_resource.usage.contains(BufferUsage::INDEX) {
            ResourceState::IndexBuffer
        } else if destination_resource.usage.contains(BufferUsage::VERTEX) {
            ResourceState::VertexBuffer
        } else if destination_resource.usage.contains(BufferUsage::UNIFORM) {
            ResourceState::UniformBuffer
        } else if destination_resource.usage.contains(BufferUsage::STORAGE) {
            ResourceState::StorageRead
        } else {
            ResourceState::CopyDst
        };
        self.transition(
            ResourceHandle::Buffer(destination),
            ResourceState::CopyDst,
            destination_state,
        )?;
        self.commands.push(Command::CopyBuffer {
            source,
            destination,
            source_offset,
            destination_offset,
            size,
        });
        if destination_state != ResourceState::CopyDst {
            self.commands.push(Command::Barrier(ResourceBarrier {
                resource: ResourceHandle::Buffer(destination),
                before: ResourceState::CopyDst,
                after: destination_state,
            }));
        }
        Ok(())
    }

    fn blit_texture(&mut self, source: TextureHandle, destination: TextureHandle) -> RhiResult<()> {
        self.require_no_render_pass()?;
        let source_desc = self.resource_texture(source)?.desc.clone();
        let destination_desc = self.resource_texture(destination)?.desc.clone();
        if source_desc.extent != destination_desc.extent
            || source_desc.format != destination_desc.format
        {
            return Err(RhiError::InvalidDescription(
                "Direct3D 12 fallback blit requires matching extents and formats",
            ));
        }
        let source_resource = self.resource_texture(source)?.resource.clone();
        let destination_resource = self.resource_texture(destination)?.resource.clone();
        unsafe {
            self.command_list
                .CopyResource(&destination_resource, &source_resource)
        };
        self.commands.push(Command::BlitTexture {
            source,
            destination,
        });
        Ok(())
    }

    fn bind_pipeline(&mut self, pipeline_handle: PipelineHandle) -> RhiResult<()> {
        let pipeline = self
            .pipelines
            .get(&pipeline_handle)
            .ok_or(RhiError::InvalidHandle)?;
        unsafe {
            self.command_list.SetPipelineState(&pipeline.pipeline_state);
            if pipeline.desc.compute_shader.is_some() {
                self.command_list
                    .SetComputeRootSignature(&pipeline.root_signature);
            } else {
                self.command_list
                    .SetGraphicsRootSignature(&pipeline.root_signature);
                self.command_list
                    .IASetPrimitiveTopology(pipeline::primitive_topology(
                        pipeline.desc.primitive_topology,
                    ));
            }
        }
        self.pipeline_bound = Some(pipeline_handle);
        self.commands.push(Command::BindPipeline(pipeline_handle));
        Ok(())
    }

    fn push_constants(
        &mut self,
        pipeline_handle: PipelineHandle,
        offset: u32,
        data: &[u8],
    ) -> RhiResult<()> {
        if !self.pipelines.contains_key(&pipeline_handle) {
            return Err(RhiError::InvalidHandle);
        }
        if self.pipeline_bound != Some(pipeline_handle) {
            return Err(RhiError::Message(String::from(
                "push constants require the matching pipeline to be bound",
            )));
        }
        if data.is_empty() || !offset.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(RhiError::InvalidDescription(
                "push constants must be non-empty and four-byte aligned",
            ));
        }
        let byte_len = u32::try_from(data.len())
            .map_err(|_| RhiError::InvalidDescription("push constants are too large"))?;
        let end = offset
            .checked_add(byte_len)
            .ok_or(RhiError::InvalidDescription("push constants are too large"))?;
        if end > 16 * 4 {
            return Err(RhiError::InvalidDescription(
                "Direct3D 12 push constants are limited to 64 bytes",
            ));
        }
        let pipeline = self.bound_native_pipeline()?;
        let words = u32::try_from(data.len() / 4)
            .map_err(|_| RhiError::InvalidDescription("push constants are too large"))?;
        unsafe {
            if pipeline.desc.compute_shader.is_some() {
                self.command_list.SetComputeRoot32BitConstants(
                    2,
                    words,
                    data.as_ptr().cast(),
                    offset / 4,
                );
            } else {
                self.command_list.SetGraphicsRoot32BitConstants(
                    2,
                    words,
                    data.as_ptr().cast(),
                    offset / 4,
                );
            }
        }
        self.commands.push(Command::PushConstants {
            pipeline: pipeline_handle,
            offset,
            data: data.to_vec(),
        });
        Ok(())
    }

    fn bind_group(
        &mut self,
        set: u32,
        bind_group: BindGroupHandle,
        dynamic_offsets: &[u32],
    ) -> RhiResult<()> {
        if set != 0 {
            return Err(RhiError::InvalidDescription(
                "Direct3D 12 backend currently exposes descriptor set zero",
            ));
        }
        let group = self
            .bind_groups
            .get(&bind_group)
            .ok_or(RhiError::InvalidHandle)?;
        if self.pipeline_bound != Some(group.desc.pipeline) {
            return Err(RhiError::Message(String::from(
                "bind group does not belong to the bound pipeline",
            )));
        }
        if !dynamic_offsets.is_empty() {
            return Err(RhiError::InvalidDescription(
                "Direct3D 12 backend does not support dynamic bind-group offsets",
            ));
        }
        self.set_descriptor_heaps();
        let resource = self
            .resource_heap
            .gpu(group.resource_offset)
            .ok_or_else(|| {
                RhiError::Backend(String::from("missing resource GPU descriptor handle"))
            })?;
        let sampler = self.sampler_heap.gpu(group.sampler_offset).ok_or_else(|| {
            RhiError::Backend(String::from("missing sampler GPU descriptor handle"))
        })?;
        let pipeline = self.bound_native_pipeline()?;
        unsafe {
            if pipeline.desc.compute_shader.is_some() {
                self.command_list.SetComputeRootDescriptorTable(0, resource);
                self.command_list.SetComputeRootDescriptorTable(1, sampler);
            } else {
                self.command_list
                    .SetGraphicsRootDescriptorTable(0, resource);
                self.command_list.SetGraphicsRootDescriptorTable(1, sampler);
            }
        }
        self.commands.push(Command::BindGroup {
            set,
            bind_group,
            dynamic_offsets: dynamic_offsets.to_vec(),
        });
        Ok(())
    }

    fn bind_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: BufferHandle,
        offset: u64,
    ) -> RhiResult<()> {
        self.require_render_pass()?;
        let resource = self.resource_buffer(buffer)?;
        if offset > resource.size {
            return Err(RhiError::InvalidDescription(
                "vertex-buffer offset exceeds the buffer",
            ));
        }
        let pipeline = self.bound_native_pipeline()?;
        let layout = pipeline
            .desc
            .vertex_buffers
            .iter()
            .find(|layout| layout.binding == slot)
            .ok_or(RhiError::InvalidDescription(
                "vertex buffer slot is not in the pipeline",
            ))?;
        let view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: unsafe { resource.resource.GetGPUVirtualAddress() } + offset,
            SizeInBytes: u32::try_from(resource.size - offset)
                .map_err(|_| RhiError::InvalidDescription("vertex buffer is too large"))?,
            StrideInBytes: layout.stride,
        };
        unsafe {
            self.command_list
                .IASetVertexBuffers(slot, Some(std::slice::from_ref(&view)))
        };
        self.commands.push(Command::BindVertexBuffer {
            slot,
            buffer,
            offset,
        });
        Ok(())
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64) -> RhiResult<()> {
        self.require_render_pass()?;
        let resource = self.resource_buffer(buffer)?;
        if offset > resource.size {
            return Err(RhiError::InvalidDescription(
                "index-buffer offset exceeds the buffer",
            ));
        }
        let view = D3D12_INDEX_BUFFER_VIEW {
            BufferLocation: unsafe { resource.resource.GetGPUVirtualAddress() } + offset,
            SizeInBytes: u32::try_from(resource.size - offset)
                .map_err(|_| RhiError::InvalidDescription("index buffer is too large"))?,
            Format: DXGI_FORMAT_R32_UINT,
        };
        unsafe {
            self.command_list
                .IASetIndexBuffer(Some(std::ptr::from_ref(&view)))
        };
        self.commands
            .push(Command::BindIndexBuffer { buffer, offset });
        Ok(())
    }

    fn draw(&mut self, vertex_count: u32, first_vertex: u32) -> RhiResult<()> {
        self.require_render_pass()?;
        if self.pipeline_bound.is_none() {
            return Err(RhiError::Message(String::from(
                "draw requires a bound pipeline",
            )));
        }
        unsafe {
            self.command_list
                .DrawInstanced(vertex_count, 1, first_vertex, 0)
        };
        self.commands.push(Command::Draw {
            vertex_count,
            first_vertex,
        });
        Ok(())
    }

    fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) -> RhiResult<()> {
        self.require_render_pass()?;
        if self.pipeline_bound.is_none() {
            return Err(RhiError::Message(String::from(
                "indexed draw requires a bound pipeline",
            )));
        }
        unsafe {
            self.command_list.DrawIndexedInstanced(
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            )
        };
        self.commands.push(Command::DrawIndexed {
            index_count,
            instance_count,
            first_index,
            vertex_offset,
            first_instance,
        });
        Ok(())
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> RhiResult<()> {
        self.require_no_render_pass()?;
        if self.pipeline_bound.is_none() {
            return Err(RhiError::Message(String::from(
                "dispatch requires a bound pipeline",
            )));
        }
        unsafe { self.command_list.Dispatch(x, y, z) };
        self.commands.push(Command::Dispatch { x, y, z });
        Ok(())
    }

    fn finish(self: Box<Self>) -> RhiResult<CommandBuffer> {
        if self.render_pass_open {
            return Err(RhiError::Message(String::from(
                "cannot finish while a render pass is open",
            )));
        }
        unsafe {
            self.command_list
                .Close()
                .map_err(|error| backend_error("close Direct3D 12 command list", error))?;
        }
        let submission = Box::new(context::NativeSubmission {
            _allocator: self.allocator,
            command_list: self.command_list,
            _deferred_buffers: self.deferred_buffers,
        });
        let token = Box::into_raw(submission) as usize as u64;
        Ok(CommandBuffer::with_native(
            self.label,
            self.commands,
            Some(token),
        ))
    }
}
