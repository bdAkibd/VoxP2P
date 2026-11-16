use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioPlaybackConfig {
    pub device_id: Option<String>,
    pub master_volume: f32,
    pub is_muted: bool,
}

impl Default for AudioPlaybackConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            master_volume: 1.0,
            is_muted: false,
        }
    }
}

pub struct AudioPlaybackPipeline {
    pub config: AudioPlaybackConfig,
    pub is_running: bool,
}

impl AudioPlaybackPipeline {
    pub fn new(config: AudioPlaybackConfig) -> Self {
        Self {
            config,
            is_running: false,
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.config.master_volume = volume.clamp(0.0, 2.0);
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.config.is_muted = muted;
    }
}
