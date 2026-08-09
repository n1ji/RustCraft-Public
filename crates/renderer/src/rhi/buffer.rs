use super::types::{BufferDesc, BufferHandle};

#[derive(Clone, Debug)]
pub struct Buffer {
    handle: BufferHandle,
    desc: BufferDesc,
}

impl Buffer {
    pub(crate) fn new(handle: BufferHandle, desc: BufferDesc) -> Self {
        Self { handle, desc }
    }

    pub const fn handle(&self) -> BufferHandle {
        self.handle
    }

    pub fn desc(&self) -> &BufferDesc {
        &self.desc
    }
}
