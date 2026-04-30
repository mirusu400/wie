use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use wie_backend::{
    AudioSink, DatabaseRepository, Filesystem, Instant, Platform, Screen,
    canvas::Image,
};

use crate::{
    audio::{LibretroAudioSink, RingBuffer},
    database::LibretroDatabaseRepository,
    filesystem::LibretroFilesystem,
};
use wie_util::Result as WieResult;

pub struct LibretroPlatform {
    screen: Arc<LibretroScreen>,
    filesystem: LibretroFilesystem,
    database_repository: LibretroDatabaseRepository,
    audio_ring: Arc<Mutex<RingBuffer>>,
}

impl LibretroPlatform {
    pub fn new(screen: Arc<LibretroScreen>, audio_ring: Arc<Mutex<RingBuffer>>, data_root: Option<PathBuf>) -> Self {
        let (filesystem, database_repository) = match data_root {
            Some(root) => (
                LibretroFilesystem::with_base(root.clone()),
                LibretroDatabaseRepository::with_base(root),
            ),
            None => (
                LibretroFilesystem::project_dirs_fallback(),
                LibretroDatabaseRepository::project_dirs_fallback(),
            ),
        };
        Self {
            screen,
            filesystem,
            database_repository,
            audio_ring,
        }
    }
}

impl Platform for LibretroPlatform {
    fn screen(&self) -> &dyn Screen {
        self.screen.as_ref()
    }

    fn now(&self) -> Instant {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Instant::from_epoch_millis(ms)
    }

    fn database_repository(&self) -> &dyn DatabaseRepository {
        &self.database_repository
    }

    fn filesystem(&self) -> &dyn Filesystem {
        &self.filesystem
    }

    fn audio_sink(&self) -> Box<dyn AudioSink> {
        Box::new(LibretroAudioSink::new(Arc::clone(&self.audio_ring)))
    }

    fn write_stdout(&self, buf: &[u8]) {
        if let Ok(s) = std::str::from_utf8(buf) {
            log::info!("{}", s.trim_end());
        }
    }

    fn write_stderr(&self, buf: &[u8]) {
        if let Ok(s) = std::str::from_utf8(buf) {
            log::warn!("{}", s.trim_end());
        }
    }

    fn exit(&self) {
        log::info!("emulator requested exit");
    }

    fn vibrate(&self, duration_ms: u64, intensity: u8) {
        log::debug!("vibrate({duration_ms}ms, {intensity}%) — unsupported");
    }
}

struct ScreenState {
    width: u32,
    height: u32,
    framebuffer: Vec<u32>,
    /// Bumped on every paint so the host can detect new frames.
    frame_counter: u64,
    /// Bumped on resize so the host can call SET_GEOMETRY.
    geometry_counter: u64,
}

pub struct LibretroScreen {
    state: Mutex<ScreenState>,
}

impl LibretroScreen {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            state: Mutex::new(ScreenState {
                width,
                height,
                framebuffer: vec![0u32; (width * height) as usize],
                frame_counter: 0,
                geometry_counter: 0,
            }),
        }
    }

    /// Returns (width, height, fb_clone, frame_counter, geometry_counter).
    pub fn snapshot(&self) -> Option<(u32, u32, Vec<u32>, u64, u64)> {
        let s = self.state.lock().ok()?;
        Some((s.width, s.height, s.framebuffer.clone(), s.frame_counter, s.geometry_counter))
    }
}

impl Screen for LibretroScreen {
    fn request_redraw(&self) -> WieResult<()> {
        Ok(())
    }

    fn paint(&self, image: &dyn Image) {
        let img_w = image.width();
        let img_h = image.height();
        // XRGB8888 — alpha bits are reserved/ignored. Force them to 0xFF so
        // any frontend post-processing path that interprets them as alpha
        // (instead of treating the byte as undefined) doesn't paint our
        // pixels black.
        let buf: Vec<u32> = image
            .colors()
            .iter()
            .map(|c| 0xFF00_0000 | ((c.r as u32) << 16) | ((c.g as u32) << 8) | (c.b as u32))
            .collect();

        // WIE games run a double-buffer cycle: a clear pass paints a near-
        // all-black frame, the game then draws over it and paints again.
        // Forwarding the clear pass causes a 1-frame black flicker on the
        // RA side, since our cadence often catches it. Drop frames where
        // a 1024-pixel sample is ≥95% black; the next genuine paint will
        // overwrite the framebuffer milliseconds later anyway.
        if !buf.is_empty() {
            let sample_n = buf.len().min(1024);
            let blackish = buf.iter().take(sample_n).filter(|&&p| (p & 0x00FF_FFFF) < 0x10_1010).count();
            if blackish * 100 / sample_n >= 95 {
                return;
            }
        }

        if let Ok(mut s) = self.state.lock() {
            if s.width != img_w || s.height != img_h {
                s.width = img_w;
                s.height = img_h;
                s.geometry_counter += 1;
            }
            s.framebuffer = buf;
            s.frame_counter += 1;
        }
    }

    fn width(&self) -> u32 {
        self.state.lock().map(|s| s.width).unwrap_or(0)
    }

    fn height(&self) -> u32 {
        self.state.lock().map(|s| s.height).unwrap_or(0)
    }
}

