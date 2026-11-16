use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use nnnoiseless::DenoiseState;
use serde::{Deserialize, Serialize};

use super::codec::{AudioEncoder, OPUS_FRAME_SAMPLES, OPUS_SAMPLE_RATE};
use super::mixer::compute_rms_level;

pub const RNNOISE_CHUNK_SAMPLES: usize = 480; // 10ms at 48kHz
pub const VAD_HANGOVER_FRAMES: u32 = 10; // 200ms hangover after speech stops

pub type AudioPacketSender = Arc<dyn Fn(u64, Vec<u8>) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioCaptureConfig {
    pub device_id: Option<String>,
    pub enable_noise_suppression: bool,
    pub input_gain: f32,
    pub voice_activation_threshold: f32,
}

impl Default for AudioCaptureConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            enable_noise_suppression: true,
            input_gain: 1.0,
            voice_activation_threshold: 0.45,
        }
    }
}

/// Helper to store f32 in AtomicU32
fn f32_to_atomic(val: f32) -> u32 {
    val.to_bits()
}

fn atomic_to_f32(atomic: u32) -> f32 {
    f32::from_bits(atomic)
}

/// Lists all available microphone input devices on the system.
pub fn list_input_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.description().map(|desc| desc.name().to_string()).ok());
    let mut result = Vec::new();

    if let Ok(devices) = host.input_devices() {
        for (idx, dev) in devices.enumerate() {
            let name = dev
                .description()
                .map(|desc| desc.name().to_string())
                .unwrap_or_else(|_| format!("Microphone {}", idx + 1));
            let is_default = default_name.as_deref() == Some(&name)
                || (default_name.is_none() && idx == 0);
            result.push(AudioDeviceInfo {
                id: name.clone(),
                name,
                is_default,
            });
        }
    }

    if result.is_empty() {
        result.push(AudioDeviceInfo {
            id: "default_in".to_string(),
            name: "Default System Microphone".to_string(),
            is_default: true,
        });
    }

    result
}

/// Microphone capture pipeline with RNNoise noise suppression, VAD, and Opus encoding.
pub struct AudioCapturePipeline {
    pub config: Arc<RwLock<AudioCaptureConfig>>,
    is_running: Arc<AtomicBool>,
    is_muted: Arc<AtomicBool>,
    mic_level: Arc<AtomicU32>,
    packet_seq: Arc<AtomicU64>,
    hangover_frames: Arc<AtomicU32>,
    denoise_state: Arc<Mutex<Box<DenoiseState<'static>>>>,
    encoder: Arc<Mutex<AudioEncoder>>,
    packet_sender: Arc<RwLock<Option<AudioPacketSender>>>,
    local_node_id: Arc<RwLock<String>>,
    stream: Option<cpal::Stream>,
}



impl AudioCapturePipeline {
    pub fn new(config: AudioCaptureConfig) -> Self {
        let denoise_state = DenoiseState::new();
        let encoder = AudioEncoder::new().expect("Failed to initialize AudioEncoder");

        Self {
            config: Arc::new(RwLock::new(config)),
            is_running: Arc::new(AtomicBool::new(false)),
            is_muted: Arc::new(AtomicBool::new(false)),
            mic_level: Arc::new(AtomicU32::new(f32_to_atomic(0.0))),
            packet_seq: Arc::new(AtomicU64::new(0)),
            hangover_frames: Arc::new(AtomicU32::new(0)),
            denoise_state: Arc::new(Mutex::new(denoise_state)),
            encoder: Arc::new(Mutex::new(encoder)),
            packet_sender: Arc::new(RwLock::new(None)),
            local_node_id: Arc::new(RwLock::new("local_node".to_string())),
            stream: None,
        }
    }

    pub fn set_local_node_id(&self, node_id: String) {
        let mut id = self.local_node_id.write().unwrap();
        *id = node_id;
    }

    pub fn get_local_node_id(&self) -> String {
        let id = self.local_node_id.read().unwrap();
        id.clone()
    }

    pub fn set_packet_sender(&self, sender: Option<AudioPacketSender>) {
        let mut ps = self.packet_sender.write().unwrap();
        *ps = sender;
    }

    pub fn set_muted(&self, muted: bool) {
        self.is_muted.store(muted, Ordering::Release);
        if muted {
            self.mic_level.store(f32_to_atomic(0.0), Ordering::Release);
        }
    }

    pub fn is_muted(&self) -> bool {
        self.is_muted.load(Ordering::Acquire)
    }

    pub fn set_gain(&self, gain: f32) {
        let mut conf = self.config.write().unwrap();
        conf.input_gain = gain.clamp(0.0, 2.0);
    }

    pub fn get_gain(&self) -> f32 {
        let conf = self.config.read().unwrap();
        conf.input_gain
    }

