use alloc::string::String;

use anyhow::Context;
use elf::{endian::AnyEndian, ElfBytes};

use wie_backend::{App, Backend};
use wie_core_arm::{Allocator, ArmCore};

pub struct LgtApp {
    core: ArmCore,
    backend: Backend,
    entrypoint: u32,
    main_class_name: String,
}

impl LgtApp {
    pub fn new(main_class_name: &str, backend: &Backend) -> anyhow::Result<Self> {
        let mut core = ArmCore::new()?;

        Allocator::init(&mut core)?;

        let resource = backend.resource();
        let data = resource.data(resource.id("binary.mod").context("Resource not found")?);

        let entrypoint = Self::load(&mut core, data)?;

        let main_class_name = main_class_name.replace('.', "/");

        Ok(Self {
            core,
            backend: backend.clone(),
            entrypoint,
            main_class_name,
        })
    }

    #[tracing::instrument(name = "start", skip_all)]
    #[allow(unused_variables)]
    async fn do_start(core: &mut ArmCore, backend: &mut Backend, entrypoint: u32, main_class_name: String) -> anyhow::Result<()> {
        core.run_function(entrypoint + 1, &[]).await?;

        todo!()
    }

    fn load(core: &mut ArmCore, data: &[u8]) -> anyhow::Result<u32> {
        let elf = ElfBytes::<AnyEndian>::minimal_parse(data)?;

        anyhow::ensure!(elf.ehdr.e_machine == elf::abi::EM_ARM, "Invalid machine type");
        anyhow::ensure!(elf.ehdr.e_type == elf::abi::ET_EXEC, "Invalid file type");
        anyhow::ensure!(elf.ehdr.class == elf::file::Class::ELF32, "Invalid file type");
        anyhow::ensure!(elf.ehdr.e_phnum == 0, "Invalid file type");

        let (shdrs_opt, strtab_opt) = elf.section_headers_with_strtab()?;
        let (shdrs, strtab) = (
            shdrs_opt.ok_or(anyhow::anyhow!("Invalid file"))?,
            strtab_opt.ok_or(anyhow::anyhow!("Invalid file"))?,
        );

        for shdr in shdrs {
            let section_name = strtab.get(shdr.sh_name as usize)?;

            if shdr.sh_addr != 0 {
                tracing::debug!("Section {} at {:x}", section_name, shdr.sh_addr);

                let data = elf.section_data(&shdr)?.0;

                core.load(data, shdr.sh_addr as u32, shdr.sh_size as usize)?;
            }
        }

        tracing::debug!("Entrypoint: {:#x}", elf.ehdr.e_entry);

        Ok(elf.ehdr.e_entry as u32)
    }
}

impl App for LgtApp {
    fn start(&mut self) -> anyhow::Result<()> {
        let mut core = self.core.clone();
        let mut backend = self.backend.clone();

        let entrypoint = self.entrypoint;
        let main_class_name = self.main_class_name.clone();

        self.core
            .spawn(move || async move { Self::do_start(&mut core, &mut backend, entrypoint, main_class_name).await });

        Ok(())
    }

    fn crash_dump(&self) -> String {
        self.core.dump_reg_stack(0)
    }
}