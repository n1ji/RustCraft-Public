use super::types::{
    PipelineDesc, PipelineHandle, SamplerDesc, SamplerHandle, ShaderDesc, ShaderHandle,
};

#[derive(Clone, Debug)]
pub struct Shader {
    handle: ShaderHandle,
    desc: ShaderDesc,
}

impl Shader {
    pub(crate) fn new(handle: ShaderHandle, desc: ShaderDesc) -> Self {
        Self { handle, desc }
    }

    pub const fn handle(&self) -> ShaderHandle {
        self.handle
    }

    pub fn desc(&self) -> &ShaderDesc {
        &self.desc
    }
}

#[derive(Clone, Debug)]
pub struct Pipeline {
    handle: PipelineHandle,
    desc: PipelineDesc,
}

impl Pipeline {
    pub(crate) fn new(handle: PipelineHandle, desc: PipelineDesc) -> Self {
        Self { handle, desc }
    }

    pub const fn handle(&self) -> PipelineHandle {
        self.handle
    }

    pub fn desc(&self) -> &PipelineDesc {
        &self.desc
    }
}

#[derive(Clone, Debug)]
pub struct Sampler {
    handle: SamplerHandle,
    desc: SamplerDesc,
}

impl Sampler {
    pub(crate) fn new(handle: SamplerHandle, desc: SamplerDesc) -> Self {
        Self { handle, desc }
    }

    pub const fn handle(&self) -> SamplerHandle {
        self.handle
    }

    pub fn desc(&self) -> &SamplerDesc {
        &self.desc
    }
}
