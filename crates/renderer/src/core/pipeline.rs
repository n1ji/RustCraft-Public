use std::collections::HashMap;

use crate::rhi::types::{PipelineDesc, PipelineHandle, RhiError, RhiResult};

#[derive(Debug, Default)]
pub struct PipelineManager {
    descriptors: HashMap<PipelineHandle, PipelineDesc>,
}

impl PipelineManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, handle: PipelineHandle, desc: PipelineDesc) -> RhiResult<()> {
        if !handle.is_valid() {
            return Err(RhiError::InvalidHandle);
        }
        self.descriptors.insert(handle, desc);
        Ok(())
    }

    pub fn unregister(&mut self, handle: PipelineHandle) -> Option<PipelineDesc> {
        self.descriptors.remove(&handle)
    }

    pub fn get(&self, handle: PipelineHandle) -> Option<&PipelineDesc> {
        self.descriptors.get(&handle)
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
}
