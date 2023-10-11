use alloc::string::{String, ToString};

use wie_backend::{App, Backend};
use wie_core_jvm::JvmCore;

pub struct SktWipiApp {
    core: JvmCore,
    backend: Backend,
    main_class_name: String,
}

impl SktWipiApp {
    pub fn new(main_class_name: &str, backend: &Backend) -> anyhow::Result<Self> {
        let core = JvmCore::new();

        Ok(Self {
            core,
            backend: backend.clone(),
            main_class_name: main_class_name.to_string(),
        })
    }

    #[tracing::instrument(name = "start", skip_all)]
    #[allow(unused_variables)]
    async fn do_start(core: &mut JvmCore, backend: &mut Backend, main_class_name: String) -> anyhow::Result<()> {
        let main_class = core.load_class(backend, &main_class_name)?;

        todo!()
    }
}

impl App for SktWipiApp {
    fn start(&mut self) -> anyhow::Result<()> {
        let mut core = self.core.clone();
        let mut backend = self.backend.clone();

        let main_class_name = self.main_class_name.clone();

        self.core
            .spawn(move || async move { Self::do_start(&mut core, &mut backend, main_class_name).await });

        Ok(())
    }

    fn crash_dump(&self) -> String {
        todo!()
    }
}