use ash::vk;

use renderer::core::ResourceManager;
use renderer::rhi::types::*;

use super::context::{NativeBindGroup, NativeState};
use super::pipeline::{descriptor_type, merge_shader_interfaces};

pub(crate) fn create_native_bind_group(
    native: &mut NativeState,
    resources: &ResourceManager,
    desc: &BindGroupDesc,
) -> RhiResult<NativeBindGroup> {
    let Some(pipeline) = native.pipelines.get(&desc.pipeline).cloned() else {
        return Err(RhiError::InvalidHandle);
    };
    let Some(&set_layout) = pipeline.set_layouts.get(desc.set as usize) else {
        return Err(RhiError::InvalidDescription(
            "bind group set is not declared by the pipeline",
        ));
    };
    let Some(pipeline_desc) = resources.pipeline(desc.pipeline) else {
        return Err(RhiError::InvalidHandle);
    };
    let interface = merge_shader_interfaces(&native.shaders, pipeline_desc)?;
    let layouts = [set_layout];
    let descriptor_set = unsafe {
        native
            .device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(native.descriptor_pool)
                    .set_layouts(&layouts),
            )
            .map_err(|error| {
                RhiError::Backend(format!("allocate Vulkan descriptor set: {error:?}"))
            })?
    }
    .into_iter()
    .next()
    .ok_or_else(|| RhiError::Backend(String::from("Vulkan returned no descriptor set")))?;

    enum PendingWrite {
        Buffer {
            binding: u32,
            descriptor_type: vk::DescriptorType,
            index: usize,
        },
        Image {
            binding: u32,
            descriptor_type: vk::DescriptorType,
            index: usize,
        },
    }
    let mut buffer_infos = Vec::with_capacity(desc.entries.len());
    let mut image_infos = Vec::with_capacity(desc.entries.len());
    let mut pending_writes = Vec::with_capacity(desc.entries.len());
    for entry in &desc.entries {
        let Some(binding) = interface.binding(desc.set, entry.binding) else {
            return Err(RhiError::InvalidDescription(
                "bind group binding is not declared by the pipeline",
            ));
        };
        match entry.resource {
            BindingResource::Buffer {
                buffer,
                offset,
                size,
            } => {
                let Some(native_buffer) =
                    native.buffers.get(&buffer).map(|resource| resource.buffer)
                else {
                    return Err(RhiError::InvalidHandle);
                };
                buffer_infos.push(vk::DescriptorBufferInfo {
                    buffer: native_buffer,
                    offset,
                    range: if size == 0 { vk::WHOLE_SIZE } else { size },
                });
                pending_writes.push(PendingWrite::Buffer {
                    binding: entry.binding,
                    descriptor_type: descriptor_type(binding.ty),
                    index: buffer_infos.len() - 1,
                });
            }
            BindingResource::Texture { texture } => {
                let Some(view) = native.textures.get(&texture).map(|resource| resource.view) else {
                    return Err(RhiError::InvalidHandle);
                };
                image_infos.push(
                    vk::DescriptorImageInfo::default()
                        .image_view(view)
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
                );
                pending_writes.push(PendingWrite::Image {
                    binding: entry.binding,
                    descriptor_type: descriptor_type(binding.ty),
                    index: image_infos.len() - 1,
                });
            }
            BindingResource::StorageTexture { texture } => {
                let Some(view) = native.textures.get(&texture).map(|resource| resource.view) else {
                    return Err(RhiError::InvalidHandle);
                };
                image_infos.push(
                    vk::DescriptorImageInfo::default()
                        .image_view(view)
                        .image_layout(vk::ImageLayout::GENERAL),
                );
                pending_writes.push(PendingWrite::Image {
                    binding: entry.binding,
                    descriptor_type: descriptor_type(binding.ty),
                    index: image_infos.len() - 1,
                });
            }
            BindingResource::Sampler { sampler } => {
                let Some(native_sampler) = native
                    .samplers
                    .get(&sampler)
                    .map(|resource| resource.sampler)
                else {
                    return Err(RhiError::InvalidHandle);
                };
                image_infos.push(vk::DescriptorImageInfo::default().sampler(native_sampler));
                pending_writes.push(PendingWrite::Image {
                    binding: entry.binding,
                    descriptor_type: descriptor_type(binding.ty),
                    index: image_infos.len() - 1,
                });
            }
            BindingResource::CombinedImageSampler { texture, sampler } => {
                let Some(view) = native.textures.get(&texture).map(|resource| resource.view) else {
                    return Err(RhiError::InvalidHandle);
                };
                let Some(native_sampler) = native
                    .samplers
                    .get(&sampler)
                    .map(|resource| resource.sampler)
                else {
                    return Err(RhiError::InvalidHandle);
                };
                image_infos.push(
                    vk::DescriptorImageInfo::default()
                        .sampler(native_sampler)
                        .image_view(view)
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
                );
                pending_writes.push(PendingWrite::Image {
                    binding: entry.binding,
                    descriptor_type: descriptor_type(binding.ty),
                    index: image_infos.len() - 1,
                });
            }
        }
    }
    let writes = pending_writes
        .into_iter()
        .map(|write| match write {
            PendingWrite::Buffer {
                binding,
                descriptor_type,
                index,
            } => vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(binding)
                .descriptor_type(descriptor_type)
                .buffer_info(std::slice::from_ref(&buffer_infos[index])),
            PendingWrite::Image {
                binding,
                descriptor_type,
                index,
            } => vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(binding)
                .descriptor_type(descriptor_type)
                .image_info(std::slice::from_ref(&image_infos[index])),
        })
        .collect::<Vec<_>>();
    unsafe { native.device.update_descriptor_sets(&writes, &[]) };
    Ok(NativeBindGroup {
        descriptor_set,
        pipeline: desc.pipeline,
        set: desc.set,
    })
}

