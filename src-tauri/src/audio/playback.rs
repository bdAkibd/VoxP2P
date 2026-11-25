use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};

use super::capture::AudioDeviceInfo;
use super::mixer::AudioMixer;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioPlaybackConfig {
    pub device_id: Option<String>,
    pub master_volume: f32,
    pub is_deafened: bool,
}

impl Default for AudioPlaybackConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            master_volume: 1.0,
            is_deafened: false,
        }
    }
}

/// Lists all available playback (speaker/headphone) output devices on the system.
pub fn list_output_devices() -> Vec<AudioDeviceInfo> {
    let mut result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let host = cpal::default_host();
        let default_name = host
            .default_output_device()
            .and_then(|d| d.description().map(|desc| desc.name().to_string()).ok());
        let mut devices_list = Vec::new();

        if let Ok(devices) = host.output_devices() {
            for (idx, dev) in devices.enumerate() {
                let name = dev
                    .description()
                    .map(|desc| desc.name().to_string())
                    .unwrap_or_else(|_| format!("Speaker {}", idx + 1));
                let is_default = default_name.as_deref() == Some(&name)
                    || (default_name.is_none() && idx == 0);
                devices_list.push(AudioDeviceInfo {
                    id: name.clone(),
                    name,
                    is_default,
                });
            }
        }
        devices_list
    }))
    .unwrap_or_else(|_| {
        eprintln!("[VoxP2P] Panic caught while enumerating output devices");
        Vec::new()
    });

    if result.is_empty() {
        result.push(AudioDeviceInfo {
            id: "default_out".to_string(),
            name: "Default System Speaker".to_string(),
            is_default: true,
        });
    }

    result
}

/// Audio playback pipeline managing CPAL output stream and AudioMixer pull.
pub struct AudioPlaybackPipeline {
    pub config: Arc<RwLock<AudioPlaybackConfig>>,
    pub mixer: Arc<AudioMixer>,
    is_running: Arc<AtomicBool>,
    stream: Option<cpal::Stream>,
}

impl AudioPlaybackPipeline {
    pub fn new(config: AudioPlaybackConfig, mixer: Arc<AudioMixer>) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            mixer,
            is_running: Arc::new(AtomicBool::new(false)),
            stream: None,
        }
    }

    pub fn set_volume(&self, volume: f32) {
        let mut conf = self.config.write().unwrap();
        conf.master_volume = volume.clamp(0.0, 2.0);
        self.mixer.set_master_volume(conf.master_volume);
    }

    pub fn get_volume(&self) -> f32 {
        self.mixer.get_master_volume()
    }

    pub fn set_deafened(&self, deafened: bool) {
        let mut conf = self.config.write().unwrap();
        conf.is_deafened = deafened;
        self.mixer.set_deafened(deafened);
    }

    pub fn is_deafened(&self) -> bool {
        self.mixer.is_deafened()
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    /// Starts playback stream on the specified output device (or system default).
    pub fn start(&mut self, device_name: Option<&str>) -> Result<(), String> {
        let host = cpal::default_host();
        let device = if let Some(name) = device_name {
            host.output_devices()
                .map_err(|e| e.to_string())?
                .find(|d| {
                    d.description()
                        .map(|desc| desc.name() == name)
                        .unwrap_or(false)
                })
                .or_else(|| host.default_output_device())
                .ok_or_else(|| format!("Output device '{name}' not found"))?
        } else {
            host.default_output_device()
                .ok_or_else(|| "No default output device available".to_string())?
        };

        let default_config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default output config: {e}"))?;

        let channels = default_config.channels() as usize;
        let mixer_clone = self.mixer.clone();
        let err_fn = |err| eprintln!("Audio playback stream error: {err}");

        let stream_config: cpal::StreamConfig = default_config.into();
        let stream = match default_config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_output_stream(
                    stream_config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        Self::render_samples(data, channels, &mixer_clone);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build f32 playback stream: {e}"))?,
            cpal::SampleFormat::I16 => {
                let mut scratch_f32 = Vec::new();
                device
                    .build_output_stream(
                        stream_config,
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            scratch_f32.resize(data.len(), 0.0);
                            Self::render_samples(&mut scratch_f32, channels, &mixer_clone);
                            for (i, &sample) in scratch_f32.iter().enumerate() {
                                data[i] = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                            }
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| format!("Failed to build i16 playback stream: {e}"))?
            }
            other => return Err(format!("Unsupported playback sample format: {other:?}")),
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start playback stream: {e}"))?;

        self.stream = Some(stream);
        self.is_running.store(true, Ordering::Release);
        Ok(())
    }

    /// Pulls mixed PCM samples from AudioMixer and maps them to output channels.
    fn render_samples(data: &mut [f32], channels: usize, mixer: &Arc<AudioMixer>) {
        if channels == 0 || data.len() < channels {
            data.fill(0.0);
            return;
        }

        let frame_count = data.len() / channels;
        let mut mono_buffer = vec![0.0f32; frame_count];
        let mut peer_levels = HashMap::new();

        mixer.mix_output(&mut mono_buffer, &mut peer_levels);

        if channels == 1 {
            data.copy_from_slice(&mono_buffer);
        } else if channels == 2 {
            for (i, &s) in mono_buffer.iter().enumerate() {
                data[2 * i] = s;
                data[2 * i + 1] = s;
            }
        } else {
            for (i, &s) in mono_buffer.iter().enumerate() {
                for ch in 0..channels {
                    data[i * channels + ch] = s;
                }
            }
        }
    }

    pub fn stop(&mut self) {
        self.stream = None;
        self.is_running.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_output_devices_not_empty() {
        let devices = list_output_devices();
        assert!(!devices.is_empty(), "Should return at least 1 output device");
    }

    #[test]
    fn test_playback_volume_and_deafen_flags() {
        let mixer = Arc::new(AudioMixer::new());
        let pipeline = AudioPlaybackPipeline::new(AudioPlaybackConfig::default(), mixer.clone());

        assert_eq!(pipeline.get_volume(), 1.0);
        assert!(!pipeline.is_deafened());

        pipeline.set_volume(1.8);
        assert_eq!(pipeline.get_volume(), 1.8);
        assert_eq!(mixer.get_master_volume(), 1.8);

        pipeline.set_deafened(true);
        assert!(pipeline.is_deafened());
        assert!(mixer.is_deafened());
    }
}
