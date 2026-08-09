use crate::rhi::command::{ClearValue, LoadOp, RenderAttachment, RenderPassDesc, StoreOp};
use crate::rhi::device::RenderDevice;
use crate::rhi::types::{
    BindGroupDesc, BindingResource, ResourceBarrier, ResourceHandle, ResourceState, RhiError,
    RhiResult, TextureFormat,
};

use super::super::queue::RenderPassKind;
use super::super::render_graph::AccessType;
use super::super::render_graph::CompiledPass;
use super::RendererRuntime;

impl<D: RenderDevice> RendererRuntime<D> {
    pub fn end_frame(&mut self) -> RhiResult<()> {
        let Some(plan) = self.last_plan.take() else {
            return Ok(());
        };
        let result = (|| {
            self.device.collect_completed()?;
            // Command encoders are allowed to snapshot the backend's native
            // resources when recording begins. Materialize frame-local bind
            // groups before that snapshot so every backend observes them.
            self.ensure_tone_mapping_bind_group(&plan)?;
            let queued_packets = self.queue.take_sorted_packets();
            let result = self.record_frame(&plan, &queued_packets);
            self.queue.restore_packets(queued_packets);
            result
        })();
        // The plan is still the current frame's plan after recording. Keep it
        // available to callers that inspect the runtime between frames, while
        // avoiding a deep clone of the compiled graph on every redraw.
        self.last_plan = Some(plan);
        result?;
        self.frame_index = self.frame_index.wrapping_add(1);
        Ok(())
    }

    fn record_frame(
        &mut self,
        plan: &super::types::FrameGraphPlan,
        queued_packets: &[super::super::queue::DrawPacket],
    ) -> RhiResult<()> {
        let mut encoder = self.device.begin_commands("renderer-frame")?;

        for pass in &plan.graph.passes {
            self.submit_barriers(&mut *encoder, &plan, pass)
                .map_err(|error| pass_error(pass, "barriers", error))?;
            let render_pass = self
                .build_render_pass(&plan, pass)
                .map_err(|error| pass_error(pass, "attachments", error))?;
            let is_tone_mapping = pass.name == "Tone Mapping Pass";
            let has_render_targets = !render_pass.colors.is_empty() || render_pass.depth.is_some();

            if is_tone_mapping {
                self.submit_tone_mapping(&mut *encoder, &plan, pass, render_pass)
                    .map_err(|error| pass_error(pass, "tone mapping", error))?;
            } else if has_render_targets {
                let pass_kind = pass_kind(&pass.name);
                if let Some(pass_kind) = pass_kind {
                    self.validate_packets(&queued_packets, pass_kind, &pass.name, &render_pass)
                        .map_err(|error| pass_error(pass, "packet validation", error))?;
                }
                encoder
                    .begin_render_pass(render_pass)
                    .map_err(|error| pass_error(pass, "begin render pass", error))?;
                if let Some(pass_kind) = pass_kind {
                    submit_packets(&mut *encoder, &queued_packets, pass_kind, &self.materials)
                        .map_err(|error| pass_error(pass, "draw submission", error))?;
                }
                encoder
                    .end_render_pass()
                    .map_err(|error| pass_error(pass, "end render pass", error))?;
            }

            if pass.name == "Present" {
                submit_present(&mut *encoder, &plan)
                    .map_err(|error| pass_error(pass, "copy to swapchain", error))?;
            }
        }

        let command_buffer = encoder.finish()?;
        self.device.submit(command_buffer)?;
        Ok(())
    }

