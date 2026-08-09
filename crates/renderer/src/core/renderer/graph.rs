use crate::rhi::device::RenderDevice;
use crate::rhi::types::{Extent2D, TextureDesc, TextureFormat, TextureHandle, TextureUsage};

use super::types::{FrameGraphPlan, FrameGraphResources, FrameInputs};
use super::{FrameGraphKey, RendererRuntime, TextureGraphKey};

impl<D: RenderDevice> RendererRuntime<D> {
    pub fn prepare_frame(
        &mut self,
        inputs: FrameInputs,
    ) -> Result<&FrameGraphPlan, super::super::render_graph::GraphError> {
        self.queue.clear();
        let key = self.frame_graph_key(inputs);
        let rebuild_graph = self.frame_graph_key != Some(key) || self.last_plan.is_none();
        let previous_plan = self.last_plan.take();
        let (resources, compiled_graph) = if rebuild_graph {
            self.graph.clear();
            let resources = self.build_default_graph(inputs);
            let compiled_graph = self.graph.compile()?;
            self.frame_graph_key = Some(key);
            (resources, compiled_graph)
        } else {
            let previous_plan = previous_plan.expect("cached frame graph plan is missing");
            let resources = previous_plan.resources;
            self.refresh_default_graph_imports(inputs, resources)?;
            (resources, previous_plan.graph)
        };
        if rebuild_graph {
            let descriptions = self
                .graph
                .resources()
                .filter_map(|(_, resource)| match resource {
                    super::super::render_graph::GraphResource::Transient(desc) => {
                        Some(desc.clone())
                    }
                    super::super::render_graph::GraphResource::Imported { .. } => None,
                })
                .collect::<Vec<_>>();

            if self.transient_descriptions != descriptions {
                let _ = self.discard_tone_mapping_bind_group();
                for handle in self.transient_textures.drain(..) {
                    let _ = self.device.destroy_texture(handle);
                    let _ = self.resources.destroy_texture(handle);
                }
                self.transient_descriptions.clear();

                let mut new_handles = Vec::with_capacity(descriptions.len());
                for desc in &descriptions {
                    match self.create_texture(desc.clone()) {
                        Ok(handle) => new_handles.push(handle),
                        Err(error) => {
                            for handle in new_handles {
                                let _ = self.device.destroy_texture(handle);
                                let _ = self.resources.destroy_texture(handle);
                            }
                            return Err(
                                super::super::render_graph::GraphError::ResourceAllocation(
                                    error.to_string(),
                                ),
                            );
                        }
                    }
                }
                self.transient_textures = new_handles;
                self.transient_descriptions = descriptions;
            }
        }

        let mut transient_handles = self.transient_textures.iter().copied();
        let physical_textures = self
            .graph
            .resources()
            .filter_map(|(id, resource)| match resource {
                super::super::render_graph::GraphResource::Transient(_) => {
                    transient_handles.next().map(|handle| (id, handle))
                }
                super::super::render_graph::GraphResource::Imported { handle, .. } => {
                    handle.is_valid().then_some((id, *handle))
                }
            })
            .collect();

        let plan = FrameGraphPlan {
            resources,
            graph: compiled_graph,
            physical_textures,
        };
        self.last_plan = Some(plan);
        Ok(self.last_plan.as_ref().expect("frame plan was just stored"))
    }

    fn frame_graph_key(&self, inputs: FrameInputs) -> FrameGraphKey {
        let render_extent = Extent2D::new(inputs.render_width.max(1), inputs.render_height.max(1));
        let display_extent =
            Extent2D::new(inputs.display_width.max(1), inputs.display_height.max(1));
        let [hdr_color, depth, motion_vectors, post_process, sdr_output, swapchain] =
            frame_texture_descriptions(render_extent, display_extent);

        FrameGraphKey {
            render_extent,
            display_extent,
            enable_post_processing: inputs.enable_post_processing,
            targets: [
                self.texture_graph_key(inputs.targets.hdr_color, &hdr_color, false),
                self.texture_graph_key(inputs.targets.depth, &depth, false),
                self.texture_graph_key(inputs.targets.motion_vectors, &motion_vectors, false),
                self.texture_graph_key(inputs.targets.post_process, &post_process, false),
                self.texture_graph_key(inputs.targets.sdr_output, &sdr_output, false),
                self.texture_graph_key(inputs.targets.swapchain, &swapchain, true),
            ],
        }
    }

    fn texture_graph_key(
        &self,
        handle: Option<TextureHandle>,
        fallback: &TextureDesc,
        external: bool,
    ) -> TextureGraphKey {
        let valid_handle = handle.filter(|handle| handle.is_valid());
        let imported = if external {
            valid_handle.is_some()
        } else {
            valid_handle.is_some_and(|handle| self.resources.texture(handle).is_some())
        };
        let desc = if imported {
            valid_handle
                .and_then(|handle| self.resources.texture(handle))
                .unwrap_or(fallback)
        } else {
            fallback
        };
        TextureGraphKey {
            imported,
            extent: desc.extent,
            format: desc.format,
            usage: desc.usage,
            mip_levels: desc.mip_levels,
            array_layers: desc.array_layers,
        }
    }

