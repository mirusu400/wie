mod audio;
mod database;
mod filesystem;
mod input;
mod system;
mod worker;

use std::{
    error::Error,
    ffi::{CStr, CString},
    os::raw::c_void,
    path::PathBuf,
    ptr, slice,
    sync::{Arc, Mutex},
};

use rust_libretro::{
    contexts::*,
    core::{Core, CoreOptions},
    env_version, retro_core,
    sys::{
        RETRO_ENVIRONMENT_GET_GAME_INFO_EXT, retro_game_geometry, retro_game_info,
        retro_game_info_ext, retro_system_av_info, retro_system_timing,
    },
    types::*,
};

use wie_backend::Event;

use crate::{
    audio::RingBuffer,
    input::{InputDelta, InputTracker},
    system::LibretroScreen,
    worker::{EmulatorWorker, TickResult},
};

const CORE_NAME: &str = "wie";
const DEFAULT_WIDTH: u32 = 240;
const DEFAULT_HEIGHT: u32 = 320;
const MAX_WIDTH: u32 = 480;
const MAX_HEIGHT: u32 = 640;
const FPS: f64 = 60.0;
const SAMPLE_RATE: f64 = 44_100.0;

/// rust-libretro 0.3.2 + bindgen 0.63 generates `retro_game_info` as an opaque
/// 1-byte struct (`_address: u8`), so the by-value `Some(*game)` in the wrapper
/// loses every field. Workaround: ignore `info` and query
/// `RETRO_ENVIRONMENT_GET_GAME_INFO_EXT` instead — `retro_game_info_ext` is
/// generated correctly and exposes path + data + size.
fn fetch_game_info_ext(env_cb: rust_libretro::sys::retro_environment_t) -> Result<(String, Vec<u8>), Box<dyn Error>> {
    let cb = env_cb.ok_or("no environment callback")?;
    let mut info_ext_ptr: *const retro_game_info_ext = ptr::null();
    let ok = unsafe {
        cb(
            RETRO_ENVIRONMENT_GET_GAME_INFO_EXT,
            &mut info_ext_ptr as *mut _ as *mut c_void,
        )
    };
    if !ok || info_ext_ptr.is_null() {
        return Err("GET_GAME_INFO_EXT not supported by frontend".into());
    }
    // SAFETY: RA guarantees the pointer is valid for the duration of retro_load_game.
    let info_ext = unsafe { &*info_ext_ptr };

    let path = if info_ext.full_path.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(info_ext.full_path) }.to_str()?.to_owned()
    };

    let data = if info_ext.data.is_null() || info_ext.size == 0 {
        if path.is_empty() {
            return Err("game_info_ext has neither data nor path".into());
        }
        std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?
    } else {
        // SAFETY: data + size valid for retro_load_game lifetime.
        unsafe { slice::from_raw_parts(info_ext.data as *const u8, info_ext.size) }.to_vec()
    };

    Ok((path, data))
}

struct WieLibretroCore {
    width: u32,
    height: u32,
    geometry_counter: u64,
    /// Last paint counter we forwarded to the frontend. Frames where the
    /// emulator hasn't repainted are skipped, letting RA dup the last
    /// frame and removing the spurious-black flicker from intermediate
    /// retro_run calls.
    last_frame_counter: u64,
    /// `<RA system>/wie/`. None until on_init populates it.
    data_root: Option<PathBuf>,
    screen: Option<Arc<LibretroScreen>>,
    audio_ring: Option<Arc<Mutex<RingBuffer>>>,
    worker: Option<EmulatorWorker>,
    input: InputTracker,
}

impl WieLibretroCore {
    fn new() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            geometry_counter: 0,
            last_frame_counter: 0,
            data_root: None,
            screen: None,
            audio_ring: None,
            worker: None,
            input: InputTracker::default(),
        }
    }

}

impl CoreOptions for WieLibretroCore {}

impl Core for WieLibretroCore {
    fn get_info(&self) -> SystemInfo {
        SystemInfo {
            library_name: CString::new(CORE_NAME).unwrap(),
            library_version: CString::new(env_version!("CARGO_PKG_VERSION").to_string()).unwrap(),
            valid_extensions: CString::new("jar|zip|kjx|wie").unwrap(),
            need_fullpath: true,
            block_extract: true,
        }
    }

    fn on_get_av_info(&mut self, _ctx: &mut GetAvInfoContext) -> retro_system_av_info {
        retro_system_av_info {
            geometry: retro_game_geometry {
                base_width: self.width,
                base_height: self.height,
                max_width: MAX_WIDTH,
                max_height: MAX_HEIGHT,
                aspect_ratio: 0.0,
            },
            timing: retro_system_timing {
                fps: FPS,
                sample_rate: SAMPLE_RATE,
            },
        }
    }

    fn on_set_environment(&mut self, initial: bool, ctx: &mut SetEnvironmentContext) {
        if initial {
            ctx.set_support_no_game(false);
        }
    }