    fn ensure_tone_mapping_bind_group(
        &mut self,
        plan: &super::types::FrameGraphPlan,
    ) -> RhiResult<()> {
        let Some(config) = self.tone_mapping else {
            return Ok(());
        };
        let Some(source) = plan
            .graph
            .passes
            .iter()
            .find(|pass| pass.name == "Tone Mapping Pass")
            .and_then(|pass| {
                pass.uses
                    .iter()
                    .find(|resource_use| resource_use.access == AccessType::Read)
            })
            .and_then(|resource_use| plan.physical_texture(resource_use.resource))
        else {
            return Ok(());
        };

        let source_changed = self.tone_mapping_bound_source != Some(source)
            || self.tone_mapping_bind_group_config != Some(config);
        if !source_changed {
            return Ok(());
        }

        self.discard_tone_mapping_bind_group().map_err(|error| {
            RhiError::Message(format!("discard previous tone bind group: {error}"))
        })?;
        let bind_group = self
            .create_bind_group(BindGroupDesc::new(
                config.pipeline,
                0,
                vec![crate::rhi::types::BindGroupEntry::new(
                    0,
                    BindingResource::CombinedImageSampler {
                        texture: source,
                        sampler: config.sampler,
                    },
                )],
            ))
            .map_err(|error| {
                RhiError::Message(format!(
                    "create tone bind group (pipeline={:?}, source={:?}, sampler={:?}): {error}",
                    config.pipeline, source, config.sampler
                ))
            })?;
        self.tone_mapping_bind_group = Some(bind_group);
        self.tone_mapping_bound_source = Some(source);
        self.tone_mapping_bind_group_config = Some(config);
        Ok(())
    }

    fn submit_barriers(
        &mut self,
        encoder: &mut dyn crate::rhi::command::CommandEncoder,
        plan: &super::types::FrameGraphPlan,
        pass: &CompiledPass,
    ) -> RhiResult<()> {
        for &(resource, before, after) in &pass.barriers_before {
            if let Some(texture) = plan.physical_texture(resource) {
                encoder.barrier(ResourceBarrier {
                    resource: ResourceHandle::Texture(texture),
                    before,
                    after,
                })?;
            }
        }
        Ok(())
    }

    fn build_render_pass(
        &self,
        plan: &super::types::FrameGraphPlan,
        pass: &CompiledPass,
    ) -> RhiResult<RenderPassDesc> {
        let mut render_pass = RenderPassDesc {
            label: pass.name.clone(),
            ..Default::default()
        };
        for resource_use in &pass.uses {
            let Some(texture) = plan.physical_texture(resource_use.resource) else {
                continue;
            };
            match resource_use.access {
                AccessType::ColorWrite | AccessType::ReadWrite => {
                    let attachment = RenderAttachment {
                        texture,
                        load: match resource_use.access {
                            AccessType::ColorWrite => LoadOp::Clear,
                            _ => LoadOp::Load,
                        },
                        store: StoreOp::Store,
                        clear: match resource_use.access {
                            AccessType::ColorWrite => ClearValue::color([0.0, 0.0, 0.0, 1.0]),
                            _ => ClearValue::default(),
                        },
                    };
                    if !render_pass.colors.contains(&attachment) {
                        render_pass.colors.push(attachment);
                    }
                }
                AccessType::DepthRead | AccessType::DepthWrite => {
                    render_pass.depth.get_or_insert(RenderAttachment {
                        texture,
                        load: match resource_use.access {
                            AccessType::DepthWrite => LoadOp::Clear,
                            _ => LoadOp::Load,
                        },
                        store: StoreOp::Store,
                        clear: match resource_use.access {
                            AccessType::DepthWrite => ClearValue::depth_stencil(1.0, 0),
                            _ => ClearValue::default(),
                        },
                    });
                }
                AccessType::Read
                | AccessType::StorageRead
                | AccessType::StorageWrite
                | AccessType::Present => {}
            }
        }
        Ok(render_pass)
    }

