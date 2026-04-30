//! PCM audio ring buffer. WIE pushes samples via [`AudioSink::play_wave`];
//! libretro pulls them in batches at the end of every retro_run.
//!
//! Stereo only. Mono input is duplicated to stereo on the way in. Sample-rate
//! mismatch between successive `play_wave` calls is logged and the new rate
//! ignored — RA's `SET_SYSTEM_AV_INFO` would have to be called and that's
//! disruptive.
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use wie_backend::AudioSink;

const FALLBACK_SAMPLE_RATE: u32 = 44_100;
const MAX_QUEUED_FRAMES: usize = 44_100 * 2; // 1 second of stereo @ 44.1kHz

#[derive(Default)]
pub struct RingBuffer {
    /// Interleaved L,R,L,R stereo i16 samples ready for libretro batch output.
    samples: VecDeque<i16>,
    /// First-seen sample rate. None until first play_wave.
    sample_rate: Option<u32>,
}

impl RingBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.unwrap_or(FALLBACK_SAMPLE_RATE)
    }

    /// Drain up to `max_frames` stereo frames (each frame = 2 i16 samples).
    pub fn drain_stereo(&mut self, max_frames: usize) -> Vec<i16> {
        let max_samples = max_frames * 2;
        let n = max_samples.min(self.samples.len());
        self.samples.drain(..n).collect()
    }

    fn push_stereo(&mut self, channels: u8, samples: &[i16]) {
        let frames = if channels == 0 { samples.len() } else { samples.len() / channels as usize };
        for f in 0..frames {
            let (l, r) = match channels {
                1 => {
                    let s = samples[f];
                    (s, s)
                }
                2 => (samples[f * 2], samples[f * 2 + 1]),
                _ => return, // unsupported layout — drop silently
            };
            self.samples.push_back(l);
            self.samples.push_back(r);
        }
        // Cap to avoid unbounded growth if the host falls behind.
        while self.samples.len() > MAX_QUEUED_FRAMES * 2 {
            self.samples.pop_front();
        }
    }
}

pub struct LibretroAudioSink {
    ring: Arc<Mutex<RingBuffer>>,
}

impl LibretroAudioSink {
    pub fn new(ring: Arc<Mutex<RingBuffer>>) -> Self {
        Self { ring }
    }
}

impl AudioSink for LibretroAudioSink {
    fn play_wave(&self, channel: u8, sampling_rate: u32, wave_data: &[i16]) {
        if let Ok(mut ring) = self.ring.lock() {
            match ring.sample_rate {
                None => ring.sample_rate = Some(sampling_rate),
                Some(rate) if rate != sampling_rate => {
                    log::warn!("play_wave: sample rate mismatch ({rate} vs {sampling_rate}), keeping {rate}");
                }
                _ => {}
            }
            ring.push_stereo(channel, wave_data);
        }
    }

    fn midi_note_on(&self, _channel_id: u8, _note: u8, _velocity: u8) {}
    fn midi_note_off(&self, _channel_id: u8, _note: u8, _velocity: u8) {}
    fn midi_program_change(&self, _channel_id: u8, _program: u8) {}
    fn midi_control_change(&self, _channel_id: u8, _control: u8, _value: u8) {}
}
