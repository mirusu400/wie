use alloc::{boxed::Box, collections::BTreeMap, format, string::String, vec::Vec};

use anyhow::Context;

use wie_backend::{extract_zip, App, Archive, Backend};

pub struct LgtArchive {
    jar: Vec<u8>,
    main_class_name: String,
}

impl LgtArchive {
    pub fn is_lgt_archive(files: &BTreeMap<String, Vec<u8>>) -> bool {
        files.contains_key("app_info")
    }

    pub fn from_zip(mut files: BTreeMap<String, Vec<u8>>) -> anyhow::Result<Self> {
        let app_info = files.get("app_info").context("Invalid format")?;
        let app_info = LgtAppInfo::parse(app_info);

        tracing::info!("Loading app {}, mclass {}", app_info.aid, app_info.mclass);

        let jar = files.remove(&format!("{}.jar", app_info.aid)).context("Invalid format")?;

        Ok(Self::from_jar(jar, &app_info.mclass))
    }

    pub fn from_jar(data: Vec<u8>, main_class_name: &str) -> Self {
        Self {
            jar: data,
            main_class_name: main_class_name.into(),
        }
    }
}

impl Archive for LgtArchive {
    fn load_app(&self, backend: &mut Backend) -> anyhow::Result<Box<dyn App>> {
        let jar_data = extract_zip(&self.jar)?;

        for (filename, data) in jar_data {
            backend.add_resource(&filename, data);
        }

        todo!("load app {}", self.main_class_name)
    }
}

// almost similar to KtfAdf.. can we merge these?
struct LgtAppInfo {
    aid: String,
    mclass: String,
}

impl LgtAppInfo {
    pub fn parse(data: &[u8]) -> Self {
        let mut aid = String::new();
        let mut mclass = String::new();

        let mut lines = data.split(|x| *x == b'\n');

        for line in &mut lines {
            if line.starts_with(b"AID:") {
                aid = String::from_utf8_lossy(&line[4..]).into();
            } else if line.starts_with(b"MClass:") {
                mclass = String::from_utf8_lossy(&line[7..]).into();
            }
            // TODO load name, it's in euc-kr..
        }

        Self { aid, mclass }
    }
}