    fn validate_packets(
        &mut self,
        packets: &[super::super::queue::DrawPacket],
        pass_kind: RenderPassKind,
        pass_name: &str,
        render_pass: &RenderPassDesc,
    ) -> RhiResult<()> {
        let color_formats = render_pass
            .colors
            .iter()
            .map(|attachment| {
                self.resources
                    .texture(attachment.texture)
                    .map(|texture| texture.format)
                    .ok_or(RhiError::InvalidHandle)
            })
            .collect::<RhiResult<Vec<_>>>()?;
        let depth_format = render_pass
            .depth
            .as_ref()
            .map(|attachment| {
                self.resources
                    .texture(attachment.texture)
                    .map(|texture| texture.format)
                    .ok_or(RhiError::InvalidHandle)
            })
            .transpose()?;

        if self.validated_graph_key != self.frame_graph_key {
            self.validated_graph_key = self.frame_graph_key;
            self.validated_pipelines.clear();
        }
        for packet in packets_for_pass(packets, pass_kind) {
            let pipeline = packet
                .material
                .map(|handle| {
                    self.materials
                        .get(handle)
                        .map(|material| material.pipeline)
                        .ok_or(RhiError::InvalidHandle)
                })
                .transpose()?
                .unwrap_or(packet.pipeline);
            if !self.validated_pipelines.insert((pass_kind, pipeline)) {
                continue;
            }
            let pipeline_desc = self
                .resources
                .pipeline(pipeline)
                .ok_or(RhiError::InvalidHandle)?;
            validate_pipeline_attachments(pass_name, &color_formats, depth_format, pipeline_desc)?;
        }
        Ok(())
    }

    fn submit_tone_mapping(
        &mut self,
        encoder: &mut dyn crate::rhi::command::CommandEncoder,
        plan: &super::types::FrameGraphPlan,
        pass: &CompiledPass,
        render_pass: RenderPassDesc,
    ) -> RhiResult<()> {
        let source = pass
            .uses
            .iter()
            .find(|resource_use| resource_use.access == AccessType::Read)
            .and_then(|resource_use| plan.physical_texture(resource_use.resource));
        let destination = pass
            .uses
            .iter()
            .find(|resource_use| {
                matches!(
                    resource_use.access,
                    AccessType::ColorWrite | AccessType::ReadWrite
                )
            })
            .and_then(|resource_use| plan.physical_texture(resource_use.resource));

        let (Some(source), Some(destination)) = (source, destination) else {
            return Ok(());
        };
        if let Some(config) = self.tone_mapping {
            if self.tone_mapping_bound_source != Some(source)
                || self.tone_mapping_bind_group_config != Some(config)
            {
                return Err(RhiError::Message(String::from(
                    "tone bind group was not prepared before command recording",
                )));
            }
            let bind_group = self
                .tone_mapping_bind_group
                .ok_or_else(|| RhiError::Message(String::from("tone bind group is missing")))?;
            let destination_format = self
                .resources
                .texture(destination)
                .map(|texture| texture.format)
                .ok_or_else(|| {
                    RhiError::Message(format!(
                        "tone destination texture {:?} is not registered",
                        destination
                    ))
                })?;
            let pipeline = self.resources.pipeline(config.pipeline).ok_or_else(|| {
                RhiError::Message(format!(
                    "tone pipeline {:?} is not registered",
                    config.pipeline
                ))
            })?;
            validate_pipeline_attachments(
                "Tone Mapping Pass",
                &[destination_format],
                None,
                pipeline,
            )?;
            encoder
                .begin_render_pass(render_pass)
                .map_err(|error| RhiError::Message(format!("begin tone render pass: {error}")))?;
            encoder.bind_pipeline(config.pipeline).map_err(|error| {
                RhiError::Message(format!("bind tone pipeline {:?}: {error}", config.pipeline))
            })?;
            encoder.bind_group(0, bind_group, &[]).map_err(|error| {
                RhiError::Message(format!("bind tone group {:?}: {error}", bind_group))
            })?;
            encoder
                .bind_vertex_buffer(0, config.vertex_buffer, 0)
                .map_err(|error| {
                    RhiError::Message(format!(
                        "bind tone vertex buffer {:?}: {error}",
                        config.vertex_buffer
                    ))
                })?;
            encoder
                .draw(3, 0)
                .map_err(|error| RhiError::Message(format!("draw tone triangle: {error}")))?;
            encoder
                .end_render_pass()
                .map_err(|error| RhiError::Message(format!("end tone render pass: {error}")))?;
        } else if source != destination {
            encoder.barrier(ResourceBarrier {
                resource: ResourceHandle::Texture(source),
                before: ResourceState::ShaderRead,
                after: ResourceState::CopySrc,
            })?;
            encoder.barrier(ResourceBarrier {
                resource: ResourceHandle::Texture(destination),
                before: ResourceState::ColorAttachment,
                after: ResourceState::CopyDst,
            })?;
            encoder.blit_texture(source, destination)?;
            encoder.barrier(ResourceBarrier {
                resource: ResourceHandle::Texture(source),
                before: ResourceState::CopySrc,
                after: ResourceState::ShaderRead,
            })?;
            encoder.barrier(ResourceBarrier {
                resource: ResourceHandle::Texture(destination),
                before: ResourceState::CopyDst,
                after: ResourceState::ColorAttachment,
            })?;
        }
        Ok(())
    }
}