    fn on_init(&mut self, ctx: &mut InitContext) {
        log::info!("wie_libretro init (version {})", env!("CARGO_PKG_VERSION"));

        let gctx: GenericContext = (&mut *ctx).into();
        // SAFETY: env callback valid throughout the core's lifetime.
        let env_cb = unsafe { *gctx.environment_callback() };
        if let Some(sys_path) = unsafe { rust_libretro::environment::get_system_directory(env_cb) } {
            let root = sys_path.join("wie");
            log::info!("system directory: {sys_path:?}, data root: {root:?}");
            self.data_root = Some(root);
        } else {
            log::warn!("frontend did not provide GET_SYSTEM_DIRECTORY; falling back to ProjectDirs");
        }
    }

    fn on_deinit(&mut self, _ctx: &mut DeinitContext) {
        log::info!("wie_libretro deinit");
    }

    fn on_load_game(
        &mut self,
        _info: Option<retro_game_info>,
        ctx: &mut LoadGameContext,
    ) -> Result<(), Box<dyn Error>> {
        log::info!("on_load_game entry");
        if !ctx.set_pixel_format(PixelFormat::XRGB8888) {
            return Err("XRGB8888 not supported".into());
        }

        let gctx: GenericContext = (&mut *ctx).into();
        // SAFETY: env callback is valid for the duration of retro_load_game.
        let env_cb = unsafe { *gctx.environment_callback() };
        let (path, data) = match fetch_game_info_ext(env_cb) {
            Ok(v) => v,
            Err(e) => {
                log::error!("fetch_game_info_ext failed: {e}");
                return Err(e);
            }
        };

        let screen = Arc::new(LibretroScreen::new(self.width, self.height));
        self.screen = Some(Arc::clone(&screen));
        let audio_ring = Arc::new(Mutex::new(RingBuffer::new()));
        self.audio_ring = Some(Arc::clone(&audio_ring));

        match EmulatorWorker::spawn(path, data, screen, audio_ring, self.data_root.clone()) {
            Ok(worker) => {
                log::info!("emulator worker spawned");
                self.worker = Some(worker);
                self.last_frame_counter = 0;
                Ok(())
            }
            Err(e) => {
                log::error!("on_load_game failed: {e}");
                self.screen = None;
                self.audio_ring = None;
                Err(e)
            }
        }
    }

    fn on_unload_game(&mut self, _ctx: &mut UnloadGameContext) {
        log::info!("wie_libretro on_unload_game");
        self.worker = None; // Drop joins the worker thread.
        self.screen = None;
        self.audio_ring = None;
    }

    fn on_run(&mut self, ctx: &mut RunContext, _delta_us: Option<i64>) {
        if let Some(worker) = self.worker.as_ref() {
            ctx.poll_input();
            // SAFETY: rust-libretro handles a null input callback internally.
            let pad = unsafe { ctx.get_joypad_bitmask(0, 0) };
            for delta in self.input.diff(pad) {
                match delta {
                    InputDelta::Pressed(k) => worker.send_event(Event::Keydown(k)),
                    InputDelta::Released(k) => worker.send_event(Event::Keyup(k)),
                }
            }

            match worker.tick() {
                TickResult::Ok => {}
                TickResult::Err(e) => {
                    log::error!("tick error: {e}");
                    self.worker = None;
                    return;
                }
                TickResult::Panic(e) => {
                    log::error!("tick panicked: {e}");
                    self.worker = None;
                    return;
                }
            }
            worker.send_event(Event::Redraw);
        }

        if let Some(screen) = self.screen.as_ref() {
            if let Some((w, h, fb, frame_counter, geom)) = screen.snapshot() {
                if geom != self.geometry_counter {
                    self.geometry_counter = geom;
                    self.width = w;
                    self.height = h;
                    log::info!("geometry changed: {w}x{h}");
                    let new_geom = retro_game_geometry {
                        base_width: w,
                        base_height: h,
                        max_width: MAX_WIDTH,
                        max_height: MAX_HEIGHT,
                        aspect_ratio: 0.0,
                    };
                    let _ = ctx.set_game_geometry(new_geom);
                }
                self.last_frame_counter = frame_counter;
                // SAFETY: Vec<u32> is contiguous; reinterpret as u8 with 4× length.
                let bytes: &[u8] = unsafe { slice::from_raw_parts(fb.as_ptr() as *const u8, fb.len() * 4) };
                ctx.draw_frame(bytes, w, h, (w * 4) as usize);
            }
        }

        if let Some(ring) = self.audio_ring.as_ref() {
            let stereo = if let Ok(mut r) = ring.lock() { r.drain_stereo(2048) } else { Vec::new() };
            if !stereo.is_empty() {
                let actx: AudioContext = (&mut *ctx).into();
                actx.batch_audio_samples(&stereo);
            }
        }
    }

    fn on_reset(&mut self, _ctx: &mut ResetContext) {
        log::info!("wie_libretro on_reset (not yet implemented)");
    }
}

retro_core!(WieLibretroCore::new());