    pub fn set_vad_threshold(&self, threshold: f32) {
        let mut conf = self.config.write().unwrap();
        conf.voice_activation_threshold = threshold.clamp(0.0, 1.0);
    }

    pub fn get_mic_level(&self) -> f32 {
        atomic_to_f32(self.mic_level.load(Ordering::Acquire))
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    pub fn set_noise_suppression(&self, enabled: bool) {
        let mut conf = self.config.write().unwrap();
        conf.enable_noise_suppression = enabled;
    }

    /// Pure DSP frame processing: applies gain, RNNoise denoise, VAD threshold, and Opus encoding.
    /// Returns `Some((seq, opus_bytes))` if voice was detected and encoded.
    pub fn process_frame_dsp(
        &self,
        raw_pcm: &[f32; OPUS_FRAME_SAMPLES],
    ) -> Result<Option<(u64, Vec<u8>)>, String> {
        let gain = self.get_gain();
        let (vad_threshold, enable_ns) = {
            let conf = self.config.read().unwrap();
            (conf.voice_activation_threshold, conf.enable_noise_suppression)
        };
        let is_muted = self.is_muted();

        // 1. Apply microphone gain
        let mut gained_pcm = [0.0f32; OPUS_FRAME_SAMPLES];
        for (i, sample) in raw_pcm.iter().enumerate() {
            gained_pcm[i] = sample * gain;
        }

        // 2. Process through RNNoise if enabled (two 480-sample chunks)
        let (denoised_frame, vad_prob) = if enable_ns {
            let mut denoiser = self
                .denoise_state
                .lock()
                .map_err(|e| format!("Denoiser lock poisoned: {e}"))?;

            // First 480 samples
            let mut chunk1_in = [0.0f32; RNNOISE_CHUNK_SAMPLES];
            chunk1_in.copy_from_slice(&gained_pcm[..RNNOISE_CHUNK_SAMPLES]);
            let mut chunk1_out = [0.0f32; RNNOISE_CHUNK_SAMPLES];
            let vad1 = denoiser.process_frame(&mut chunk1_out, &chunk1_in);

            // Second 480 samples
            let mut chunk2_in = [0.0f32; RNNOISE_CHUNK_SAMPLES];
            chunk2_in.copy_from_slice(&gained_pcm[RNNOISE_CHUNK_SAMPLES..]);
            let mut chunk2_out = [0.0f32; RNNOISE_CHUNK_SAMPLES];
            let vad2 = denoiser.process_frame(&mut chunk2_out, &chunk2_in);

            let mut out = [0.0f32; OPUS_FRAME_SAMPLES];
            out[..RNNOISE_CHUNK_SAMPLES].copy_from_slice(&chunk1_out);
            out[RNNOISE_CHUNK_SAMPLES..].copy_from_slice(&chunk2_out);

            (out, vad1.max(vad2))
        } else {
            (gained_pcm, 1.0)
        };


        // 3. Compute frame RMS volume level
        let rms = compute_rms_level(&denoised_frame);

        if is_muted {
            self.mic_level.store(f32_to_atomic(0.0), Ordering::Release);
            return Ok(None);
        }

        self.mic_level.store(f32_to_atomic(rms), Ordering::Release);

        // 4. Voice Activity Detection (VAD) + Hangover logic
        let speech_active = vad_prob >= vad_threshold || rms >= (vad_threshold * 0.15);

        let should_transmit = if speech_active {
            self.hangover_frames
                .store(VAD_HANGOVER_FRAMES, Ordering::Release);
            true
        } else {
            let rem = self.hangover_frames.load(Ordering::Acquire);
            if rem > 0 {
                self.hangover_frames.store(rem - 1, Ordering::Release);
                true
            } else {
                false
            }
        };

        if !should_transmit {
            return Ok(None);
        }

        // 5. Encode via Opus
        let mut encoder = self
            .encoder
            .lock()
            .map_err(|e| format!("Encoder lock poisoned: {e}"))?;
        let opus_data = encoder.encode(&denoised_frame)?;
        let seq = self.packet_seq.fetch_add(1, Ordering::SeqCst) + 1;

        // 6. Send via packet sender if registered
        if let Some(ref sender) = *self.packet_sender.read().unwrap() {
            sender(seq, opus_data.clone());
        }

        Ok(Some((seq, opus_data)))
    }

    /// Starts microphone capture from the chosen device (or system default).
    pub fn start(&mut self, device_name: Option<&str>) -> Result<(), String> {
        let host = cpal::default_host();
        let device = if let Some(name) = device_name {
            host.input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| {
                    d.description()
                        .map(|desc| desc.name() == name)
                        .unwrap_or(false)
                })
                .ok_or_else(|| format!("Input device '{name}' not found"))?
        } else {
            host.default_input_device()
                .ok_or_else(|| "No default input device available".to_string())?
        };

        let default_config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {e}"))?;

        let channels = default_config.channels() as usize;
        let sample_rate: u32 = default_config.sample_rate().into();

        let pipeline_ref = Arc::new(self.clone_dsp_handles());

        // Internal buffer to accumulate incoming samples
        let sample_fifo = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
            OPUS_FRAME_SAMPLES * 4,
        )));
        let fifo_clone = sample_fifo.clone();

        let err_fn = |err| eprintln!("Microphone capture stream error: {err}");

        let stream_config: cpal::StreamConfig = default_config.into();
        let stream = match default_config.sample_format() {
            cpal::SampleFormat::F32 => {
                let p_ref = pipeline_ref.clone();
                device
                    .build_input_stream(
                        stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            Self::handle_raw_samples(
                                data,
                                channels,
                                sample_rate,
                                &fifo_clone,
                                &p_ref,
                            );
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| format!("Failed to build f32 input stream: {e}"))?
            }
            cpal::SampleFormat::I16 => {
                let p_ref = pipeline_ref.clone();
                device
                    .build_input_stream(
                        stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let f32_samples: Vec<f32> =
                                data.iter().map(|&s| s as f32 / 32768.0).collect();
                            Self::handle_raw_samples(
                                &f32_samples,
                                channels,
                                sample_rate,
                                &fifo_clone,
                                &p_ref,
                            );
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| format!("Failed to build i16 input stream: {e}"))?
            }
            other => return Err(format!("Unsupported audio sample format: {other:?}")),
        };


        stream
            .play()
            .map_err(|e| format!("Failed to start input stream: {e}"))?;

        self.stream = Some(stream);
        self.is_running.store(true, Ordering::Release);
        Ok(())
    }


    /// Converts incoming multi-channel samples to 48kHz mono and triggers DSP when 960 samples accumulate.
    fn handle_raw_samples(
        data: &[f32],
        channels: usize,
        src_rate: u32,
        fifo: &Arc<Mutex<Vec<f32>>>,
        dsp: &DspHandles,
    ) {
        if data.is_empty() {
            return;
        }

        // Downmix multi-channel to mono
        let mut mono_data = Vec::with_capacity(data.len() / channels);
        for chunk in data.chunks_exact(channels) {
            let sum: f32 = chunk.iter().sum();
            mono_data.push(sum / channels as f32);
        }

        // Resample to 48kHz if required
        let resampled_mono = if src_rate == OPUS_SAMPLE_RATE {
            mono_data
        } else {
            let ratio = src_rate as f64 / OPUS_SAMPLE_RATE as f64;
            let target_len = (mono_data.len() as f64 / ratio).round() as usize;
            let mut out = Vec::with_capacity(target_len);

            for i in 0..target_len {
                let src_idx = i as f64 * ratio;
                let idx0 = src_idx.floor() as usize;
                let idx1 = (idx0 + 1).min(mono_data.len() - 1);
                let frac = (src_idx - idx0 as f64) as f32;

                let s0 = mono_data[idx0];
                let s1 = mono_data[idx1];
                out.push(s0 + frac * (s1 - s0));
            }
            out
        };

        // Push to FIFO and process 960-sample chunks
        let mut buffer = match fifo.lock() {
            Ok(b) => b,
            Err(_) => return,
        };
        buffer.extend_from_slice(&resampled_mono);

        while buffer.len() >= OPUS_FRAME_SAMPLES {
            let mut frame = [0.0f32; OPUS_FRAME_SAMPLES];
            frame.copy_from_slice(&buffer[..OPUS_FRAME_SAMPLES]);
            buffer.drain(..OPUS_FRAME_SAMPLES);

            let _ = dsp.process_frame(&frame);
        }
    }

    pub fn stop(&mut self) {
        self.stream = None;
        self.is_running.store(false, Ordering::Release);
        self.mic_level.store(f32_to_atomic(0.0), Ordering::Release);
    }

    fn clone_dsp_handles(&self) -> DspHandles {
        DspHandles {
            config: self.config.clone(),
            is_muted: self.is_muted.clone(),
            mic_level: self.mic_level.clone(),
            packet_seq: self.packet_seq.clone(),
            hangover_frames: self.hangover_frames.clone(),
            denoise_state: self.denoise_state.clone(),
            encoder: self.encoder.clone(),
            packet_sender: self.packet_sender.clone(),
        }
    }
}