pub(crate) fn buffer_usage(usage: BufferUsage) -> vk::BufferUsageFlags {
    let mut flags = vk::BufferUsageFlags::empty();
    if usage.contains(BufferUsage::VERTEX) {
        flags |= vk::BufferUsageFlags::VERTEX_BUFFER;
    }
    if usage.contains(BufferUsage::INDEX) {
        flags |= vk::BufferUsageFlags::INDEX_BUFFER;
    }
    if usage.contains(BufferUsage::UNIFORM) {
        flags |= vk::BufferUsageFlags::UNIFORM_BUFFER;
    }
    if usage.contains(BufferUsage::STORAGE) {
        flags |= vk::BufferUsageFlags::STORAGE_BUFFER;
    }
    if usage.contains(BufferUsage::INDIRECT) {
        flags |= vk::BufferUsageFlags::INDIRECT_BUFFER;
    }
    if usage.contains(BufferUsage::TRANSFER_SRC) {
        flags |= vk::BufferUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(BufferUsage::TRANSFER_DST) {
        flags |= vk::BufferUsageFlags::TRANSFER_DST;
    }
    flags
}

pub(crate) fn texture_format(format: TextureFormat) -> vk::Format {
    match format {
        TextureFormat::R8Unorm => vk::Format::R8_UNORM,
        TextureFormat::R8G8B8A8Unorm | TextureFormat::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
        TextureFormat::R8G8B8A8Srgb | TextureFormat::Rgba8Srgb => vk::Format::R8G8B8A8_SRGB,
        TextureFormat::B8G8R8A8Srgb => vk::Format::B8G8R8A8_SRGB,
        TextureFormat::R16G16Float => vk::Format::R16G16_SFLOAT,
        TextureFormat::R16Float => vk::Format::R16_SFLOAT,
        TextureFormat::R16G16B16A16Float | TextureFormat::Rgba16Float => {
            vk::Format::R16G16B16A16_SFLOAT
        }
        TextureFormat::D24UnormS8Uint => vk::Format::D24_UNORM_S8_UINT,
        TextureFormat::D32Float => vk::Format::D32_SFLOAT,
    }
}

pub(crate) fn texture_usage(usage: TextureUsage) -> vk::ImageUsageFlags {
    let mut flags = vk::ImageUsageFlags::empty();
    if usage.contains(TextureUsage::SAMPLED) {
        flags |= vk::ImageUsageFlags::SAMPLED;
    }
    if usage.contains(TextureUsage::STORAGE) {
        flags |= vk::ImageUsageFlags::STORAGE;
    }
    if usage.contains(TextureUsage::COLOR_ATTACHMENT) {
        flags |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
    }
    if usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
        flags |= vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
    }
    if usage.contains(TextureUsage::TRANSFER_SRC) {
        flags |= vk::ImageUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(TextureUsage::TRANSFER_DST) {
        flags |= vk::ImageUsageFlags::TRANSFER_DST;
    }
    flags
}

pub(crate) const fn texture_bytes_per_pixel(format: TextureFormat) -> u32 {
    match format {
        TextureFormat::R8Unorm => 1,
        TextureFormat::R8G8B8A8Unorm
        | TextureFormat::Rgba8Unorm
        | TextureFormat::R8G8B8A8Srgb
        | TextureFormat::Rgba8Srgb
        | TextureFormat::B8G8R8A8Srgb
        | TextureFormat::R16G16Float
        | TextureFormat::D24UnormS8Uint
        | TextureFormat::D32Float => 4,
        TextureFormat::R16Float => 2,
        TextureFormat::R16G16B16A16Float | TextureFormat::Rgba16Float => 8,
    }
}

pub(crate) fn texture_aspect(format: TextureFormat, usage: TextureUsage) -> vk::ImageAspectFlags {
    if matches!(format, TextureFormat::D24UnormS8Uint) {
        vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
    } else if usage.contains(TextureUsage::DEPTH_ATTACHMENT) {
        vk::ImageAspectFlags::DEPTH
    } else {
        vk::ImageAspectFlags::COLOR
    }
}

pub(crate) fn texture_subresource(desc: &TextureDesc) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: texture_aspect(desc.format, desc.usage),
        base_mip_level: 0,
        level_count: desc.mip_levels,
        base_array_layer: 0,
        layer_count: desc.array_layers,
    }
}
