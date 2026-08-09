use crate::rhi::types::{
    BindGroupHandle, BufferHandle, MaterialHandle, PipelineHandle, RhiError, RhiResult,
    TextureHandle,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterialDesc {
    pub pipeline: PipelineHandle,
    /// Descriptor sets owned by this material, indexed by their RHI set number.
    /// The backend remains responsible for translating them into native
    /// descriptor objects; the core only keeps the binding relationship.
    pub bind_groups: Vec<BindGroupHandle>,
    pub textures: Vec<TextureHandle>,
    pub uniform_buffer: Option<BufferHandle>,
    /// Optional pipeline push constants applied before the draw packet.
    pub push_constants: Vec<u8>,
}

#[derive(Debug)]
struct MaterialSlot {
    generation: u32,
    value: Option<MaterialDesc>,
}

#[derive(Debug, Default)]
pub struct MaterialManager {
    slots: Vec<MaterialSlot>,
    free: Vec<u32>,
}

impl MaterialManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, desc: MaterialDesc) -> RhiResult<MaterialHandle> {
        if !desc.pipeline.is_valid() {
            return Err(RhiError::InvalidDescription(
                "material must reference a valid pipeline",
            ));
        }

        let (index, generation) = if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1).max(1);
            slot.value = Some(desc);
            (index, slot.generation)
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(MaterialSlot {
                generation: 1,
                value: Some(desc),
            });
            (index, 1)
        };

        Ok(MaterialHandle::new(index, generation))
    }

    pub fn create_for_resources(
        &mut self,
        resources: &super::resource::ResourceManager,
        desc: MaterialDesc,
    ) -> RhiResult<MaterialHandle> {
        if resources.pipeline(desc.pipeline).is_none()
            || desc
                .bind_groups
                .iter()
                .any(|handle| resources.bind_group(*handle).is_none())
            || desc
                .textures
                .iter()
                .any(|handle| resources.texture(*handle).is_none())
            || desc
                .uniform_buffer
                .is_some_and(|handle| resources.buffer(handle).is_none())
        {
            return Err(RhiError::InvalidHandle);
        }
        self.create(desc)
    }

    pub fn destroy(&mut self, handle: MaterialHandle) -> RhiResult<MaterialDesc> {
        let slot = self
            .slots
            .get_mut(handle.index() as usize)
            .filter(|slot| slot.generation == handle.generation());
        let slot = slot.ok_or(RhiError::InvalidHandle)?;
        let value = slot.value.take().ok_or(RhiError::InvalidHandle)?;
        self.free.push(handle.index());
        Ok(value)
    }

    pub fn get(&self, handle: MaterialHandle) -> Option<&MaterialDesc> {
        let slot = self.slots.get(handle.index() as usize)?;
        (slot.generation == handle.generation()).then_some(slot.value.as_ref()?)
    }

    pub fn references_pipeline(&self, pipeline: PipelineHandle) -> bool {
        self.slots
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|material| material.pipeline == pipeline)
    }

    pub fn references_texture(&self, texture: TextureHandle) -> bool {
        self.slots
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|material| material.textures.contains(&texture))
    }

    pub fn references_buffer(&self, buffer: BufferHandle) -> bool {
        self.slots
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|material| material.uniform_buffer == Some(buffer))
    }

    pub fn references_bind_group(&self, bind_group: BindGroupHandle) -> bool {
        self.slots
            .iter()
            .filter_map(|slot| slot.value.as_ref())
            .any(|material| material.bind_groups.contains(&bind_group))
    }

    pub fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    }
}
