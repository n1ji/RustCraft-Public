use super::types::{TextureDesc, TextureHandle};

#[derive(Clone, Debug)]
pub struct Texture {
    handle: TextureHandle,
    desc: TextureDesc,
}

impl Texture {
    pub(crate) fn new(handle: TextureHandle, desc: TextureDesc) -> Self {
        Self { handle, desc }
    }

    pub const fn handle(&self) -> TextureHandle {
        self.handle
    }

    pub fn desc(&self) -> &TextureDesc {
        &self.desc
    }
}
