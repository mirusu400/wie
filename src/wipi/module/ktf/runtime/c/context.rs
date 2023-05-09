use crate::{
    backend::Backend,
    core::arm::{allocator::Allocator, ArmCore},
    util::{ByteRead, ByteWrite},
    wipi::c::{CContextBase, CContextMethod, CResult},
};

pub struct KtfCContext {
    core: ArmCore,
    backend: Backend,
}

impl KtfCContext {
    pub fn new(core: ArmCore, backend: Backend) -> Self {
        Self { core, backend }
    }
}

impl CContextBase for KtfCContext {
    fn alloc(&mut self, size: u32) -> CResult<u32> {
        Allocator::alloc(&mut self.core, size)
    }

    fn register_function(&mut self, method: CContextMethod) -> CResult<u32> {
        self.core.register_function(
            move |core: ArmCore, backend: Backend, a0: u32, a1: u32, a2: u32| {
                let mut context = KtfCContext::new(core, backend);

                let result = method(&mut context, vec![a0, a1, a2])?;

                Ok::<_, anyhow::Error>(result)
            },
            &self.backend,
        )
    }
    fn backend(&mut self) -> &mut Backend {
        &mut self.backend
    }
}

impl ByteRead for KtfCContext {
    fn read_bytes(&self, address: u32, size: u32) -> anyhow::Result<Vec<u8>> {
        self.core.read_bytes(address, size)
    }
}

impl ByteWrite for KtfCContext {
    fn write_bytes(&mut self, address: u32, data: &[u8]) -> anyhow::Result<()> {
        self.core.write_bytes(address, data)
    }
}