/// Lightweight handle cloneable for real-time CPAL callback closure.
struct DspHandles {
    config: Arc<RwLock<AudioCaptureConfig>>,
    is_muted: Arc<AtomicBool>,
    mic_level: Arc<AtomicU32>,
    packet_seq: Arc<AtomicU64>,
    hangover_frames: Arc<AtomicU32>,
    denoise_state: Arc<Mutex<Box<DenoiseState<'static>>>>,
    encoder: Arc<Mutex<AudioEncoder>>,
    packet_sender: Arc<RwLock<Option<AudioPacketSender>>>,
}

impl DspHandles {
    fn process_frame(&self, raw_pcm: &[f32; OPUS_FRAME_SAMPLES]) -> Result<(), String> {
        let (gain, vad_threshold, enable_ns) = {
            let conf = self.config.read().unwrap();
            (conf.input_gain, conf.voice_activation_threshold, conf.enable_noise_suppression)
        };
        let is_muted = self.is_muted.load(Ordering::Acquire);

        let mut gained_pcm = [0.0f32; OPUS_FRAME_SAMPLES];
        for (i, sample) in raw_pcm.iter().enumerate() {
            gained_pcm[i] = sample * gain;
        }

        let (denoised_frame, vad_prob) = if enable_ns {
            let mut denoiser = match self.denoise_state.lock() {
                Ok(d) => d,
                Err(_) => return Ok(()),
            };

            let mut chunk1_in = [0.0f32; RNNOISE_CHUNK_SAMPLES];
            chunk1_in.copy_from_slice(&gained_pcm[..RNNOISE_CHUNK_SAMPLES]);
            let mut chunk1_out = [0.0f32; RNNOISE_CHUNK_SAMPLES];
            let vad1 = denoiser.process_frame(&mut chunk1_out, &chunk1_in);

            let mut chunk2_in = [0.0f32; RNNOISE_CHUNK_SAMPLES];
            chunk2_in.copy_from_slice(&gained_pcm[RNNOISE_CHUNK_SAMPLES..]);
            let mut chunk2_out = [0.0f32; RNNOISE_CHUNK_SAMPLES];
            let vad2 = denoiser.process_frame(&mut chunk2_out, &chunk2_in);

            let mut out = [0.0f32; OPUS_FRAME_SAMPLES];
            out[..RNNOISE_CHUNK_SAMPLES].copy_from_slice(&chunk1_out);
            out[RNNOISE_CHUNK_SAMPLES..].copy_from_slice(&chunk2_out);

            (out, vad1.max(vad2))
        } else {
            (gained_pcm, 1.0)
        };


        let rms = compute_rms_level(&denoised_frame);

        if is_muted {
            self.mic_level.store(f32_to_atomic(0.0), Ordering::Release);
            return Ok(());
        }

        self.mic_level.store(f32_to_atomic(rms), Ordering::Release);

        let speech_active = vad_prob >= vad_threshold || rms >= (vad_threshold * 0.15);

        let should_transmit = if speech_active {
            self.hangover_frames
                .store(VAD_HANGOVER_FRAMES, Ordering::Release);
            true
        } else {
            let rem = self.hangover_frames.load(Ordering::Acquire);
            if rem > 0 {
                self.hangover_frames.store(rem - 1, Ordering::Release);
                true
            } else {
                false
            }
        };

        if !should_transmit {
            return Ok(());
        }

        let mut encoder = match self.encoder.lock() {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };
        let opus_data = encoder.encode(&denoised_frame)?;
        let seq = self.packet_seq.fetch_add(1, Ordering::SeqCst) + 1;

        if let Ok(sender_guard) = self.packet_sender.read() {
            if let Some(ref sender) = *sender_guard {
                sender(seq, opus_data);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_input_devices_not_empty() {
        let devices = list_input_devices();
        assert!(!devices.is_empty(), "Should return at least 1 device");
    }

    #[test]
    fn test_dsp_mute_suppresses_transmission() {
        let pipeline = AudioCapturePipeline::new(AudioCaptureConfig::default());
        pipeline.set_muted(true);

        let tone = [0.5f32; OPUS_FRAME_SAMPLES];
        let result = pipeline.process_frame_dsp(&tone).unwrap();

        assert!(
            result.is_none(),
            "Muted mic must not produce audio packets"
        );
        assert_eq!(pipeline.get_mic_level(), 0.0);
    }

    #[test]
    fn test_dsp_speech_produces_packet() {
        let pipeline = AudioCapturePipeline::new(AudioCaptureConfig {
            voice_activation_threshold: 0.1,
            ..Default::default()
        });

        // 440Hz test sine tone
        let mut tone = [0.0f32; OPUS_FRAME_SAMPLES];
        for i in 0..OPUS_FRAME_SAMPLES {
            let t = i as f32 / 48000.0;
            tone[i] = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.8;
        }

        let result = pipeline.process_frame_dsp(&tone).unwrap();
        assert!(
            result.is_some(),
            "Active speech tone must produce an Opus packet"
        );
        let (seq, packet) = result.unwrap();
        assert_eq!(seq, 1);
        assert!(!packet.is_empty());
        assert!(pipeline.get_mic_level() > 0.0);
    }
}
