//! Backend-neutral mesh upload helpers used by world/entity/GUI systems.

use renderer_api::core::RendererRuntime;
use renderer_api::rhi::device::RenderDevice;
use renderer_api::rhi::types::{
    BindGroupHandle, BufferDesc, BufferHandle, BufferUsage, MaterialHandle, PipelineHandle,
    RhiResult,
};

use super::MeshDraw;
use crate::render::entity::mesh::EntityVertex;
use crate::render::gui::GuiVertexBuilder;
use crate::world::mesh::ChunkMesh;

pub fn upload_mesh<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    pipeline: PipelineHandle,
    vertices: &[u8],
    indices: &[u32],
    label: &str,
) -> RhiResult<MeshDraw> {
    upload_mesh_with_bind_group(runtime, pipeline, None, vertices, indices, label)
}

pub fn upload_mesh_with_bind_group<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    pipeline: PipelineHandle,
    bind_group: Option<BindGroupHandle>,
    vertices: &[u8],
    indices: &[u32],
    label: &str,
) -> RhiResult<MeshDraw> {
    if vertices.is_empty() {
        return Err(renderer_api::rhi::types::RhiError::InvalidDescription(
            "mesh vertex data must be non-empty",
        ));
    }
    let vertex_buffer = runtime.create_buffer(BufferDesc::new(
        vertices.len() as u64,
        BufferUsage::VERTEX | BufferUsage::TRANSFER_DST,
        format!("{label}.vertices"),
    ))?;
    if let Err(error) = runtime.device().upload_buffer(vertex_buffer, 0, vertices) {
        let _ = runtime.destroy_buffer(vertex_buffer);
        return Err(error);
    }

    let index_buffer = if indices.is_empty() {
        None
    } else {
        let handle = match runtime.create_buffer(BufferDesc::new(
            (indices.len() * std::mem::size_of::<u32>()) as u64,
            BufferUsage::INDEX | BufferUsage::TRANSFER_DST,
            format!("{label}.indices"),
        )) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = runtime.destroy_buffer(vertex_buffer);
                return Err(error);
            }
        };
        if let Err(error) = runtime
            .device()
            .upload_buffer(handle, 0, bytemuck::cast_slice(indices))
        {
            let _ = runtime.destroy_buffer(vertex_buffer);
            let _ = runtime.destroy_buffer(handle);
            return Err(error);
        }
        Some(handle)
    };

    Ok(MeshDraw {
        pipeline,
        bind_group,
        vertex_buffer,
        index_buffer,
        material: None,
        push_constants: None,
        index_count: indices.len() as u32,
        first_index: 0,
        vertex_offset: 0,
        sort_key: 0,
    })
}

pub fn upload_gui_batch<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    pipeline: PipelineHandle,
    bind_group: Option<BindGroupHandle>,
    builder: &GuiVertexBuilder,
    label: &str,
) -> RhiResult<Option<MeshDraw>> {
    if builder.vertices.is_empty() || builder.indices.is_empty() {
        return Ok(None);
    }
    upload_mesh_with_bind_group(
        runtime,
        pipeline,
        bind_group,
        bytemuck::cast_slice(&builder.vertices),
        &builder.indices,
        label,
    )
    .map(Some)
}

pub fn upload_chunk_mesh<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    pipeline: PipelineHandle,
    bind_group: Option<BindGroupHandle>,
    mesh: &ChunkMesh,
    label: &str,
) -> RhiResult<Option<MeshDraw>> {
    if mesh.is_empty() {
        return Ok(None);
    }
    upload_mesh_with_bind_group(
        runtime,
        pipeline,
        bind_group,
        bytemuck::cast_slice(&mesh.vertices),
        &mesh.indices,
        label,
    )
    .map(Some)
}

pub fn upload_entity_mesh<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    pipeline: PipelineHandle,
    bind_group: Option<BindGroupHandle>,
    vertices: &[EntityVertex],
    indices: &[u32],
    label: &str,
) -> RhiResult<Option<MeshDraw>> {
    if vertices.is_empty() || indices.is_empty() {
        return Ok(None);
    }
    upload_mesh_with_bind_group(
        runtime,
        pipeline,
        bind_group,
        bytemuck::cast_slice(vertices),
        indices,
        label,
    )
    .map(Some)
}

