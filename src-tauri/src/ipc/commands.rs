use tauri::State;
use crate::audio::capture::AudioDeviceInfo;
use crate::network::node::{NodeManager, NodeStatus, UserProfile};
use crate::storage::db::Database;
use crate::storage::models::{Contact, Message};

#[tauri::command]
pub async fn get_my_profile(node_manager: State<'_, NodeManager>) -> Result<UserProfile, String> {
    Ok(node_manager.get_my_profile().await)
}

#[tauri::command]
pub async fn update_profile(
    nickname: String,
    avatar_path: Option<String>,
    node_manager: State<'_, NodeManager>,
) -> Result<UserProfile, String> {
    node_manager.update_profile(nickname, avatar_path).await
}

#[tauri::command]
pub async fn add_contact(
    node_id: String,
    node_manager: State<'_, NodeManager>,
) -> Result<Contact, String> {
    node_manager.add_contact(node_id).await
}

#[tauri::command]
pub async fn get_node_status(node_manager: State<'_, NodeManager>) -> Result<NodeStatus, String> {
    Ok(node_manager.get_status().await)
}

#[tauri::command]
pub fn get_contacts(db: State<'_, Database>) -> Result<Vec<Contact>, String> {
    db.get_contacts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_undelivered_messages(
    node_id: String,
    db: State<'_, Database>,
) -> Result<Vec<Message>, String> {
    db.get_undelivered_messages(&node_id).map_err(|e| e.to_string())
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
