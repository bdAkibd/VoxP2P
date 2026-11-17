use std::sync::Arc;
use tauri::State;
use crate::audio::{AudioDeviceInfo, AudioDevicesResponse, AudioManager};
use crate::network::file_transfer::{
    AttachFileResponse, DeviceSyncTicket, FileTransferManager, ImportSyncResult,
};
use crate::network::node::{NodeManager, NodeStatus, UserProfile};
use crate::network::room::{RoomGossipMessage, RoomManager, RoomState};
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
pub fn set_mic_test(
    enabled: bool,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    audio_manager.set_mic_test(enabled);
    Ok(())
}

#[tauri::command]
pub fn set_meter_active(
    active: bool,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    audio_manager.set_meter_active(active);
    Ok(())
}

#[tauri::command]
pub fn set_vad_threshold(
    threshold: f32,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    if !(0.0..=1.0).contains(&threshold) {
        return Err("Threshold out of range [0.0, 1.0]".to_string());
    }
    audio_manager.set_vad_threshold(threshold);
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
pub async fn get_room_ticket(
    room_manager: State<'_, RoomManager>,
) -> Result<Option<String>, String> {
    Ok(room_manager.get_room_ticket().await)
}

#[tauri::command]
pub async fn start_direct_call(
    recipient_node_id: String,
    room_manager: State<'_, RoomManager>,
    ft_manager: State<'_, Arc<FileTransferManager>>,
    node_manager: State<'_, NodeManager>,
) -> Result<RoomState, String> {
    let local_id = node_manager.get_node_id().await;
    let (first, second) = if local_id < recipient_node_id {
        (&local_id, &recipient_node_id)
    } else {
        (&recipient_node_id, &local_id)
    };
    let short1 = if first.len() >= 8 { &first[..8] } else { first };
    let short2 = if second.len() >= 8 { &second[..8] } else { second };
    let call_room_name = format!("call_{}_{}", short1, short2);

    let session = room_manager.create_or_join_room(&call_room_name, None).await;
    let st = session.state.read().await.clone();

    // If ticket was generated, send DirectCallRing over DM_ALPN to contact
    if let Some(ref ticket) = st.ticket {
        let ring = crate::network::file_transfer::DmPayload::DirectCallRing {
            caller_id: local_id.clone(),
            room_ticket: ticket.clone(),
        };
        let _ = ft_manager.send_direct_call_ring(&recipient_node_id, ring).await;

        // Also queue DM chat invite message for offline/fallback access
        let invite_msg = format!("📞 Incoming 1-on-1 Call: {}", ticket);
        let _ = ft_manager
            .send_direct_message(&recipient_node_id, &invite_msg, None, None)
            .await;
    }

    Ok(st)
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

#[tauri::command]
pub async fn attach_file(
    file_path: String,
    chat_id: Option<String>,
    text: Option<String>,
    ft_manager: State<'_, Arc<FileTransferManager>>,
    room_manager: State<'_, RoomManager>,
) -> Result<AttachFileResponse, String> {
    let resp = ft_manager.attach_file(&file_path, chat_id.clone(), text).await?;

    if let Some(ref target) = chat_id {
        if let Some(room) = room_manager.get_current_room().await {
            if &room.room_id == target {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;
                let gossip_msg = RoomGossipMessage::ChatMessage {
                    message_id: resp.id.clone(),
                    sender_id: room.local_node_id.clone(),
                    content: resp.text.clone(),
                    attachment_hash: Some(resp.attachment_hash.clone()),
                    attachment_name: Some(resp.attachment_name.clone()),
                    timestamp: now,
                };
                let _ = room.broadcast_tx.send(gossip_msg);
            }
        }
    }

    Ok(resp)
}

#[tauri::command]
pub async fn download_attachment(
    node_id: String,
    blob_hash: String,
    save_path: String,
    ft_manager: State<'_, Arc<FileTransferManager>>,
) -> Result<(), String> {
    ft_manager
        .download_attachment(&node_id, &blob_hash, &save_path)
        .await
}

#[tauri::command]
pub async fn send_room_message(
    content: String,
    attachment_hash: Option<String>,
    attachment_name: Option<String>,
    room_manager: State<'_, RoomManager>,
) -> Result<Message, String> {
    room_manager
        .send_room_chat(content, attachment_hash, attachment_name)
        .await
}

#[tauri::command]
pub async fn send_direct_message(
    recipient_node_id: String,
    content: String,
    attachment_hash: Option<String>,
    attachment_name: Option<String>,
    ft_manager: State<'_, Arc<FileTransferManager>>,
) -> Result<Message, String> {
    ft_manager
        .send_direct_message(
            &recipient_node_id,
            &content,
            attachment_hash,
            attachment_name,
        )
        .await
}

#[tauri::command]
pub async fn send_message(
    chat_id: String,
    content: String,
    attachment_hash: Option<String>,
    attachment_name: Option<String>,
    room_manager: State<'_, RoomManager>,
    ft_manager: State<'_, Arc<FileTransferManager>>,
) -> Result<Message, String> {
    let current_room_opt = room_manager.get_current_room().await;
    if let Some(room) = current_room_opt {
        if room.room_id == chat_id {
            return room.send_room_chat(content, attachment_hash, attachment_name).await;
        }
    }

    ft_manager
        .send_direct_message(&chat_id, &content, attachment_hash, attachment_name)
        .await
}

#[tauri::command]
pub fn get_messages(
    chat_id: String,
    limit: Option<usize>,
    db: State<'_, Database>,
) -> Result<Vec<Message>, String> {
    db.get_messages_for_chat(&chat_id, limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_rnnoise(
    enabled: bool,
    audio_manager: State<'_, Arc<AudioManager>>,
) -> Result<(), String> {
    audio_manager.set_rnnoise(enabled);
    Ok(())
}

#[tauri::command]
pub async fn set_bypass_vpn(
    bypass: bool,
    node_manager: State<'_, NodeManager>,
) -> Result<(), String> {
    node_manager.set_bypass_vpn(bypass).await;
    Ok(())
}

#[tauri::command]
pub async fn save_theme(
    theme_name: String,
    theme_json: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Manager;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
    let themes_file = app_data_dir.join("themes.json");

    let mut map: std::collections::HashMap<String, serde_json::Value> = if themes_file.exists() {
        let data = std::fs::read_to_string(&themes_file).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    let val: serde_json::Value =
        serde_json::from_str(&theme_json).map_err(|e| format!("Invalid theme JSON: {e}"))?;
    map.insert(theme_name, val);

    let serialized = serde_json::to_string_pretty(&map).map_err(|e| e.to_string())?;
    std::fs::write(&themes_file, serialized).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_saved_themes(app_handle: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let themes_file = app_data_dir.join("themes.json");
    if themes_file.exists() {
        std::fs::read_to_string(&themes_file).map_err(|e| e.to_string())
    } else {
        Ok("{}".to_string())
    }
}

#[tauri::command]
pub async fn sync_devices(
    ft_manager: State<'_, Arc<FileTransferManager>>,
) -> Result<DeviceSyncTicket, String> {
    ft_manager.sync_devices().await
}

#[tauri::command]
pub async fn import_sync_ticket(
    ticket: String,
    ft_manager: State<'_, Arc<FileTransferManager>>,
) -> Result<ImportSyncResult, String> {
    ft_manager.import_sync_ticket(&ticket).await
}
