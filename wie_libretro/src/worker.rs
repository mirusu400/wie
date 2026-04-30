//! Emulator worker thread.
//!
//! WIE's ARM interpreter + JVM call chain easily blows past Win64's 1MB
//! main-thread stack. RetroArch owns the host process, so we can't change
//! its EXE stack reserve from a cdylib. Instead we host the emulator on a
//! single dedicated worker thread with a 32MB stack and talk to it over
//! channels.
//!
//! Why not stacker::maybe_grow? It works for one-shot calls, but spinning
//! a fresh stack-grow thread every tick (60Hz) drowns the emulator in
//! context-switch overhead. A long-lived worker amortizes that cost to a
//! single thread spawn at load time.
//!
//! Why no log calls inside the worker? RetroArch's log callback is not
//! documented as thread-safe. Empirically, calling it from a non-main
//! thread under verbose logging would freeze the RA frontend after the
//! first frame. The worker exchanges info with the host thread purely
//! through channels and shared `Arc`s; logging happens on the main
//! thread once results are received.

use std::{
    error::Error,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, SyncSender, channel, sync_channel},
    },
    thread::{self, JoinHandle},
};

use wie_backend::{Emulator, Event, Options, Platform, extract_zip};
use wie_j2me::J2MEEmulator;
use wie_ktf::KtfEmulator;
use wie_lgt::LgtEmulator;
use wie_skt::SktEmulator;

use crate::{
    audio::RingBuffer,
    system::{LibretroPlatform, LibretroScreen},
};

const WORKER_STACK_BYTES: usize = 32 * 1024 * 1024;
/// Bound on inflight events from main → worker. 256 covers ~4 seconds of
/// 60Hz key chatter; if main blocks because the worker is wedged we'd
/// rather drop input than livelock.
const EVENT_CAPACITY: usize = 256;

pub enum WorkerCmd {
    Tick,
    Event(Event),
    Shutdown,
}

pub enum TickResult {
    Ok,
    Err(String),
    Panic(String),
}

pub struct EmulatorWorker {
    cmd_tx: SyncSender<WorkerCmd>,
    res_rx: Receiver<TickResult>,
    handle: Option<JoinHandle<()>>,
}

impl EmulatorWorker {
    pub fn spawn(
        path: String,
        data: Vec<u8>,
        screen: Arc<LibretroScreen>,
        audio_ring: Arc<Mutex<RingBuffer>>,
        data_root: Option<PathBuf>,
    ) -> Result<Self, Box<dyn Error>> {
        let (cmd_tx, cmd_rx) = sync_channel::<WorkerCmd>(EVENT_CAPACITY);
        let (res_tx, res_rx) = channel::<TickResult>();
        let (init_tx, init_rx) = channel::<Result<(), String>>();

        let handle = thread::Builder::new()
            .name("wie-emu".into())
            .stack_size(WORKER_STACK_BYTES)
            .spawn(move || {
                worker_main(path, data, screen, audio_ring, data_root, cmd_rx, res_tx, init_tx);
            })?;

        match init_rx.recv() {
            Ok(Ok(())) => Ok(EmulatorWorker { cmd_tx, res_rx, handle: Some(handle) }),
            Ok(Err(e)) => Err(format!("worker init failed: {e}").into()),
            Err(_) => Err("worker thread died during init".into()),
        }
    }

    /// Non-blocking event dispatch. Drops the event when the channel is
    /// saturated rather than stalling the main thread.
    pub fn send_event(&self, event: Event) {
        let _ = self.cmd_tx.try_send(WorkerCmd::Event(event));
    }

    /// Run one emulator step and wait for the result.
    pub fn tick(&self) -> TickResult {
        if self.cmd_tx.send(WorkerCmd::Tick).is_err() {
            return TickResult::Panic("worker channel closed".into());
        }
        match self.res_rx.recv() {
            Ok(msg) => msg,
            Err(_) => TickResult::Panic("worker dropped result channel".into()),
        }
    }
}

impl Drop for EmulatorWorker {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(WorkerCmd::Shutdown);
        if let Some(h) = self.handle.take() {
            // Best-effort join. If the worker is wedged we'd rather leak
            // the thread than block libretro shutdown.
            let _ = h.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn worker_main(
    path: String,
    data: Vec<u8>,
    screen: Arc<LibretroScreen>,
    audio_ring: Arc<Mutex<RingBuffer>>,
    data_root: Option<PathBuf>,
    cmd_rx: Receiver<WorkerCmd>,
    res_tx: Sender<TickResult>,
    init_tx: Sender<Result<(), String>>,
) {
    let platform: Box<dyn Platform> = Box::new(LibretroPlatform::new(screen, audio_ring, data_root));

    let build = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        build_emulator(path, data, platform)
    }));
    let mut emu: Box<dyn Emulator> = match build {
        Ok(Ok(emu)) => emu,
        Ok(Err(e)) => {
            let _ = init_tx.send(Err(format!("{e}")));
            return;
        }
        Err(_) => {
            let _ = init_tx.send(Err("panic during build_emulator".into()));
            return;
        }
    };
    let _ = init_tx.send(Ok(()));

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            WorkerCmd::Tick => {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| emu.tick()));
                let msg = match result {
                    Ok(Ok(())) => TickResult::Ok,
                    Ok(Err(e)) => TickResult::Err(format!("{e:?}")),
                    Err(_) => TickResult::Panic("tick panicked".into()),
                };
                let fatal = matches!(msg, TickResult::Panic(_));
                let _ = res_tx.send(msg);
                if fatal {
                    return;
                }
            }
            WorkerCmd::Event(event) => {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| emu.handle_event(event)));
            }
            WorkerCmd::Shutdown => return,
        }
    }
}

fn build_emulator(
    path: String,
    data: Vec<u8>,
    platform: Box<dyn Platform>,
) -> Result<Box<dyn Emulator>, Box<dyn Error>> {
    let options = Options { enable_gdbserver: false };
    let lower = path.to_ascii_lowercase();
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();

    if lower.ends_with(".zip") || lower.ends_with(".kjx") || lower.ends_with(".wie") {
        let files = extract_zip(&data).map_err(|e| format!("zip extract: {e:?}"))?;

        let emu: Box<dyn Emulator> = if KtfEmulator::loadable_archive(&files) {
            Box::new(KtfEmulator::from_archive(platform, files, options)?)
        } else if LgtEmulator::loadable_archive(&files) {
            Box::new(LgtEmulator::from_archive(platform, files, options)?)
        } else if SktEmulator::loadable_archive(&files) {
            Box::new(SktEmulator::from_archive(platform, files)?)
        } else {
            return Err("unknown zip archive format".into());
        };
        Ok(emu)
    } else if lower.ends_with(".jar") {
        let stem = filename.trim_end_matches(".jar").to_owned();

        let emu: Box<dyn Emulator> = if KtfEmulator::loadable_jar(&data) {
            Box::new(KtfEmulator::from_jar(platform, &filename, data, &stem, &stem, None, options)?)
        } else if LgtEmulator::loadable_jar(&data) {
            Box::new(LgtEmulator::from_jar(platform, &filename, data, &stem, &stem, None, options)?)
        } else if SktEmulator::loadable_jar(&data) {
            Box::new(SktEmulator::from_jar(platform, &filename, data, &stem, None)?)
        } else {
            Box::new(J2MEEmulator::from_jar(platform, &filename, data)?)
        };
        Ok(emu)
    } else if lower.ends_with(".jad") {
        Err(".jad alone is not supported by libretro frontend; pack jad+jar into a .zip".into())
    } else {
        Err(format!("unknown file extension for: {path}").into())
    }
}
