pub mod capture;
pub mod codec;
pub mod mixer;
pub mod playback;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

pub use capture::*;
pub use codec::*;
pub use mixer::*;
pub use playback::*;

use serde::{Deserialize, Serialize};

use crate::ipc::events::EVENT_AUDIO_LEVELS;
use crate::network::room::AudioPacket;
use crate::storage::db::Database;

pub type AudioLevelsCallback = Arc<dyn Fn(&HashMap<String, f32>) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevicesResponse {
    pub input_devices: Vec<AudioDeviceInfo>,
    pub output_devices: Vec<AudioDeviceInfo>,
    pub input: Vec<AudioDeviceInfo>,
    pub output: Vec<AudioDeviceInfo>,
}

/// Central audio manager coordinating microphone capture, per-peer mixing,
/// hardware playback, and throttled 50ms volume levels emission.
pub struct AudioManager {
    pub capture: Arc<Mutex<AudioCapturePipeline>>,
    pub playback: Arc<Mutex<AudioPlaybackPipeline>>,
    pub mixer: Arc<AudioMixer>,
    pub db: Database,
    levels_callback: Option<AudioLevelsCallback>,
    is_running: Arc<AtomicBool>,
    levels_cancel: Arc<AtomicBool>,
    local_node_id: Arc<RwLock<String>>,
}

impl AudioManager {
    pub fn new(db: Database, app_handle: Option<tauri::AppHandle>) -> Arc<Self> {
        let levels_cb: Option<AudioLevelsCallback> = app_handle.map(|app| {
            let app_handle = app.clone();
            let cb: AudioLevelsCallback = Arc::new(move |levels: &HashMap<String, f32>| {
                use tauri::Emitter;
                let _ = app_handle.emit(EVENT_AUDIO_LEVELS, levels);
            });
            cb
        });
        Self::with_callback(db, levels_cb)
    }

    pub fn with_callback(
        db: Database,
        levels_cb: Option<AudioLevelsCallback>,
    ) -> Arc<Self> {
        let capture_pipeline = AudioCapturePipeline::new(AudioCaptureConfig::default());
        let mixer = Arc::new(AudioMixer::new());
        let playback_pipeline =
            AudioPlaybackPipeline::new(AudioPlaybackConfig::default(), mixer.clone());

        Arc::new(Self {
            capture: Arc::new(Mutex::new(capture_pipeline)),
            playback: Arc::new(Mutex::new(playback_pipeline)),
            mixer,
            db,
            levels_callback: levels_cb,
            is_running: Arc::new(AtomicBool::new(false)),
            levels_cancel: Arc::new(AtomicBool::new(false)),
            local_node_id: Arc::new(RwLock::new("local_node".to_string())),
        })
    }

    pub fn set_local_node_id(&self, node_id: &str) {
        {
            let mut id = self.local_node_id.write().unwrap();
            *id = node_id.to_string();
        }
        if let Ok(cap) = self.capture.lock() {
            cap.set_local_node_id(node_id.to_string());
        }
    }

    pub fn get_local_node_id(&self) -> String {
        let id = self.local_node_id.read().unwrap();
        id.clone()
    }

    pub fn set_packet_sender(&self, sender: Option<AudioPacketSender>) {
        if let Ok(cap) = self.capture.lock() {
            cap.set_packet_sender(sender);
        }
    }

    /// Starts both capture and playback streams, and launches the 50ms throttled levels task.
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
        if self.is_running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        // Attempt to start hardware capture if an input device exists
        if let Ok(mut cap) = self.capture.lock() {
            let _ = cap.start(None); // Ok to fail gracefully if no physical mic
        }

        // Attempt to start hardware playback if an output device exists
        if let Ok(mut play) = self.playback.lock() {
            let _ = play.start(None); // Ok to fail gracefully if no physical speaker
        }

        // Launch throttled 50ms volume levels emission task
        self.levels_cancel.store(false, Ordering::Release);
        self.spawn_levels_task();

