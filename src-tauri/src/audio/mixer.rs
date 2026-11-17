use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use ringbuf::traits::*;
use ringbuf::HeapRb;
use serde::{Deserialize, Serialize};

use super::codec::{AudioDecoder, OPUS_FRAME_SAMPLES};
use crate::network::room::AudioPacket;

pub const JITTER_BUFFER_CAPACITY: usize = OPUS_FRAME_SAMPLES * 10; // 9600 samples (200ms)
pub const LOOPBACK_BUFFER_CAPACITY: usize = OPUS_FRAME_SAMPLES * 10; // 9600 samples (200ms)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerVolumeLevel {
    pub node_id: String,
    pub volume_level: f32, // RMS level in [0.0, 1.0]
}

/// Helper to store f32 in AtomicU32
fn f32_to_atomic(val: f32) -> u32 {
    val.to_bits()
}

fn atomic_to_f32(atomic: u32) -> f32 {
    f32::from_bits(atomic)
}

/// Computes the Root-Mean-Square (RMS) volume level of a PCM audio buffer.
pub fn compute_rms_level(buffer: &[f32]) -> f32 {
    if buffer.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = buffer.iter().map(|&s| s * s).sum();
    (sum_sq / buffer.len() as f32).sqrt().min(1.0)
}

/// Jitter buffer and dedicated Opus decoder for a single remote peer.
pub struct PeerJitterBuffer {
    pub node_id: String,
    volume: AtomicU32,
    is_muted: AtomicBool,
    last_seq: AtomicU64,
    has_received_packet: AtomicBool,
    decoder: Mutex<AudioDecoder>,
    producer: Mutex<ringbuf::HeapProd<f32>>,
    consumer: Mutex<ringbuf::HeapCons<f32>>,
}


impl PeerJitterBuffer {
    pub fn new(node_id: &str, initial_volume: f32) -> Result<Self, String> {
        let decoder = AudioDecoder::new()?;
        let rb = HeapRb::<f32>::new(JITTER_BUFFER_CAPACITY);
        let (producer, consumer) = rb.split();

        Ok(Self {
            node_id: node_id.to_string(),
            volume: AtomicU32::new(f32_to_atomic(initial_volume.clamp(0.0, 2.0))),
            is_muted: AtomicBool::new(false),
            last_seq: AtomicU64::new(0),
            has_received_packet: AtomicBool::new(false),
            decoder: Mutex::new(decoder),
            producer: Mutex::new(producer),
            consumer: Mutex::new(consumer),
        })
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(f32_to_atomic(volume.clamp(0.0, 2.0)), Ordering::Release);
    }

    pub fn get_volume(&self) -> f32 {
        atomic_to_f32(self.volume.load(Ordering::Acquire))
    }

    pub fn set_muted(&self, muted: bool) {
        self.is_muted.store(muted, Ordering::Release);
    }

    pub fn is_muted(&self) -> bool {
        self.is_muted.load(Ordering::Acquire)
    }

    /// Handles an incoming Opus audio packet for this peer.
    /// Implements Packet Loss Concealment (PLC) for missing sequence numbers.
    pub fn handle_packet(&self, seq: u64, opus_data: &[u8]) -> Result<(), String> {
        let mut decoder = self
            .decoder
            .lock()
            .map_err(|e| format!("Decoder lock poisoned: {e}"))?;
        let mut producer = self
            .producer
            .lock()
            .map_err(|e| format!("Producer lock poisoned: {e}"))?;

        let was_initialized = self.has_received_packet.swap(true, Ordering::SeqCst);

        if was_initialized {
            let prev_seq = self.last_seq.load(Ordering::Acquire);
            if seq <= prev_seq {
                // Out-of-order or duplicate packet; drop to prevent stutter
                return Ok(());
            }

            // Check for packet loss
            let missing = seq.saturating_sub(prev_seq + 1);
            if missing > 0 {
                // Limit PLC concealment to at most 5 frames (100ms) to prevent audio drift
                let plc_frames = missing.min(5);
                for _ in 0..plc_frames {
                    let mut plc_pcm = [0.0f32; OPUS_FRAME_SAMPLES];
                    if decoder.decode(None, &mut plc_pcm, false).is_ok() {
                        self.push_samples_bounded(&mut producer, &plc_pcm);
                    }
                }
            }
        }

        // Decode the current frame
        let mut frame_pcm = [0.0f32; OPUS_FRAME_SAMPLES];
        decoder.decode(Some(opus_data), &mut frame_pcm, false)?;
        self.push_samples_bounded(&mut producer, &frame_pcm);

        self.last_seq.store(seq, Ordering::Release);
        Ok(())
    }

