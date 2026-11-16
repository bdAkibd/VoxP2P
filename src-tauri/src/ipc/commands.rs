use std::sync::Arc;
use tauri::State;
use crate::audio::{AudioDeviceInfo, AudioDevicesResponse, AudioManager};
use crate::network::node::{NodeManager, NodeStatus, UserProfile};

use crate::network::room::{RoomManager, RoomState};
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
pub fn list_audio_devices(
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<AudioDevicesResponse, String> {
    Ok(audio_manager.list_devices())
}

#[tauri::command]
pub fn list_audio_input_devices(
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<Vec<AudioDeviceInfo>, String> {
    Ok(audio_manager.list_devices().input_devices)
}

#[tauri::command]
pub fn list_audio_output_devices(
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<Vec<AudioDeviceInfo>, String> {
    Ok(audio_manager.list_devices().output_devices)
}

#[tauri::command]
pub fn set_input_device(
    name: String,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    let dev = if name.is_empty() || name == "default" {
        None
    } else {
        Some(name.as_str())
    };
    audio_manager.set_input_device(dev)
}

#[tauri::command]
pub fn set_output_device(
    name: String,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    let dev = if name.is_empty() || name == "default" {
        None
    } else {
        Some(name.as_str())
    };
    audio_manager.set_output_device(dev)
}

#[tauri::command]
pub fn set_mute(
    muted: bool,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    audio_manager.set_mute(muted);
    Ok(())
}

#[tauri::command]
pub fn set_deafen(
    deafened: bool,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    audio_manager.set_deafen(deafened);
    Ok(())
}

#[tauri::command]
pub fn set_peer_volume(
    node_id: String,
    volume: f32,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    if !(0.0..=2.0).contains(&volume) {
        return Err("Volume out of range [0.0, 2.0]".to_string());
    }
    audio_manager.set_peer_volume(&node_id, volume)
}

#[tauri::command]
pub fn set_microphone_gain(
    gain: f32,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    if !(0.0..=2.0).contains(&gain) {
        return Err("Gain out of range [0.0, 2.0]".to_string());
    }
    audio_manager.set_microphone_gain(gain);
    Ok(())
}

#[tauri::command]
pub fn set_master_volume(
    volume: f32,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    if !(0.0..=2.0).contains(&volume) {
        return Err("Volume out of range [0.0, 2.0]".to_string());
    }
    audio_manager.set_master_volume(volume);
    Ok(())
}


#[tauri::command]
pub async fn transfer_host(
    target_node_id: String,
    room_manager: State<'_, RoomManager>,
) -> Result<(), String> {
    room_manager.transfer_host(&target_node_id).await
}

#[tauri::command]
pub async fn get_room_state(
    room_manager: State<'_, RoomManager>,
) -> Result<Option<RoomState>, String> {
    room_manager.get_room_state().await
}

#[tauri::command]
pub async fn join_room(
    room_id: String,
    host_id: Option<String>,
    room_manager: State<'_, RoomManager>,
) -> Result<RoomState, String> {
    let session = room_manager
        .create_or_join_room(&room_id, host_id.as_deref())
        .await;
    let st = session.state.read().await;
    Ok(st.clone())
}

#[tauri::command]
pub async fn leave_room(room_manager: State<'_, RoomManager>) -> Result<(), String> {
    room_manager.leave_room().await;
    Ok(())
}
