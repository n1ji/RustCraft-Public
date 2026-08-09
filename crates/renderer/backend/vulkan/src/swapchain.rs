use ash::vk;

use renderer::rhi::device::CommandQueue;
use renderer::rhi::types::{
    RhiError, RhiResult, SurfaceConfig, SurfaceFrame, TextureDesc, TextureFormat, TextureHandle,
    TextureUsage,
};

use super::context::{NativeState, NativeTexture};
use super::VulkanBackend;

fn requested_vk_format(format: TextureFormat) -> Option<vk::Format> {
    match format {
        TextureFormat::R8G8B8A8Unorm | TextureFormat::Rgba8Unorm => {
            Some(vk::Format::R8G8B8A8_UNORM)
        }
        TextureFormat::R8G8B8A8Srgb | TextureFormat::Rgba8Srgb => Some(vk::Format::R8G8B8A8_SRGB),
        TextureFormat::B8G8R8A8Srgb => Some(vk::Format::B8G8R8A8_SRGB),
        _ => None,
    }
}

fn rhi_format(format: vk::Format) -> Option<TextureFormat> {
    match format {
        vk::Format::R8G8B8A8_UNORM => Some(TextureFormat::R8G8B8A8Unorm),
        vk::Format::R8G8B8A8_SRGB => Some(TextureFormat::R8G8B8A8Srgb),
        vk::Format::B8G8R8A8_SRGB => Some(TextureFormat::B8G8R8A8Srgb),
        _ => None,
    }
}

fn choose_surface_format(
    formats: &[vk::SurfaceFormatKHR],
    requested: TextureFormat,
) -> RhiResult<vk::SurfaceFormatKHR> {
    let requested = requested_vk_format(requested);
    if formats.len() == 1 && formats[0].format == vk::Format::UNDEFINED {
        return Ok(vk::SurfaceFormatKHR {
            format: requested.unwrap_or(vk::Format::B8G8R8A8_SRGB),
            color_space: formats[0].color_space,
        });
    }
    formats
        .iter()
        .find(|format| requested == Some(format.format))
        .copied()
        .or_else(|| {
            formats
                .iter()
                .find(|format| format.format == vk::Format::B8G8R8A8_SRGB)
                .copied()
        })
        .ok_or_else(|| {
            RhiError::Backend(String::from(
                "Vulkan surface does not expose the requested color format",
            ))
        })
}

fn choose_present_mode(modes: &[vk::PresentModeKHR], vsync: bool) -> RhiResult<vk::PresentModeKHR> {
    let preferred: &[vk::PresentModeKHR] = if vsync {
        &[vk::PresentModeKHR::FIFO]
    } else {
        &[
            vk::PresentModeKHR::MAILBOX,
            vk::PresentModeKHR::IMMEDIATE,
            vk::PresentModeKHR::FIFO,
        ]
    };
    preferred
        .iter()
        .copied()
        .find(|mode| modes.contains(mode))
        .or_else(|| modes.first().copied())
        .ok_or_else(|| RhiError::Backend(String::from("Vulkan surface has no present modes")))
}

fn choose_extent(
    config: &SurfaceConfig,
    capabilities: &vk::SurfaceCapabilitiesKHR,
) -> vk::Extent2D {
    if capabilities.current_extent.width != u32::MAX {
        return capabilities.current_extent;
    }
    vk::Extent2D {
        width: config.extent.width.clamp(
            capabilities.min_image_extent.width,
            capabilities.max_image_extent.width,
        ),
        height: config.extent.height.clamp(
            capabilities.min_image_extent.height,
            capabilities.max_image_extent.height,
        ),
    }
}

fn choose_composite_alpha(capabilities: &vk::SurfaceCapabilitiesKHR) -> vk::CompositeAlphaFlagsKHR {
    [
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::INHERIT,
    ]
    .into_iter()
    .find(|flag| capabilities.supported_composite_alpha.contains(*flag))
    .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE)
}

