use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::rhi::types::{ResourceState, TextureDesc, TextureHandle};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GraphResourceId(u32);

impl GraphResourceId {
    pub const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PassId(u32);

impl PassId {
    pub const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphResource {
    Transient(TextureDesc),
    Imported {
        handle: TextureHandle,
        desc: TextureDesc,
    },
}

impl GraphResource {
    pub fn desc(&self) -> &TextureDesc {
        match self {
            Self::Transient(desc) | Self::Imported { desc, .. } => desc,
        }
    }

    pub fn imported_handle(&self) -> Option<TextureHandle> {
        match self {
            Self::Transient(_) => None,
            Self::Imported { handle, .. } => Some(*handle),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessType {
    Read,
    DepthRead,
    ColorWrite,
    DepthWrite,
    StorageRead,
    StorageWrite,
    ReadWrite,
    Present,
}

impl AccessType {
    pub const fn reads(self) -> bool {
        matches!(
            self,
            Self::Read | Self::DepthRead | Self::StorageRead | Self::ReadWrite | Self::Present
        )
    }

    pub const fn writes(self) -> bool {
        matches!(
            self,
            Self::ColorWrite
                | Self::DepthWrite
                | Self::StorageWrite
                | Self::ReadWrite
                | Self::Present
        )
    }

    pub const fn state(self) -> ResourceState {
        match self {
            Self::Read | Self::StorageRead => ResourceState::ShaderRead,
            Self::DepthRead => ResourceState::DepthAttachment,
            Self::ColorWrite => ResourceState::ColorAttachment,
            Self::DepthWrite => ResourceState::DepthAttachment,
            Self::StorageWrite => ResourceState::StorageWrite,
            Self::ReadWrite => ResourceState::ColorAttachment,
            Self::Present => ResourceState::Present,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceUse {
    pub resource: GraphResourceId,
    pub access: AccessType,
}

#[derive(Clone, Debug)]
struct PassNode {
    name: String,
    uses: Vec<ResourceUse>,
    explicit_dependencies: Vec<PassId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLifetime {
    pub resource: GraphResourceId,
    pub first_pass: usize,
    pub last_pass: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledPass {
    pub id: PassId,
    pub name: String,
    pub uses: Vec<ResourceUse>,
    pub barriers_before: Vec<(GraphResourceId, ResourceState, ResourceState)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompiledRenderGraph {
    pub passes: Vec<CompiledPass>,
    pub lifetimes: Vec<ResourceLifetime>,
}

#[derive(Debug, Default)]
pub struct RenderGraph {
    resources: Vec<GraphResource>,
    passes: Vec<PassNode>,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.resources.clear();
        self.passes.clear();
    }

    pub fn add_texture(&mut self, desc: TextureDesc) -> GraphResourceId {
        let id = GraphResourceId(self.resources.len() as u32);
        self.resources.push(GraphResource::Transient(desc));
        id
    }

    pub fn import_texture(&mut self, handle: TextureHandle, desc: TextureDesc) -> GraphResourceId {
        let id = GraphResourceId(self.resources.len() as u32);
        self.resources
            .push(GraphResource::Imported { handle, desc });
        id
    }

    /// Refresh the physical texture behind an imported graph resource without
    /// touching the graph topology. Swapchain images rotate every frame, but
    /// the passes and resource dependencies do not. Keeping this update
    /// separate lets the renderer reuse the compiled graph safely.
    pub fn update_imported_texture(
        &mut self,
        id: GraphResourceId,
        handle: TextureHandle,
        desc: TextureDesc,
    ) -> Result<(), GraphError> {
        let resource = self
            .resources
            .get_mut(id.0 as usize)
            .ok_or(GraphError::InvalidResource(id))?;
        match resource {
            GraphResource::Imported {
                handle: imported_handle,
                desc: imported_desc,
            } => {
                *imported_handle = handle;
                *imported_desc = desc;
                Ok(())
            }
            GraphResource::Transient(_) => Err(GraphError::InvalidResource(id)),
        }
    }

    pub fn resource(&self, id: GraphResourceId) -> Option<&GraphResource> {
        self.resources.get(id.0 as usize)
    }

    pub fn add_pass(&mut self, name: impl Into<String>) -> PassId {
        let id = PassId(self.passes.len() as u32);
        self.passes.push(PassNode {
            name: name.into(),
            uses: Vec::new(),
            explicit_dependencies: Vec::new(),
        });
        id
    }

    pub fn use_resource(
        &mut self,
        pass: PassId,
        resource: GraphResourceId,
        access: AccessType,
    ) -> Result<(), GraphError> {
        if self.resources.get(resource.0 as usize).is_none() {
            return Err(GraphError::InvalidResource(resource));
        }
        let node = self
            .passes
            .get_mut(pass.0 as usize)
            .ok_or(GraphError::InvalidPass(pass))?;
        node.uses.push(ResourceUse { resource, access });
        Ok(())
    }

    pub fn read(&mut self, pass: PassId, resource: GraphResourceId) -> Result<(), GraphError> {
        self.use_resource(pass, resource, AccessType::Read)
    }

    pub fn write_color(
        &mut self,
        pass: PassId,
        resource: GraphResourceId,
    ) -> Result<(), GraphError> {
        self.use_resource(pass, resource, AccessType::ColorWrite)
    }

    pub fn write_depth(
        &mut self,
        pass: PassId,
        resource: GraphResourceId,
    ) -> Result<(), GraphError> {
        self.use_resource(pass, resource, AccessType::DepthWrite)
    }

    pub fn write_storage(
        &mut self,
        pass: PassId,
        resource: GraphResourceId,
    ) -> Result<(), GraphError> {
        self.use_resource(pass, resource, AccessType::StorageWrite)
    }

    pub fn present(&mut self, pass: PassId, resource: GraphResourceId) -> Result<(), GraphError> {
        self.use_resource(pass, resource, AccessType::Present)
    }

    pub fn depends_on(&mut self, pass: PassId, dependency: PassId) -> Result<(), GraphError> {
        if self.passes.get(pass.0 as usize).is_none() {
            return Err(GraphError::InvalidPass(pass));
        }
        if self.passes.get(dependency.0 as usize).is_none() {
            return Err(GraphError::InvalidPass(dependency));
        }
        self.passes[pass.0 as usize]
            .explicit_dependencies
            .push(dependency);
        Ok(())
    }

    pub fn compile(&self) -> Result<CompiledRenderGraph, GraphError> {
        let pass_count = self.passes.len();
        let mut edges = vec![Vec::<usize>::new(); pass_count];
        let mut indegree = vec![0usize; pass_count];
        let mut edge_set = HashSet::new();

        let mut add_edge = |from: usize, to: usize| {
            if from == to || !edge_set.insert((from, to)) {
                return;
            }
            edges[from].push(to);
            indegree[to] += 1;
        };

        for (pass_index, pass) in self.passes.iter().enumerate() {
            for dependency in &pass.explicit_dependencies {
                add_edge(dependency.0 as usize, pass_index);
            }
        }

        for first in 0..pass_count {
            for second in (first + 1)..pass_count {
                if passes_have_hazard(&self.passes[first], &self.passes[second]) {
                    add_edge(first, second);
                }
            }
        }

        let mut ready = VecDeque::new();
        for (index, &degree) in indegree.iter().enumerate() {
            if degree == 0 {
                ready.push_back(index);
            }
        }

        let mut order = Vec::with_capacity(pass_count);
        while let Some(index) = ready.pop_front() {
            order.push(index);
            for &next in &edges[index] {
                indegree[next] -= 1;
                if indegree[next] == 0 {
                    ready.push_back(next);
                }
            }
        }

        if order.len() != pass_count {
            return Err(GraphError::Cycle);
        }

        let mut last_states = HashMap::<GraphResourceId, ResourceState>::new();
        let mut compiled_passes = Vec::with_capacity(order.len());
        let mut lifetimes = HashMap::<GraphResourceId, (usize, usize)>::new();

        for (compiled_index, &pass_index) in order.iter().enumerate() {
            let pass = &self.passes[pass_index];
            let mut barriers_before = Vec::new();
            for resource_use in &pass.uses {
                let next_state = resource_use.access.state();
                let previous_state = last_states
                    .insert(resource_use.resource, next_state)
                    .unwrap_or(ResourceState::Undefined);
                if previous_state != next_state {
                    barriers_before.push((resource_use.resource, previous_state, next_state));
                }
                lifetimes
                    .entry(resource_use.resource)
                    .and_modify(|range| range.1 = compiled_index)
                    .or_insert((compiled_index, compiled_index));
            }
            compiled_passes.push(CompiledPass {
                id: PassId(pass_index as u32),
                name: pass.name.clone(),
                uses: pass.uses.clone(),
                barriers_before,
            });
        }

        let lifetimes = lifetimes
            .into_iter()
            .map(|(resource, (first_pass, last_pass))| ResourceLifetime {
                resource,
                first_pass,
                last_pass,
            })
            .collect();

        Ok(CompiledRenderGraph {
            passes: compiled_passes,
            lifetimes,
        })
    }

    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    pub fn resources(&self) -> impl Iterator<Item = (GraphResourceId, &GraphResource)> {
        self.resources
            .iter()
            .enumerate()
            .map(|(index, resource)| (GraphResourceId(index as u32), resource))
    }

    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }
}

fn passes_have_hazard(first: &PassNode, second: &PassNode) -> bool {
    first.uses.iter().any(|first_use| {
        second.uses.iter().any(|second_use| {
            first_use.resource == second_use.resource
                && (first_use.access.writes() || second_use.access.writes())
        })
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphError {
    InvalidResource(GraphResourceId),
    InvalidPass(PassId),
    Cycle,
    ResourceAllocation(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidResource(id) => write!(formatter, "invalid graph resource {}", id.0),
            Self::InvalidPass(id) => write!(formatter, "invalid graph pass {}", id.0),
            Self::Cycle => formatter.write_str("render graph contains a cycle"),
            Self::ResourceAllocation(message) => {
                write!(
                    formatter,
                    "render graph resource allocation failed: {message}"
                )
            }
        }
    }
}

impl std::error::Error for GraphError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhi::types::{Extent2D, TextureFormat, TextureUsage};

    fn texture() -> TextureDesc {
        TextureDesc::new(
            Extent2D::new(16, 16),
            TextureFormat::Rgba8Unorm,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::SAMPLED,
        )
    }

    #[test]
    fn hazards_order_passes_and_emit_barriers() {
        let mut graph = RenderGraph::new();
        let texture = graph.add_texture(texture());
        let geometry = graph.add_pass("geometry");
        graph.write_color(geometry, texture).unwrap();
        let lighting = graph.add_pass("lighting");
        graph.read(lighting, texture).unwrap();

        let compiled = graph.compile().unwrap();
        assert_eq!(compiled.passes[0].name, "geometry");
        assert_eq!(compiled.passes[0].barriers_before.len(), 1);
        assert_eq!(compiled.passes[1].name, "lighting");
        assert_eq!(compiled.passes[1].barriers_before.len(), 1);
    }

    #[test]
    fn explicit_cycles_are_rejected() {
        let mut graph = RenderGraph::new();
        let first = graph.add_pass("first");
        let second = graph.add_pass("second");
        graph.depends_on(first, second).unwrap();
        graph.depends_on(second, first).unwrap();
        assert_eq!(graph.compile(), Err(GraphError::Cycle));
    }
}