        Ok(())
    }

    /// Spawns background Tokio task that samples RMS volume levels every 50ms and emits `audio://levels`.
    fn spawn_levels_task(self: &Arc<Self>) {
        let levels_cancel = self.levels_cancel.clone();
        let capture = self.capture.clone();
        let mixer = self.mixer.clone();
        let local_node_id = self.local_node_id.clone();
        let levels_cb = self.levels_callback.clone();

        let task_fn = async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
            while !levels_cancel.load(Ordering::Acquire) {
                interval.tick().await;

                let mut levels_map = HashMap::new();

                // 1. Local microphone volume level
                let local_id = {
                    let id = local_node_id.read().unwrap();
                    id.clone()
                };
                let mic_level = if let Ok(cap) = capture.lock() {
                    cap.get_mic_level()
                } else {
                    0.0
                };
                levels_map.insert(local_id, mic_level);

                // 2. Active peer volume levels from mixer
                let peer_levels = mixer.get_peer_levels();
                for (node_id, level) in peer_levels {
                    levels_map.insert(node_id, level);
                }

                // 3. Emit via callback closure if provided
                if let Some(ref cb) = levels_cb {
                    cb(&levels_map);
                }
            }
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(task_fn);
        } else {
            tauri::async_runtime::spawn(task_fn);
        }
    }


    pub fn stop(&self) {
        self.is_running.store(false, Ordering::Release);
        self.levels_cancel.store(true, Ordering::Release);

        if let Ok(mut cap) = self.capture.lock() {
            cap.stop();
        }
        if let Ok(mut play) = self.playback.lock() {
            play.stop();
        }
    }

    pub fn set_input_device(&self, name: Option<&str>) -> Result<(), String> {
        let mut cap = self
            .capture
            .lock()
            .map_err(|e| format!("Capture lock poisoned: {e}"))?;
        cap.stop();
        cap.start(name)
    }

    pub fn set_output_device(&self, name: Option<&str>) -> Result<(), String> {
        let mut play = self
            .playback
            .lock()
            .map_err(|e| format!("Playback lock poisoned: {e}"))?;
        play.stop();
        play.start(name)
    }

    pub fn set_mute(&self, muted: bool) {
        if let Ok(cap) = self.capture.lock() {
            cap.set_muted(muted);
        }
    }

    pub fn is_muted(&self) -> bool {
        if let Ok(cap) = self.capture.lock() {
            cap.is_muted()
        } else {
            false
        }
    }

    pub fn set_deafen(&self, deafened: bool) {
        if let Ok(play) = self.playback.lock() {
            play.set_deafened(deafened);
        }
        self.mixer.set_deafened(deafened);
    }

    pub fn is_deafened(&self) -> bool {
        self.mixer.is_deafened()
    }

    pub fn set_peer_volume(&self, node_id: &str, volume: f32) -> Result<(), String> {
        let clamped = volume.clamp(0.0, 2.0);
        self.mixer.add_or_update_peer(node_id, clamped);

        // Persist to contacts SQLite database table
        if let Err(e) = self.db.update_contact_volume(node_id, clamped as f64) {
            eprintln!("Warning: Failed to persist contact volume to database: {e}");
        }

        Ok(())
    }

    pub fn get_peer_volume(&self, node_id: &str) -> Option<f32> {
        self.mixer.get_peer_volume(node_id)
    }

    pub fn set_master_volume(&self, volume: f32) {
        if let Ok(play) = self.playback.lock() {
            play.set_volume(volume);
        }
        self.mixer.set_master_volume(volume);
    }

    pub fn get_master_volume(&self) -> f32 {
        self.mixer.get_master_volume()
    }

    pub fn set_microphone_gain(&self, gain: f32) {
        if let Ok(cap) = self.capture.lock() {
            cap.set_gain(gain);
        }
    }

    pub fn set_rnnoise(&self, enabled: bool) {
        if let Ok(cap) = self.capture.lock() {
            cap.set_noise_suppression(enabled);
        }
    }

    pub fn list_devices(&self) -> AudioDevicesResponse {
        let input_devices = list_input_devices();
        let output_devices = list_output_devices();

        AudioDevicesResponse {
            input: input_devices.clone(),
            output: output_devices.clone(),
            input_devices,
            output_devices,
        }
    }

    pub fn handle_incoming_audio_packet(&self, packet: &AudioPacket) -> Result<(), String> {
        // If peer exists in contacts, preload custom volume if not yet configured
        if self.mixer.get_peer_volume(&packet.sender_id).is_none() {
            let initial_vol = match self.db.get_contact(&packet.sender_id) {
                Ok(Some(contact)) => contact.custom_volume as f32,
                _ => 1.0,
            };
            self.mixer.add_or_update_peer(&packet.sender_id, initial_vol);
        }

        self.mixer.handle_audio_packet(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test_audio_manager_mute_deafen_and_peer_volume() {
        let db = Database::new_in_memory().unwrap();
        // Insert contact
        let contact = crate::storage::models::Contact {
            node_id: "peer_vol_test".to_string(),
            nickname: "Alice".to_string(),
            avatar_path: None,
            custom_volume: 1.0,
            last_seen: 0,
        };
        db.insert_or_update_contact(&contact).unwrap();

        let mgr = AudioManager::with_callback(db.clone(), None);

        // Initial states
        assert!(!mgr.is_muted());
        assert!(!mgr.is_deafened());

        // Toggle mute
        mgr.set_mute(true);
        assert!(mgr.is_muted());
        mgr.set_mute(false);
        assert!(!mgr.is_muted());

        // Toggle deafen
        mgr.set_deafen(true);
        assert!(mgr.is_deafened());
        mgr.set_deafen(false);
        assert!(!mgr.is_deafened());

        // Set peer volume and verify persistence to DB
        mgr.set_peer_volume("peer_vol_test", 1.6).unwrap();
        assert_eq!(mgr.get_peer_volume("peer_vol_test"), Some(1.6));

        let updated_contact = db.get_contact("peer_vol_test").unwrap().unwrap();
        assert!((updated_contact.custom_volume - 1.6).abs() < 1e-4);
    }


    #[test]
    fn test_audio_manager_devices_listing() {
        let db = Database::new_in_memory().unwrap();
        let mgr = AudioManager::with_callback(db, None);
        let devices = mgr.list_devices();
        assert!(!devices.input.is_empty());
        assert!(!devices.output.is_empty());
    }

    #[test]
    fn test_levels_callback_trigger() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let called = Arc::new(AtomicUsize::new(0));
            let called_clone = called.clone();

            let levels_cb: AudioLevelsCallback = Arc::new(move |levels| {
                if levels.contains_key("my_node") {
                    called_clone.fetch_add(1, Ordering::SeqCst);
                }
            });

            let mgr = AudioManager::with_callback(db, Some(levels_cb));
            mgr.set_local_node_id("my_node");
            mgr.spawn_levels_task();

            // Wait for 130ms (at least 2 ticks of 50ms)
            tokio::time::sleep(std::time::Duration::from_millis(130)).await;

            mgr.stop();
            assert!(
                called.load(Ordering::SeqCst) >= 1,
                "Levels callback should fire at least once"
            );
        });
    }

}
