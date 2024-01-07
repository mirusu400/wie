use alloc::boxed::Box;

use java_runtime_base::MethodBody;

use jvm::JvmResult;
use wie_backend::{AsyncCallable, SystemHandle};
use wie_core_arm::ArmCore;
use wie_impl_java::WieContextBase;

use crate::runtime::java::jvm::KtfJvm;

#[derive(Clone)]
pub struct KtfWieContext {
    core: ArmCore,
    system: SystemHandle,
}

impl KtfWieContext {
    pub fn new(core: &ArmCore, system: &SystemHandle) -> Self {
        Self {
            core: core.clone(),
            system: system.clone(),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl WieContextBase for KtfWieContext {
    fn system(&mut self) -> &mut SystemHandle {
        &mut self.system
    }

    fn spawn(&mut self, callback: Box<dyn MethodBody<anyhow::Error, dyn WieContextBase>>) -> JvmResult<()> {
        struct SpawnProxy {
            core: ArmCore,
            system: SystemHandle,
            callback: Box<dyn MethodBody<anyhow::Error, dyn WieContextBase>>,
        }

        #[async_trait::async_trait(?Send)]
        impl AsyncCallable<u32, anyhow::Error> for SpawnProxy {
            async fn call(mut self) -> Result<u32, anyhow::Error> {
                let mut context = KtfWieContext::new(&self.core, &self.system);
                let mut jvm = KtfJvm::new(&self.core, &self.system).jvm();

                let _ = self.callback.call(&mut jvm, &mut context, Box::new([])).await?;

                Ok(0) // TODO resturn value
            }
        }

        let system = self.system.clone();

        self.core.spawn(SpawnProxy {
            core: self.core.clone(),
            system,
            callback,
        });

        Ok(())
    }
}