    /// Pushes samples into the ring buffer, maintaining bounded latency by skipping old samples if full.
    fn push_samples_bounded(
        &self,
        producer: &mut ringbuf::HeapProd<f32>,
        samples: &[f32; OPUS_FRAME_SAMPLES],
    ) {
        if producer.vacant_len() < OPUS_FRAME_SAMPLES {
            // Buffer is near full; drop older samples via consumer to prevent latency buildup
            if let Ok(mut consumer) = self.consumer.lock() {
                let overflow = OPUS_FRAME_SAMPLES.saturating_sub(producer.vacant_len());
                let _ = consumer.skip(overflow);
            }
        }
        let _ = producer.push_slice(samples);
    }

    /// Pulls up to `out.len()` samples into `out` and returns the RMS volume level.
    pub fn pull_samples(&self, out: &mut [f32]) -> f32 {
        let mut consumer = match self.consumer.lock() {
            Ok(c) => c,
            Err(_) => {
                out.fill(0.0);
                return 0.0;
            }
        };

        let read_count = consumer.pop_slice(out);
        if read_count < out.len() {
            out[read_count..].fill(0.0);
        }

        compute_rms_level(out)
    }

    /// Clears the internal ring buffer (e.g. when deafened).
    pub fn clear(&self) {
        if let Ok(mut consumer) = self.consumer.lock() {
            consumer.clear();
        }
    }
}

/// Multi-peer PCM audio mixer with per-peer jitter buffering, volume multipliers, and soft clipping.
pub struct AudioMixer {
    peers: Arc<RwLock<HashMap<String, Arc<PeerJitterBuffer>>>>,
    latest_peer_levels: Arc<RwLock<HashMap<String, f32>>>,
    master_volume: AtomicU32,
    is_deafened: AtomicBool,
    is_mic_test_enabled: Arc<AtomicBool>,
    loopback_producer: Mutex<ringbuf::HeapProd<f32>>,
    loopback_consumer: Mutex<ringbuf::HeapCons<f32>>,
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMixer {
    pub fn new() -> Self {
        let rb = HeapRb::<f32>::new(LOOPBACK_BUFFER_CAPACITY);
        let (loopback_producer, loopback_consumer) = rb.split();

        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            latest_peer_levels: Arc::new(RwLock::new(HashMap::new())),
            master_volume: AtomicU32::new(f32_to_atomic(1.0)),
            is_deafened: AtomicBool::new(false),
            is_mic_test_enabled: Arc::new(AtomicBool::new(false)),
            loopback_producer: Mutex::new(loopback_producer),
            loopback_consumer: Mutex::new(loopback_consumer),
        }
    }

    pub fn set_mic_test(&self, enabled: bool) {
        self.is_mic_test_enabled.store(enabled, Ordering::Release);
        if !enabled {
            if let Ok(mut cons) = self.loopback_consumer.lock() {
                cons.clear();
            }
        }
    }

    pub fn is_mic_test_enabled(&self) -> bool {
        self.is_mic_test_enabled.load(Ordering::Acquire)
    }

    pub fn push_loopback_samples(&self, samples: &[f32]) {
        if !self.is_mic_test_enabled() {
            return;
        }
        if let Ok(mut prod) = self.loopback_producer.lock() {
            if prod.vacant_len() < samples.len() {
                if let Ok(mut cons) = self.loopback_consumer.lock() {
                    let overflow = samples.len().saturating_sub(prod.vacant_len());
                    let _ = cons.skip(overflow);
                }
            }
            let _ = prod.push_slice(samples);
        }
    }

    pub fn get_peer_levels(&self) -> HashMap<String, f32> {
        let levels = self.latest_peer_levels.read().unwrap();
        levels.clone()
    }


    pub fn set_master_volume(&self, volume: f32) {
        self.master_volume
            .store(f32_to_atomic(volume.clamp(0.0, 2.0)), Ordering::Release);
    }

    pub fn get_master_volume(&self) -> f32 {
        atomic_to_f32(self.master_volume.load(Ordering::Acquire))
    }

