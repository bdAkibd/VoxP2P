use crate::audio::capture::AudioDeviceInfo;
use crate::network::node::NodeStatus;

#[tauri::command]
pub fn get_node_status() -> Result<NodeStatus, String> {
    Ok(NodeStatus::Stopped)
}

#[tauri::command]
pub fn list_audio_input_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    Ok(vec![AudioDeviceInfo {
        id: "default_in".to_string(),
        name: "Default System Microphone".to_string(),
        is_default: true,
    }])
}

#[tauri::command]
pub fn list_audio_output_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    Ok(vec![AudioDeviceInfo {
        id: "default_out".to_string(),
        name: "Default System Speaker".to_string(),
        is_default: true,
    }])
}

#[tauri::command]
pub fn set_microphone_gain(gain: f32) -> Result<(), String> {
    if !(0.0..=2.0).contains(&gain) {
        return Err("Gain out of range [0.0, 2.0]".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn set_master_volume(volume: f32) -> Result<(), String> {
    if !(0.0..=2.0).contains(&volume) {
        return Err("Volume out of range [0.0, 2.0]".to_string());
    }
    Ok(())
}