fn pass_error(pass: &CompiledPass, stage: &str, error: RhiError) -> RhiError {
    RhiError::Message(format!("{} ({stage}): {error}", pass.name))
}

fn submit_packets(
    encoder: &mut dyn crate::rhi::command::CommandEncoder,
    packets: &[super::super::queue::DrawPacket],
    pass_kind: RenderPassKind,
    materials: &super::super::material::MaterialManager,
) -> RhiResult<()> {
    let mut bound_pipeline = None;
    let mut bound_set_zero = None;
    for packet in packets_for_pass(packets, pass_kind) {
        let material = packet
            .material
            .map(|handle| materials.get(handle).ok_or(RhiError::InvalidHandle))
            .transpose()?;
        let pipeline = material
            .as_ref()
            .map_or(packet.pipeline, |material| material.pipeline);
        if bound_pipeline != Some(pipeline) {
            encoder.bind_pipeline(pipeline)?;
            bound_pipeline = Some(pipeline);
            bound_set_zero = None;
        }
        if let Some(bind_group) = packet.bind_group {
            if bound_set_zero != Some((pipeline, bind_group)) {
                encoder.bind_group(0, bind_group, &[])?;
                bound_set_zero = Some((pipeline, bind_group));
            }
        }
        if let Some(material) = material.as_ref() {
            for (set, bind_group) in material.bind_groups.iter().enumerate() {
                if packet.bind_group == Some(*bind_group) {
                    continue;
                }
                if set == 0 {
                    if bound_set_zero != Some((pipeline, *bind_group)) {
                        encoder.bind_group(0, *bind_group, &[])?;
                        bound_set_zero = Some((pipeline, *bind_group));
                    }
                } else {
                    encoder.bind_group(set as u32, *bind_group, &[])?;
                }
            }
            if !material.push_constants.is_empty() {
                encoder.push_constants(pipeline, 0, &material.push_constants)?;
            }
        }
        if let Some(push_constants) = packet.push_constants {
            encoder.push_constants(pipeline, 0, &push_constants)?;
        }
        encoder.bind_vertex_buffer(0, packet.vertex_buffer, 0)?;
        if let Some(index_buffer) = packet.index_buffer {
            encoder.bind_index_buffer(index_buffer, 0)?;
            encoder.draw_indexed(
                packet.index_count,
                1,
                packet.first_index,
                packet.vertex_offset,
                0,
            )?;
        } else {
            encoder.draw(packet.index_count, packet.first_index)?;
        }
    }
    Ok(())
}