fn destroy_swapchain_texture(native: &mut NativeState, handle: TextureHandle) {
    native.remove_texture_table(handle);
    if let Some(resource) = native.textures.remove(&handle) {
        if resource.owns_view {
            unsafe { native.device.destroy_image_view(resource.view, None) };
        }
        if let Some(allocation) = resource.allocation {
            unsafe { native.device.destroy_image(resource.image, None) };
            let _ = native.allocator_mut().free(allocation);
        }
    }
}

impl VulkanBackend {
    pub(crate) fn configure_surface(&mut self, config: SurfaceConfig) -> RhiResult<()> {
        if config.extent.is_empty() {
            return Err(RhiError::InvalidDescription(
                "surface extent must be non-zero",
            ));
        }
        let already_configured = self.native.as_ref().is_some_and(|native| {
            native.swapchain.is_some() && native.swapchain_config.as_ref() == Some(&config)
        });
        if already_configured {
            return Ok(());
        }

        {
            let Some(native) = self.native.as_ref() else {
                return Err(RhiError::Backend(String::from(
                    "cannot configure a surface without an attached Vulkan context",
                )));
            };
            if native.surface.is_none()
                || native.surface_loader.is_none()
                || native.instance.is_none()
            {
                return Err(RhiError::Backend(String::from(
                    "the attached Vulkan context has no window surface",
                )));
            }
        }

        self.destroy_swapchain_resources()?;

        let (surface, capabilities, surface_format, present_mode, extent, swapchain_loader, device) = {
            let native = self
                .native
                .as_ref()
                .ok_or_else(|| RhiError::Backend(String::from("Vulkan context is unavailable")))?;
            let surface = native
                .surface
                .ok_or_else(|| RhiError::Backend(String::from("Vulkan surface is unavailable")))?;
            let surface_loader = native.surface_loader.as_ref().ok_or_else(|| {
                RhiError::Backend(String::from("Vulkan surface loader is unavailable"))
            })?;
            let instance = native
                .instance
                .as_ref()
                .ok_or_else(|| RhiError::Backend(String::from("Vulkan instance is unavailable")))?;
            let capabilities = unsafe {
                surface_loader
                    .get_physical_device_surface_capabilities(native.physical_device, surface)
            }
            .map_err(|error| {
                RhiError::Backend(format!("query Vulkan surface capabilities: {error:?}"))
            })?;
            let formats = unsafe {
                surface_loader.get_physical_device_surface_formats(native.physical_device, surface)
            }
            .map_err(|error| {
                RhiError::Backend(format!("query Vulkan surface formats: {error:?}"))
            })?;
            let present_modes = unsafe {
                surface_loader
                    .get_physical_device_surface_present_modes(native.physical_device, surface)
            }
            .map_err(|error| RhiError::Backend(format!("query Vulkan present modes: {error:?}")))?;
            let surface_format = choose_surface_format(&formats, config.format)?;
            let present_mode = choose_present_mode(&present_modes, config.vsync)?;
            let extent = choose_extent(&config, &capabilities);
            let swapchain_loader = ash::khr::swapchain::Device::new(instance, &native.device);
            (
                surface,
                capabilities,
                surface_format,
                present_mode,
                extent,
                swapchain_loader,
                native.device.clone(),
            )
        };
        let actual_format = rhi_format(surface_format.format).ok_or_else(|| {
            RhiError::Backend(format!(
                "Vulkan surface format {:?} is not representable by the RHI",
                surface_format.format
            ))
        })?;

        if !capabilities
            .supported_usage_flags
            .contains(vk::ImageUsageFlags::TRANSFER_DST)
        {
            return Err(RhiError::Backend(String::from(
                "Vulkan surface does not support transfer-destination images",
            )));
        }
        let mut image_usage =
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST;
        if capabilities
            .supported_usage_flags
            .contains(vk::ImageUsageFlags::TRANSFER_SRC)
        {
            image_usage |= vk::ImageUsageFlags::TRANSFER_SRC;
        }
        let min_image_count = capabilities.min_image_count.saturating_add(1).max(1);
        let image_count = if capabilities.max_image_count == 0 {
            min_image_count
        } else {
            min_image_count.min(capabilities.max_image_count)
        };
        let create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(image_usage)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(capabilities.current_transform)
            .composite_alpha(choose_composite_alpha(&capabilities))
            .present_mode(present_mode)
            .clipped(true);
        let swapchain = unsafe { swapchain_loader.create_swapchain(&create_info, None) }
            .map_err(|error| RhiError::Backend(format!("create Vulkan swapchain: {error:?}")))?;
        let images = match unsafe { swapchain_loader.get_swapchain_images(swapchain) } {
            Ok(images) => images,
            Err(error) => {
                unsafe { swapchain_loader.destroy_swapchain(swapchain, None) };
                return Err(RhiError::Backend(format!(
                    "get Vulkan swapchain images: {error:?}"
                )));
            }
        };

        let mut render_finished = Vec::with_capacity(images.len());
        for _ in &images {
            match unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) } {
                Ok(semaphore) => render_finished.push(semaphore),
                Err(error) => {
                    unsafe {
                        for semaphore in render_finished.drain(..) {
                            device.destroy_semaphore(semaphore, None);
                        }
                        swapchain_loader.destroy_swapchain(swapchain, None);
                    }
                    return Err(RhiError::Backend(format!(
                        "create Vulkan render-finished semaphore: {error:?}"
                    )));
                }
            }
        }

        let subresource_range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        let mut views = Vec::with_capacity(images.len());
        for image in &images {
            let view = unsafe {
                device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(*image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(surface_format.format)
                        .subresource_range(subresource_range),
                    None,
                )
            };
            match view {
                Ok(view) => views.push(view),
                Err(error) => {
                    unsafe {
                        for view in views.drain(..) {
                            device.destroy_image_view(view, None);
                        }
                        for semaphore in render_finished.drain(..) {
                            device.destroy_semaphore(semaphore, None);
                        }
                        swapchain_loader.destroy_swapchain(swapchain, None);
                    }
                    return Err(RhiError::Backend(format!(
                        "create Vulkan swapchain image view: {error:?}"
                    )));
                }
            }
        }

        let mut texture_usage = TextureUsage::COLOR_ATTACHMENT | TextureUsage::PRESENT;
        if image_usage.contains(vk::ImageUsageFlags::TRANSFER_DST) {
            texture_usage |= TextureUsage::TRANSFER_DST;
        }
        if image_usage.contains(vk::ImageUsageFlags::TRANSFER_SRC) {
            texture_usage |= TextureUsage::TRANSFER_SRC;
        }
        let mut desc = TextureDesc::new(
            renderer::rhi::types::Extent2D::new(extent.width, extent.height),
            actual_format,
            texture_usage,
        );
        desc.label = String::from("vulkan.swapchain-image");
        let mut handles = Vec::with_capacity(images.len());
        for _ in &images {
            match self.resources.create_external_texture(desc.clone()) {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    for handle in handles {
                        let _ = self.resources.destroy_texture(handle);
                    }
                    unsafe {
                        for view in views {
                            device.destroy_image_view(view, None);
                        }
                        for semaphore in render_finished.drain(..) {
                            device.destroy_semaphore(semaphore, None);
                        }
                        swapchain_loader.destroy_swapchain(swapchain, None);
                    }
                    return Err(error);
                }
            }
        }

        let Some(native) = self.native.as_mut() else {
            for handle in handles {
                let _ = self.resources.destroy_texture(handle);
            }
            unsafe {
                for view in views {
                    device.destroy_image_view(view, None);
                }
                for semaphore in render_finished.drain(..) {
                    device.destroy_semaphore(semaphore, None);
                }
                swapchain_loader.destroy_swapchain(swapchain, None);
            }
            return Err(RhiError::Backend(String::from(
                "Vulkan context disappeared while configuring the surface",
            )));
        };
        for ((handle, image), view) in handles.iter().copied().zip(images).zip(views) {
            native.textures.insert(
                handle,
                NativeTexture {
                    image,
                    view,
                    allocation: None,
                    owns_view: true,
                    // A newly-created swapchain image is in UNDEFINED until
                    // the first command buffer transitions it.  PRESENT is
                    // only true after the first successful frame submission.
                    layout: vk::ImageLayout::UNDEFINED,
                },
            );
            let desc = self
                .resources
                .texture(handle)
                .cloned()
                .ok_or(RhiError::InvalidHandle)?;
            native.add_texture_table(handle, image, view, vk::ImageLayout::UNDEFINED, desc);
        }
        native.swapchain_loader = Some(swapchain_loader);
        native.swapchain = Some(swapchain);
        native.swapchain_format = Some(actual_format);
        native.swapchain_extent = extent;
        native.swapchain_config = Some(config);
        native.swapchain_textures = handles;
        native.swapchain_render_finished = render_finished;
        native.acquired_image = None;
        native.acquired_frame_slot = None;
        native.acquired_image_submitted = false;
        for slot in &mut native.frame_slots {
            slot.in_flight = false;
        }
        Ok(())
    }

    pub(crate) fn acquire_surface_frame(&mut self) -> RhiResult<SurfaceFrame> {
        self.collect_completed()?;
        let (slot_index, device, fence, in_flight) = {
            let Some(native) = self.native.as_mut() else {
                return Err(RhiError::Backend(String::from(
                    "cannot acquire a surface frame without an attached Vulkan context",
                )));
            };
            if native.acquired_image.is_some() {
                return Err(RhiError::Message(String::from(
                    "a Vulkan swapchain image is already acquired",
                )));
            }
            if native.frame_slots.is_empty() {
                return Err(RhiError::Backend(String::from(
                    "Vulkan context has no reusable frame slots",
                )));
            }
            let slot_index = native.next_frame_slot % native.frame_slots.len();
            native.next_frame_slot = native.next_frame_slot.wrapping_add(1);
            let slot = native.frame_slots.get(slot_index).ok_or_else(|| {
                RhiError::Backend(String::from("Vulkan frame slot index is invalid"))
            })?;
            (
                slot_index,
                native.device.clone(),
                slot.fence,
                slot.in_flight,
            )
        };
        if in_flight {
            unsafe {
                device
                    .wait_for_fences(&[fence], true, u64::MAX)
                    .map_err(|error| {
                        RhiError::Backend(format!("wait Vulkan frame fence: {error:?}"))
                    })?;
            }
            self.collect_completed()?;
        }

        let Some(native) = self.native.as_mut() else {
            return Err(RhiError::Backend(String::from(
                "cannot acquire a surface frame without an attached Vulkan context",
            )));
        };
        let Some(loader) = native.swapchain_loader.as_ref() else {
            return Err(RhiError::Backend(String::from(
                "Vulkan surface has not been configured",
            )));
        };
        let swapchain = native.swapchain.ok_or_else(|| {
            RhiError::Backend(String::from("Vulkan surface has not been configured"))
        })?;
        let image_available = native
            .frame_slots
            .get(slot_index)
            .ok_or_else(|| RhiError::Backend(String::from("Vulkan frame slot index is invalid")))?
            .image_available;
        let (image_index, _suboptimal) = match unsafe {
            loader.acquire_next_image(swapchain, u64::MAX, image_available, vk::Fence::null())
        } {
            Ok(acquired) => acquired,
            Err(error) => {
                return Err(RhiError::Backend(format!(
                    "acquire Vulkan swapchain image: {error:?}"
                )));
            }
        };
        let texture = native
            .swapchain_textures
            .get(image_index as usize)
            .copied()
            .ok_or_else(|| {
                native.acquired_frame_slot = None;
                RhiError::Backend(String::from("Vulkan returned an invalid image index"))
            })?;
        native.acquired_image = Some(image_index);
        native.acquired_frame_slot = Some(slot_index);
        native.acquired_image_submitted = false;
        Ok(SurfaceFrame {
            texture,
            image_index,
        })
    }

    pub(crate) fn present(&mut self, frame: SurfaceFrame) -> RhiResult<()> {
        let valid_frame = self.native.as_ref().is_some_and(|native| {
            native.acquired_image == Some(frame.image_index)
                && native
                    .swapchain_textures
                    .get(frame.image_index as usize)
                    .copied()
                    == Some(frame.texture)
        });
        if !valid_frame {
            return Err(RhiError::InvalidHandle);
        }
        let submitted = self
            .native
            .as_ref()
            .is_some_and(|native| native.acquired_image_submitted);
        if !submitted {
            self.wait_idle()?;
        }
        let Some(native) = self.native.as_mut() else {
            return Err(RhiError::Backend(String::from(
                "cannot present without an attached Vulkan context",
            )));
        };
        let loader = native.swapchain_loader.as_ref().ok_or_else(|| {
            RhiError::Backend(String::from("Vulkan surface has not been configured"))
        })?;
        let swapchain = native.swapchain.ok_or_else(|| {
            RhiError::Backend(String::from("Vulkan surface has not been configured"))
        })?;
        let wait_semaphore = if submitted {
            native
                .swapchain_render_finished
                .get(frame.image_index as usize)
                .copied()
                .ok_or_else(|| {
                    RhiError::Backend(String::from(
                        "Vulkan swapchain has no render-finished semaphore for the image",
                    ))
                })?
        } else {
            if let Some(frame_slot) = native.acquired_frame_slot {
                let slot = native.frame_slots.get_mut(frame_slot).ok_or_else(|| {
                    RhiError::Backend(String::from("Vulkan frame slot index is invalid"))
                })?;
                let replacement = unsafe {
                    native
                        .device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                }
                .map_err(|error| {
                    RhiError::Backend(format!(
                        "replace unused Vulkan acquire semaphore: {error:?}"
                    ))
                })?;
                unsafe { native.device.destroy_semaphore(slot.image_available, None) };
                slot.image_available = replacement;
            }
            vk::Semaphore::null()
        };
        let wait_semaphores = if submitted {
            std::slice::from_ref(&wait_semaphore)
        } else {
            &[]
        };
        let result = unsafe {
            loader.queue_present(
                native.queue,
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(wait_semaphores)
                    .swapchains(std::slice::from_ref(&swapchain))
                    .image_indices(std::slice::from_ref(&frame.image_index)),
            )
        }
        .map(|_| ())
        .map_err(|error| RhiError::Backend(format!("present Vulkan swapchain image: {error:?}")));
        if result.is_ok() {
            native.acquired_image = None;
            native.acquired_frame_slot = None;
            native.acquired_image_submitted = false;
        }
        result
    }

    fn destroy_swapchain_resources(&mut self) -> RhiResult<()> {
        let old_handles = self
            .native
            .as_ref()
            .map(|native| native.swapchain_textures.clone())
            .unwrap_or_default();
        for handle in &old_handles {
            self.resources.validate_texture_destruction(*handle)?;
        }
        self.wait_idle()?;
        let (loader, swapchain, render_finished) = {
            let Some(native) = self.native.as_mut() else {
                return Ok(());
            };
            native.acquired_image = None;
            native.acquired_frame_slot = None;
            native.acquired_image_submitted = false;
            for slot in &mut native.frame_slots {
                slot.in_flight = false;
            }
            native.swapchain_textures.clear();
            native.swapchain_config = None;
            native.swapchain_format = None;
            native.swapchain_extent = vk::Extent2D::default();
            (
                native.swapchain_loader.take(),
                native.swapchain.take(),
                std::mem::take(&mut native.swapchain_render_finished),
            )
        };
        for handle in old_handles {
            let _ = self.resources.destroy_texture(handle)?;
            if let Some(native) = self.native.as_mut() {
                destroy_swapchain_texture(native, handle);
            }
        }
        if let (Some(loader), Some(swapchain)) = (loader, swapchain) {
            unsafe { loader.destroy_swapchain(swapchain, None) };
        }
        if let Some(native) = self.native.as_mut() {
            unsafe {
                for semaphore in render_finished {
                    native.device.destroy_semaphore(semaphore, None);
                }
            }
        }
        Ok(())
    }
}
