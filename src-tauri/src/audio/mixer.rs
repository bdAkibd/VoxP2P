use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerVolumeLevel {
    pub node_id: String,
    pub volume_level: f32, // RMS level in [0.0, 1.0]
}

#[derive(Debug, Clone)]
pub struct PeerAudioStream {
    pub node_id: String,
    pub volume: f32,
    pub is_muted: bool,
}

pub struct AudioMixer {
    pub master_volume: f32,
    pub streams: Vec<PeerAudioStream>,
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMixer {
    pub fn new() -> Self {
        Self {
            master_volume: 1.0,
            streams: Vec::new(),
        }
    }

    pub fn set_peer_volume(&mut self, node_id: &str, volume: f32) {
        if let Some(stream) = self.streams.iter_mut().find(|s| s.node_id == node_id) {
            stream.volume = volume.clamp(0.0, 2.0);
        } else {
            self.streams.push(PeerAudioStream {
                node_id: node_id.to_string(),
                volume: volume.clamp(0.0, 2.0),
                is_muted: false,
            });
        }
    }

    pub fn set_peer_muted(&mut self, node_id: &str, muted: bool) {
        if let Some(stream) = self.streams.iter_mut().find(|s| s.node_id == node_id) {
            stream.is_muted = muted;
        }
    }

    /// Mixes multiple PCM f32 buffers into an output buffer with soft-clipping protection.
    pub fn mix_pcm_buffers(peer_buffers: &[(&PeerAudioStream, &[f32])], output: &mut [f32]) {
        for sample in output.iter_mut() {
            *sample = 0.0;
        }

        for (stream, buffer) in peer_buffers {
            if stream.is_muted {
                continue;
            }
            let gain = stream.volume;
            let len = buffer.len().min(output.len());
            for i in 0..len {
                output[i] += buffer[i] * gain;
            }
        }

        // Apply tanh soft-clipping to prevent harsh distortion
        for sample in output.iter_mut() {
            *sample = sample.tanh();
        }
    }

    /// Calculates root-mean-square (RMS) volume level of a PCM audio buffer.
    pub fn compute_rms_level(buffer: &[f32]) -> f32 {
        if buffer.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = buffer.iter().map(|&s| s * s).sum();
        (sum_sq / buffer.len() as f32).sqrt().min(1.0)
    }
}