fn packets_for_pass(
    packets: &[super::super::queue::DrawPacket],
    pass: RenderPassKind,
) -> &[super::super::queue::DrawPacket] {
    let start = packets.partition_point(|packet| packet.pass < pass);
    let end = packets.partition_point(|packet| packet.pass <= pass);
    &packets[start..end]
}

fn submit_present(
    encoder: &mut dyn crate::rhi::command::CommandEncoder,
    plan: &super::types::FrameGraphPlan,
) -> RhiResult<()> {
    let (Some(source), Some(destination)) = (
        plan.physical_texture(plan.resources.sdr_output),
        plan.physical_texture(plan.resources.swapchain),
    ) else {
        return Ok(());
    };
    if source == destination {
        return Ok(());
    }
    encoder.barrier(ResourceBarrier {
        resource: ResourceHandle::Texture(source),
        before: ResourceState::ShaderRead,
        after: ResourceState::CopySrc,
    })?;
    encoder.barrier(ResourceBarrier {
        resource: ResourceHandle::Texture(destination),
        before: ResourceState::Present,
        after: ResourceState::CopyDst,
    })?;
    encoder.copy_texture(source, destination)?;
    encoder.barrier(ResourceBarrier {
        resource: ResourceHandle::Texture(source),
        before: ResourceState::CopySrc,
        after: ResourceState::ShaderRead,
    })?;
    encoder.barrier(ResourceBarrier {
        resource: ResourceHandle::Texture(destination),
        before: ResourceState::CopyDst,
        after: ResourceState::Present,
    })?;
    Ok(())
}

fn pass_kind(name: &str) -> Option<RenderPassKind> {
    match name {
        "Sky Pass" => Some(RenderPassKind::Sky),
        "Geometry Pass" => Some(RenderPassKind::Geometry),
        "Lighting Pass" => Some(RenderPassKind::Lighting),
        "Transparent Pass" => Some(RenderPassKind::Transparent),
        "First Person Pass" => Some(RenderPassKind::FirstPerson),
        "Post Processing Pass" => Some(RenderPassKind::PostProcess),
        "GUI Pass" => Some(RenderPassKind::Gui),
        _ => None,
    }
}

pub(crate) fn validate_pipeline_attachments(
    pass_label: &str,
    color_formats: &[TextureFormat],
    depth_format: Option<TextureFormat>,
    pipeline: &crate::rhi::types::PipelineDesc,
) -> RhiResult<()> {
    if pipeline.color_formats != color_formats || pipeline.depth_format != depth_format {
        return Err(RhiError::Message(format!(
            "pipeline '{}' attachments do not match {pass_label}: pipeline colors={:?}, depth={:?}; pass colors={:?}, depth={:?}",
            pipeline.label,
            pipeline.color_formats,
            pipeline.depth_format,
            color_formats,
            depth_format,
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_attachments_must_match_the_dynamic_rendering_pass() {
        let colors = [TextureFormat::R16G16B16A16Float, TextureFormat::R16G16Float];
        let mut pipeline = crate::rhi::types::PipelineDesc {
            color_formats: colors.to_vec(),
            depth_format: Some(TextureFormat::D32Float),
            label: String::from("test.pipeline"),
            ..crate::rhi::types::PipelineDesc::default()
        };
        assert!(validate_pipeline_attachments(
            "Geometry Pass",
            &colors,
            Some(TextureFormat::D32Float),
            &pipeline,
        )
        .is_ok());
        pipeline.depth_format = None;
        let error = validate_pipeline_attachments(
            "Geometry Pass",
            &colors,
            Some(TextureFormat::D32Float),
            &pipeline,
        )
        .unwrap_err();
        assert!(error.to_string().contains("test.pipeline"));
        pipeline.depth_format = Some(TextureFormat::D32Float);
        pipeline.color_formats.pop();
        assert!(validate_pipeline_attachments(
            "Geometry Pass",
            &colors,
            Some(TextureFormat::D32Float),
            &pipeline,
        )
        .is_err());
    }
}
