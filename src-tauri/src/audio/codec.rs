use serde::{Deserialize, Serialize};

pub const OPUS_SAMPLE_RATE: u32 = 48000;
pub const OPUS_CHANNELS: u16 = 1;
pub const OPUS_FRAME_SIZE_MS: u32 = 20;
pub const OPUS_FRAME_SAMPLES: usize = (OPUS_SAMPLE_RATE as usize * OPUS_FRAME_SIZE_MS as usize) / 1000; // 960 samples

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioCodecConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate: u32, // 24_000 to 64_000 bps
}

impl Default for AudioCodecConfig {
    fn default() -> Self {
        Self {
            sample_rate: OPUS_SAMPLE_RATE,
            channels: OPUS_CHANNELS,
            bitrate: 48000,
        }
    }
}