    pub fn set_deafened(&self, deafened: bool) {
        self.is_deafened.store(deafened, Ordering::Release);
    }

    pub fn is_deafened(&self) -> bool {
        self.is_deafened.load(Ordering::Acquire)
    }

    pub fn add_or_update_peer(&self, node_id: &str, initial_volume: f32) -> Arc<PeerJitterBuffer> {
        let mut peers = self.peers.write().unwrap();
        if let Some(peer) = peers.get(node_id) {
            peer.set_volume(initial_volume);
            peer.clone()
        } else {
            let peer = Arc::new(
                PeerJitterBuffer::new(node_id, initial_volume)
                    .unwrap_or_else(|_| panic!("Failed to create jitter buffer for {node_id}")),
            );
            peers.insert(node_id.to_string(), peer.clone());
            peer
        }
    }

    pub fn remove_peer(&self, node_id: &str) {
        let mut peers = self.peers.write().unwrap();
        peers.remove(node_id);
    }

    pub fn set_peer_volume(&self, node_id: &str, volume: f32) {
        let peers = self.peers.read().unwrap();
        if let Some(peer) = peers.get(node_id) {
            peer.set_volume(volume);
        }
    }

    pub fn get_peer_volume(&self, node_id: &str) -> Option<f32> {
        let peers = self.peers.read().unwrap();
        peers.get(node_id).map(|p| p.get_volume())
    }

    pub fn set_peer_muted(&self, node_id: &str, muted: bool) {
        let peers = self.peers.read().unwrap();
        if let Some(peer) = peers.get(node_id) {
            peer.set_muted(muted);
        }
    }

    /// Handles an incoming network audio packet, automatically routing to the peer's jitter buffer.
    pub fn handle_audio_packet(&self, packet: &AudioPacket) -> Result<(), String> {
        let peer = {
            let peers = self.peers.read().unwrap();
            peers.get(&packet.sender_id).cloned()
        };

        let peer = match peer {
            Some(p) => p,
            None => self.add_or_update_peer(&packet.sender_id, 1.0),
        };

        peer.handle_packet(packet.seq, &packet.opus_data)
    }

