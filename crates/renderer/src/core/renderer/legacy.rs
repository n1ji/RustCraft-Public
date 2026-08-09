use crate::rhi::device::RenderDevice;
use crate::rhi::types::{
    BufferUsage, RhiError, RhiResult, TextureDesc, TextureFormat, TextureUsage,
};

use super::super::queue::RenderQueue;
use super::super::render_graph::{CompiledRenderGraph, RenderGraph};
use super::super::resource::ResourceManager;
use super::{CoreRenderer, FrameDesc, Renderer};

impl<D: RenderDevice> CoreRenderer<D> {
    pub fn new(device: D) -> Self {
        Self {
            device,
            resources: ResourceManager::new(),
            graph: RenderGraph::new(),
            queue: RenderQueue::default(),
            last_compiled_graph: None,
            frame_index: 0,
        }
    }

    pub fn into_device(self) -> D {
        self.device
    }

    fn build_default_graph(
        &mut self,
        desc: FrameDesc,
    ) -> Result<(), super::super::render_graph::GraphError> {
        self.graph.clear();
        let color = self.graph.add_texture(TextureDesc::new(
            desc.extent,
            TextureFormat::Rgba16Float,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::SAMPLED,
        ));
        let depth = self.graph.add_texture(TextureDesc::new(
            desc.extent,
            TextureFormat::D32Float,
            TextureUsage::DEPTH_ATTACHMENT,
        ));
        let geometry = self.graph.add_pass("geometry");
        self.graph.write_color(geometry, color)?;
        self.graph.write_depth(geometry, depth)?;
        let lighting = self.graph.add_pass("lighting");
        self.graph.read(lighting, color)?;
        self.graph.write_storage(lighting, color)?;
        if desc.enable_post_processing {
            let post = self.graph.add_pass("post-processing");
            self.graph.read(post, color)?;
            self.graph.write_storage(post, color)?;
        }
        let gui = self.graph.add_pass("gui");
        self.graph.read(gui, color)?;
        self.graph.write_color(gui, color)?;
        let present = self.graph.add_pass("present");
        self.graph.present(present, color)?;
        Ok(())
    }
}

impl<D: RenderDevice> Renderer for CoreRenderer<D> {
    fn device(&mut self) -> &mut dyn RenderDevice {
        &mut self.device
    }

    fn resources(&self) -> &ResourceManager {
        &self.resources
    }

    fn resources_mut(&mut self) -> &mut ResourceManager {
        &mut self.resources
    }

    fn render_graph(&self) -> &RenderGraph {
        &self.graph
    }

    fn render_graph_mut(&mut self) -> &mut RenderGraph {
        &mut self.graph
    }

    fn render_queue(&self) -> &RenderQueue {
        &self.queue
    }

    fn render_queue_mut(&mut self) -> &mut RenderQueue {
        &mut self.queue
    }

    fn begin_frame(&mut self, desc: FrameDesc) -> RhiResult<&CompiledRenderGraph> {
        if desc.extent.is_empty() {
            return Err(RhiError::InvalidDescription(
                "frame extent must be non-zero",
            ));
        }
        self.build_default_graph(desc)
            .map_err(|error| RhiError::Message(error.to_string()))?;
        let compiled = self
            .graph
            .compile()
            .map_err(|error| RhiError::Message(error.to_string()))?;
        self.last_compiled_graph = Some(compiled);
        Ok(self.last_compiled_graph.as_ref().expect("graph was stored"))
    }

    fn end_frame(&mut self) -> RhiResult<()> {
        self.device.collect_completed()?;
        let encoder = self.device.begin_commands("renderer-frame")?;
        let command_buffer = encoder.finish()?;
        self.device.submit(command_buffer)?;
        self.frame_index = self.frame_index.wrapping_add(1);
        Ok(())
    }

    fn frame_index(&self) -> u64 {
        self.frame_index
    }
}

#[allow(dead_code)]
fn _default_buffer_usage() -> BufferUsage {
    BufferUsage::VERTEX | BufferUsage::INDEX | BufferUsage::STORAGE
}
