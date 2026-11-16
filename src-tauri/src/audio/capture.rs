use serde::{Deserialize, Serialize};

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
            voice_activation_threshold: 0.02,
        }
    }
}

pub struct AudioCapturePipeline {
    pub config: AudioCaptureConfig,
    pub is_active: bool,
}

impl AudioCapturePipeline {
    pub fn new(config: AudioCaptureConfig) -> Self {
        Self {
            config,
            is_active: false,
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }

    pub fn update_gain(&mut self, gain: f32) {
        self.config.input_gain = gain.clamp(0.0, 2.0);
    }
}