    fn refresh_default_graph_imports(
        &mut self,
        inputs: FrameInputs,
        resources: FrameGraphResources,
    ) -> Result<(), super::super::render_graph::GraphError> {
        let render_extent = Extent2D::new(inputs.render_width.max(1), inputs.render_height.max(1));
        let display_extent =
            Extent2D::new(inputs.display_width.max(1), inputs.display_height.max(1));
        let [hdr_color, depth, motion_vectors, post_process, sdr_output, swapchain] =
            frame_texture_descriptions(render_extent, display_extent);

        self.refresh_imported_frame_texture(
            resources.hdr_color,
            inputs.targets.hdr_color,
            hdr_color,
            false,
        )?;
        self.refresh_imported_frame_texture(resources.depth, inputs.targets.depth, depth, false)?;
        self.refresh_imported_frame_texture(
            resources.motion_vectors,
            inputs.targets.motion_vectors,
            motion_vectors,
            false,
        )?;
        self.refresh_imported_frame_texture(
            resources.post_process,
            inputs.targets.post_process,
            post_process,
            false,
        )?;
        self.refresh_imported_frame_texture(
            resources.sdr_output,
            inputs.targets.sdr_output,
            sdr_output,
            false,
        )?;
        self.refresh_imported_frame_texture(
            resources.swapchain,
            inputs.targets.swapchain,
            swapchain,
            true,
        )?;
        Ok(())
    }

    fn refresh_imported_frame_texture(
        &mut self,
        resource: super::super::render_graph::GraphResourceId,
        handle: Option<TextureHandle>,
        fallback: TextureDesc,
        external: bool,
    ) -> Result<(), super::super::render_graph::GraphError> {
        let Some(handle) = handle.filter(|handle| handle.is_valid()) else {
            return Ok(());
        };
        if !external && self.resources.texture(handle).is_none() {
            return Ok(());
        }
        let desc = self.resources.texture(handle).cloned().unwrap_or(fallback);
        self.graph.update_imported_texture(resource, handle, desc)
    }

    fn build_default_graph(&mut self, inputs: FrameInputs) -> FrameGraphResources {
        let render_extent = Extent2D::new(inputs.render_width.max(1), inputs.render_height.max(1));
        let display_extent =
            Extent2D::new(inputs.display_width.max(1), inputs.display_height.max(1));
        let [hdr_desc, depth_desc, motion_vectors_desc, post_process_desc, sdr_output_desc, swapchain_desc] =
            frame_texture_descriptions(render_extent, display_extent);
        let hdr_color = self.add_or_import_frame_texture(inputs.targets.hdr_color, hdr_desc);
        let depth = self.add_or_import_frame_texture(inputs.targets.depth, depth_desc);
        let motion_vectors =
            self.add_or_import_frame_texture(inputs.targets.motion_vectors, motion_vectors_desc);
        let post_process =
            self.add_or_import_frame_texture(inputs.targets.post_process, post_process_desc);
        let sdr_output =
            self.add_or_import_frame_texture(inputs.targets.sdr_output, sdr_output_desc);
        let swapchain =
            self.add_or_import_external_frame_texture(inputs.targets.swapchain, swapchain_desc);

        // Sky has no depth dependency. Keeping it in its own pass makes the
        // clear/load contract explicit and prevents an unstable packet order
        // from allowing the fullscreen sky draw to cover entities.
        let sky = self.graph.add_pass("Sky Pass");
        let _ = self.graph.write_color(sky, hdr_color);
        let _ = self.graph.write_color(sky, motion_vectors);

        let geometry = self.graph.add_pass("Geometry Pass");
        let _ = self.graph.use_resource(
            geometry,
            hdr_color,
            super::super::render_graph::AccessType::ReadWrite,
        );
        let _ = self.graph.write_depth(geometry, depth);
        let _ = self.graph.use_resource(
            geometry,
            motion_vectors,
            super::super::render_graph::AccessType::ReadWrite,
        );

        let lighting = self.graph.add_pass("Lighting Pass");
        let _ = self.graph.use_resource(
            lighting,
            hdr_color,
            super::super::render_graph::AccessType::ReadWrite,
        );
        let _ = self.graph.use_resource(
            lighting,
            depth,
            super::super::render_graph::AccessType::DepthRead,
        );

        let transparent = self.graph.add_pass("Transparent Pass");
        let _ = self.graph.use_resource(
            transparent,
            hdr_color,
            super::super::render_graph::AccessType::ReadWrite,
        );
        let _ = self.graph.use_resource(
            transparent,
            motion_vectors,
            super::super::render_graph::AccessType::ReadWrite,
        );
        let _ = self.graph.use_resource(
            transparent,
            depth,
            super::super::render_graph::AccessType::DepthRead,
        );

        // Vanilla renders the hand after the world and clears depth first.
        // Model this as a graph pass so every backend receives the same
        // attachment contract instead of relying on native API state.
        let first_person = self.graph.add_pass("First Person Pass");
        let _ = self.graph.use_resource(
            first_person,
            hdr_color,
            super::super::render_graph::AccessType::ReadWrite,
        );
        let _ = self.graph.use_resource(
            first_person,
            motion_vectors,
            super::super::render_graph::AccessType::ReadWrite,
        );
        let _ = self.graph.write_depth(first_person, depth);

        if inputs.enable_post_processing {
            let post = self.graph.add_pass("Post Processing Pass");
            let _ = self.graph.read(post, hdr_color);
            let _ = self.graph.use_resource(
                post,
                depth,
                super::super::render_graph::AccessType::DepthRead,
            );
            let _ = self.graph.read(post, motion_vectors);
            let _ = self.graph.write_storage(post, post_process);
        }

        let tone_input = if inputs.enable_post_processing {
            post_process
        } else {
            hdr_color
        };
        let tone = self.graph.add_pass("Tone Mapping Pass");
        let _ = self.graph.read(tone, tone_input);
        let _ = self.graph.write_color(tone, sdr_output);

        let gui = self.graph.add_pass("GUI Pass");
        let _ = self.graph.use_resource(
            gui,
            sdr_output,
            super::super::render_graph::AccessType::ReadWrite,
        );

        let present = self.graph.add_pass("Present");
        let _ = self.graph.read(present, sdr_output);
        let _ = self.graph.present(present, swapchain);

        FrameGraphResources {
            hdr_color,
            depth,
            motion_vectors,
            post_process,
            sdr_output,
            swapchain,
        }
    }

