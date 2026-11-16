use audiopus::{
    coder::{Decoder, Encoder},
    packet::Packet,
    Application, Bitrate, Channels, MutSignals, SampleRate,
};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

pub const OPUS_SAMPLE_RATE: u32 = 48000;
pub const OPUS_CHANNELS: u16 = 1;
pub const OPUS_FRAME_SIZE_MS: u32 = 20;
pub const OPUS_FRAME_SAMPLES: usize = (OPUS_SAMPLE_RATE as usize * OPUS_FRAME_SIZE_MS as usize) / 1000; // 960 samples
pub const OPUS_BITRATE: u32 = 32000; // 32kbps
pub const MAX_OPUS_PACKET_SIZE: usize = 1276;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioCodecConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate: u32,
    pub frame_size_ms: u32,
}

impl Default for AudioCodecConfig {
    fn default() -> Self {
        Self {
            sample_rate: OPUS_SAMPLE_RATE,
            channels: OPUS_CHANNELS,
            bitrate: OPUS_BITRATE,
            frame_size_ms: OPUS_FRAME_SIZE_MS,
        }
    }
}

/// Opus audio encoder configured for VoIP low-latency voice streaming.
pub struct AudioEncoder {
    encoder: Encoder,
    config: AudioCodecConfig,
}

impl AudioEncoder {
    /// Creates a new Opus encoder with VoIP application mode, 32kbps bitrate, and in-band FEC enabled.
    pub fn new() -> Result<Self, String> {
        Self::with_config(AudioCodecConfig::default())
    }

    /// Creates an Opus encoder with custom configuration.
    pub fn with_config(config: AudioCodecConfig) -> Result<Self, String> {
        let channels = match config.channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            other => return Err(format!("Unsupported channel count: {other}")),
        };

        let sample_rate = match config.sample_rate {
            8000 => SampleRate::Hz8000,
            12000 => SampleRate::Hz12000,
            16000 => SampleRate::Hz16000,
            24000 => SampleRate::Hz24000,
            48000 => SampleRate::Hz48000,
            other => return Err(format!("Unsupported sample rate: {other}")),
        };

        let mut encoder = Encoder::new(sample_rate, channels, Application::Voip)
            .map_err(|e| format!("Failed to create Opus encoder: {e}"))?;

        encoder
            .set_bitrate(Bitrate::BitsPerSecond(config.bitrate as i32))
            .map_err(|e| format!("Failed to set Opus bitrate: {e}"))?;

        // Enable in-band FEC for packet loss resilience
        encoder
            .set_inband_fec(true)
            .map_err(|e| format!("Failed to enable in-band FEC: {e}"))?;

        Ok(Self { encoder, config })
    }

    /// Encodes a 20ms mono PCM frame (960 f32 samples) into an Opus packet byte buffer.
    pub fn encode(&mut self, pcm: &[f32; OPUS_FRAME_SAMPLES]) -> Result<Vec<u8>, String> {
        let mut out_buffer = vec![0u8; MAX_OPUS_PACKET_SIZE];
        let bytes_written = self
            .encoder
            .encode_float(pcm, &mut out_buffer)
            .map_err(|e| format!("Opus encoding failed: {e}"))?;

        out_buffer.truncate(bytes_written);
        Ok(out_buffer)
    }

    pub fn config(&self) -> &AudioCodecConfig {
        &self.config
    }
}

/// Opus audio decoder supporting packet loss concealment (PLC).
pub struct AudioDecoder {
    decoder: Decoder,
    channels: Channels,
}

impl AudioDecoder {
    /// Creates a new Opus decoder for 48kHz mono audio.
    pub fn new() -> Result<Self, String> {
        Self::with_sample_rate_and_channels(SampleRate::Hz48000, Channels::Mono)
    }

    pub fn with_sample_rate_and_channels(
        sample_rate: SampleRate,
        channels: Channels,
    ) -> Result<Self, String> {
        let decoder = Decoder::new(sample_rate, channels)
            .map_err(|e| format!("Failed to create Opus decoder: {e}"))?;

        Ok(Self { decoder, channels })
    }

    pub fn channels(&self) -> Channels {
        self.channels
    }

    /// Decodes an incoming Opus packet into 960 f32 PCM samples (20ms).
    /// If `packet` is `None`, performs Packet Loss Concealment (PLC) to reconstruct missing audio.
    pub fn decode(
        &mut self,
        packet: Option<&[u8]>,
        out_pcm: &mut [f32; OPUS_FRAME_SAMPLES],
        fec: bool,
    ) -> Result<usize, String> {
        let out_signals = MutSignals::try_from(out_pcm.as_mut_slice())
            .map_err(|e| format!("Invalid output PCM buffer for Opus decoding: {e}"))?;

        match packet {
            Some(data) => {
                let packet_ref = Packet::try_from(data)
                    .map_err(|e| format!("Invalid Opus packet buffer: {e}"))?;

                self.decoder
                    .decode_float(Some(packet_ref), out_signals, fec)
                    .map_err(|e| format!("Opus decoding failed: {e}"))
            }
            None => {
                // Packet loss concealment (PLC)
                self.decoder
                    .decode_float(None, out_signals, false)
                    .map_err(|e| format!("Opus PLC decoding failed: {e}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_decoder_roundtrip() {
        let mut encoder = AudioEncoder::new().expect("Failed to create encoder");
        let mut decoder = AudioDecoder::new().expect("Failed to create decoder");

        // Generate a 440 Hz test tone
        let mut pcm_in = [0.0f32; OPUS_FRAME_SAMPLES];
        for i in 0..OPUS_FRAME_SAMPLES {
            let t = i as f32 / OPUS_SAMPLE_RATE as f32;
            pcm_in[i] = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
        }

        let encoded = encoder.encode(&pcm_in).expect("Encoding should succeed");
        assert!(!encoded.is_empty(), "Encoded buffer should not be empty");
        assert!(encoded.len() <= MAX_OPUS_PACKET_SIZE);

        let mut pcm_out = [0.0f32; OPUS_FRAME_SAMPLES];
        let samples_decoded = decoder
            .decode(Some(&encoded), &mut pcm_out, false)
            .expect("Decoding should succeed");

        assert_eq!(samples_decoded, OPUS_FRAME_SAMPLES);
        // Verify output is not pure silence
        let energy: f32 = pcm_out.iter().map(|s| s * s).sum();
        assert!(energy > 0.01, "Decoded tone should have non-zero energy");
    }

    #[test]
    fn test_decoder_packet_loss_concealment() {
        let mut decoder = AudioDecoder::new().expect("Failed to create decoder");

        let mut plc_out = [0.0f32; OPUS_FRAME_SAMPLES];
        // Decode None (PLC)
        let samples_decoded = decoder
            .decode(None, &mut plc_out, false)
            .expect("PLC decoding should succeed");

        assert_eq!(samples_decoded, OPUS_FRAME_SAMPLES);
    }
}