    /// Mixes all active peers' PCM buffers into `output` with soft-clipping protection.
    /// Updates `peer_levels` with RMS volume levels for speaking rings.
    pub fn mix_output(&self, output: &mut [f32], peer_levels: &mut HashMap<String, f32>) {
        if self.is_deafened() {
            output.fill(0.0);
            // Drain buffers so latency does not build up while deafened
            let peers = self.peers.read().unwrap();
            for (node_id, peer) in peers.iter() {
                peer.clear();
                peer_levels.insert(node_id.clone(), 0.0);
            }
            if let Ok(mut cons) = self.loopback_consumer.lock() {
                cons.clear();
            }
            return;
        }

        output.fill(0.0);
        let master_gain = self.get_master_volume();

        // Snapshot peers to minimize read-lock duration
        let peer_list: Vec<Arc<PeerJitterBuffer>> = {
            let peers = self.peers.read().unwrap();
            peers.values().cloned().collect()
        };

        let mut scratch = vec![0.0f32; output.len()];

        for peer in peer_list {
            let rms = peer.pull_samples(&mut scratch);
            peer_levels.insert(peer.node_id.clone(), rms);

            if !peer.is_muted() {
                let peer_gain = peer.get_volume() * master_gain;
                for i in 0..output.len() {
                    output[i] += scratch[i] * peer_gain;
                }
            }
        }

        // Mix in microphone loopback test samples if enabled
        if self.is_mic_test_enabled() {
            let mut loopback_scratch = vec![0.0f32; output.len()];
            if let Ok(mut cons) = self.loopback_consumer.lock() {
                let read = cons.pop_slice(&mut loopback_scratch);
                for i in 0..read {
                    output[i] += loopback_scratch[i] * master_gain;
                }
            }
        }

        // Apply tanh soft clipping to prevent harsh digital distortion
        for sample in output.iter_mut() {
            *sample = sample.tanh();
        }

        if let Ok(mut latest) = self.latest_peer_levels.write() {
            *latest = peer_levels.clone();
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::codec::AudioEncoder;

    #[test]
    fn test_peer_volume_and_mute() {
        let peer = PeerJitterBuffer::new("peer1", 1.0).unwrap();
        assert_eq!(peer.get_volume(), 1.0);
        assert!(!peer.is_muted());

        peer.set_volume(1.75);
        assert_eq!(peer.get_volume(), 1.75);

        peer.set_volume(2.5); // clamps to 2.0
        assert_eq!(peer.get_volume(), 2.0);

        peer.set_muted(true);
        assert!(peer.is_muted());
    }

    #[test]
    fn test_mixer_output_and_soft_clipping() {
        let mixer = AudioMixer::new();
        let peer_a = mixer.add_or_update_peer("peer_a", 1.0);
        let peer_b = mixer.add_or_update_peer("peer_b", 1.5);

        let mut encoder = AudioEncoder::new().unwrap();

        // Synthesize test tone
        let test_pcm = [0.8f32; OPUS_FRAME_SAMPLES];
        let opus_data = encoder.encode(&test_pcm).unwrap();

        // Handle packets
        peer_a.handle_packet(1, &opus_data).unwrap();
        peer_b.handle_packet(1, &opus_data).unwrap();

        let mut output = [0.0f32; OPUS_FRAME_SAMPLES];
        let mut peer_levels = HashMap::new();

        mixer.mix_output(&mut output, &mut peer_levels);

        assert!(peer_levels.contains_key("peer_a"));
        assert!(peer_levels.contains_key("peer_b"));
        assert!(peer_levels["peer_a"] > 0.0);
        assert!(peer_levels["peer_b"] > 0.0);

        // Verify soft-clipping: all samples must remain within [-1.0, 1.0]
        for s in output.iter() {
            assert!(s.abs() <= 1.0, "Sample exceeded 1.0: {s}");
        }
    }

    #[test]
    fn test_plc_on_missing_packets() {
        let peer = PeerJitterBuffer::new("peer_plc", 1.0).unwrap();
        let mut encoder = AudioEncoder::new().unwrap();

        let test_pcm = [0.5f32; OPUS_FRAME_SAMPLES];
        let opus_data = encoder.encode(&test_pcm).unwrap();

        // Send seq 1
        peer.handle_packet(1, &opus_data).unwrap();

        // Skip seq 2, 3 and send seq 4 (triggering PLC for 2 lost frames)
        peer.handle_packet(4, &opus_data).unwrap();

        let mut out = [0.0f32; OPUS_FRAME_SAMPLES * 3];
        let rms = peer.pull_samples(&mut out);
        assert!(rms > 0.0);
    }

    #[test]
    fn test_deafened_produces_silence() {
        let mixer = AudioMixer::new();
        let peer = mixer.add_or_update_peer("peer_deaf", 1.0);
        let mut encoder = AudioEncoder::new().unwrap();
        let test_pcm = [0.5f32; OPUS_FRAME_SAMPLES];
        let opus_data = encoder.encode(&test_pcm).unwrap();
        peer.handle_packet(1, &opus_data).unwrap();

        mixer.set_deafened(true);
        let mut output = [1.0f32; OPUS_FRAME_SAMPLES];
        let mut peer_levels = HashMap::new();
        mixer.mix_output(&mut output, &mut peer_levels);

        for s in output.iter() {
            assert_eq!(*s, 0.0);
        }
        assert_eq!(peer_levels["peer_deaf"], 0.0);
    }

    #[test]
    fn test_loopback_mixing() {
        let mixer = AudioMixer::new();
        assert!(!mixer.is_mic_test_enabled());

        let test_pcm = [0.3f32; OPUS_FRAME_SAMPLES];
        // When disabled, push does nothing
        mixer.push_loopback_samples(&test_pcm);

        let mut output = [0.0f32; OPUS_FRAME_SAMPLES];
        let mut peer_levels = HashMap::new();
        mixer.mix_output(&mut output, &mut peer_levels);
        assert_eq!(output[0], 0.0);

        // When enabled, push samples and mix
        mixer.set_mic_test(true);
        assert!(mixer.is_mic_test_enabled());
        mixer.push_loopback_samples(&test_pcm);

        mixer.mix_output(&mut output, &mut peer_levels);
        assert!(output[0] > 0.0);

        // Disabling clears buffer
        mixer.set_mic_test(false);
        assert!(!mixer.is_mic_test_enabled());
        mixer.mix_output(&mut output, &mut peer_levels);
        assert_eq!(output[0], 0.0);
    }
}