    fn add_or_import_frame_texture(
        &mut self,
        handle: Option<TextureHandle>,
        fallback: TextureDesc,
    ) -> super::super::render_graph::GraphResourceId {
        let handle = handle
            .filter(|handle| handle.is_valid())
            .filter(|handle| self.resources.texture(*handle).is_some());
        let desc = handle
            .and_then(|handle| self.resources.texture(handle).cloned())
            .unwrap_or(fallback);
        add_or_import_texture(&mut self.graph, handle, desc)
    }

    fn add_or_import_external_frame_texture(
        &mut self,
        handle: Option<TextureHandle>,
        fallback: TextureDesc,
    ) -> super::super::render_graph::GraphResourceId {
        let handle = handle.filter(|handle| handle.is_valid());
        add_or_import_texture(&mut self.graph, handle, fallback)
    }
}

fn frame_texture(
    extent: Extent2D,
    format: TextureFormat,
    usage: TextureUsage,
    label: &str,
) -> TextureDesc {
    let mut desc = TextureDesc::new(extent, format, usage);
    desc.label = label.to_string();
    desc
}

fn frame_texture_descriptions(
    render_extent: Extent2D,
    display_extent: Extent2D,
) -> [TextureDesc; 6] {
    [
        frame_texture(
            render_extent,
            TextureFormat::R16G16B16A16Float,
            TextureUsage::COLOR_ATTACHMENT
                | TextureUsage::SAMPLED
                | TextureUsage::STORAGE
                | TextureUsage::TRANSFER_SRC,
            "frame.hdr_color",
        ),
        frame_texture(
            render_extent,
            TextureFormat::D32Float,
            TextureUsage::DEPTH_ATTACHMENT | TextureUsage::SAMPLED,
            "frame.depth",
        ),
        frame_texture(
            render_extent,
            TextureFormat::R16G16Float,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::SAMPLED,
            "frame.motion_vectors",
        ),
        frame_texture(
            render_extent,
            TextureFormat::R16G16B16A16Float,
            TextureUsage::COLOR_ATTACHMENT
                | TextureUsage::SAMPLED
                | TextureUsage::STORAGE
                | TextureUsage::TRANSFER_SRC,
            "frame.post_process",
        ),
        frame_texture(
            display_extent,
            TextureFormat::B8G8R8A8Srgb,
            TextureUsage::COLOR_ATTACHMENT
                | TextureUsage::SAMPLED
                | TextureUsage::TRANSFER_SRC
                | TextureUsage::TRANSFER_DST,
            "frame.sdr_output",
        ),
        frame_texture(
            display_extent,
            TextureFormat::B8G8R8A8Srgb,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::PRESENT | TextureUsage::TRANSFER_DST,
            "frame.swapchain",
        ),
    ]
}

fn add_or_import_texture(
    graph: &mut super::super::render_graph::RenderGraph,
    handle: Option<TextureHandle>,
    desc: TextureDesc,
) -> super::super::render_graph::GraphResourceId {
    match handle.filter(|handle| handle.is_valid()) {
        Some(handle) => graph.import_texture(handle, desc),
        None => graph.add_texture(desc),
    }
}