/// Update a frequently animated entity mesh without allocating a new Vulkan
/// buffer on every redraw. The existing buffers are reused while they have
/// enough capacity; only a capacity growth replaces them.
pub fn update_entity_mesh<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    draw: &mut Option<MeshDraw>,
    pipeline: PipelineHandle,
    bind_group: Option<BindGroupHandle>,
    material: Option<MaterialHandle>,
    vertices: &[EntityVertex],
    indices: &[u32],
    label: &str,
) -> RhiResult<()> {
    if vertices.is_empty() || indices.is_empty() {
        if let Some(old) = draw.take() {
            destroy_mesh(runtime, old)?;
        }
        return Ok(());
    }

    let old = *draw;
    let vertex_size = std::mem::size_of_val(vertices) as u64;
    let vertex_reusable = old.as_ref().is_some_and(|old| {
        runtime
            .resources()
            .buffer(old.vertex_buffer)
            .is_some_and(|desc| desc.size >= vertex_size)
    });
    let vertex_buffer = if vertex_reusable {
        old.expect("reusable mesh must have an old draw")
            .vertex_buffer
    } else {
        runtime.create_buffer(BufferDesc::new(
            vertex_size,
            BufferUsage::VERTEX | BufferUsage::TRANSFER_DST,
            format!("{label}.vertices"),
        ))?
    };
    if let Err(error) =
        runtime
            .device()
            .upload_buffer(vertex_buffer, 0, bytemuck::cast_slice(vertices))
    {
        if !vertex_reusable {
            let _ = runtime.destroy_buffer(vertex_buffer);
        }
        return Err(error);
    }

    let index_size = (indices.len() * std::mem::size_of::<u32>()) as u64;
    let index_reusable = old
        .as_ref()
        .and_then(|old| old.index_buffer)
        .and_then(|buffer| {
            runtime
                .resources()
                .buffer(buffer)
                .map(|desc| (buffer, desc.size))
        })
        .is_some_and(|(_, capacity)| capacity >= index_size);
    let index_buffer = if index_reusable {
        old.and_then(|old| old.index_buffer)
    } else {
        match runtime.create_buffer(BufferDesc::new(
            index_size,
            BufferUsage::INDEX | BufferUsage::TRANSFER_DST,
            format!("{label}.indices"),
        )) {
            Ok(handle) => Some(handle),
            Err(error) => {
                if !vertex_reusable {
                    let _ = runtime.destroy_buffer(vertex_buffer);
                }
                return Err(error);
            }
        }
    };
    let Some(index_buffer_handle) = index_buffer else {
        if !vertex_reusable {
            let _ = runtime.destroy_buffer(vertex_buffer);
        }
        return Err(renderer_api::rhi::types::RhiError::InvalidDescription(
            "animated entity mesh requires indices",
        ));
    };
    if let Err(error) =
        runtime
            .device()
            .upload_buffer(index_buffer_handle, 0, bytemuck::cast_slice(indices))
    {
        if !index_reusable {
            let _ = runtime.destroy_buffer(index_buffer_handle);
        }
        if !vertex_reusable {
            let _ = runtime.destroy_buffer(vertex_buffer);
        }
        return Err(error);
    }

    *draw = Some(MeshDraw {
        pipeline,
        bind_group,
        vertex_buffer,
        index_buffer: Some(index_buffer_handle),
        material,
        push_constants: None,
        index_count: indices.len() as u32,
        first_index: 0,
        vertex_offset: 0,
        sort_key: old.map_or(0, |old| old.sort_key),
    });

    if let Some(old) = old {
        if old.vertex_buffer != vertex_buffer {
            let _ = runtime.destroy_buffer(old.vertex_buffer);
        }
        if old.index_buffer != Some(index_buffer_handle) {
            if let Some(old_index) = old.index_buffer {
                let _ = runtime.destroy_buffer(old_index);
            }
        }
    }
    Ok(())
}

pub fn destroy_mesh<D: RenderDevice>(
    runtime: &mut RendererRuntime<D>,
    mesh: MeshDraw,
) -> RhiResult<()> {
    runtime.destroy_buffer(mesh.vertex_buffer)?;
    if let Some(index_buffer) = mesh.index_buffer {
        runtime.destroy_buffer(index_buffer)?;
    }
    Ok(())
}

pub fn buffer_handles(mesh: MeshDraw) -> (BufferHandle, Option<BufferHandle>) {
    (mesh.vertex_buffer, mesh.index_buffer)
}
