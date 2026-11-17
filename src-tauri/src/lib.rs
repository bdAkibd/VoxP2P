pub mod audio;
pub mod ipc;
pub mod network;
pub mod storage;

use std::sync::Arc;
use tauri::Manager;
use audio::AudioManager;
use ipc::commands::{
    add_contact,
    attach_file,
    download_attachment,
    get_contacts,
    get_messages,
    get_my_profile,
    get_node_status,
    get_room_state,
    get_room_ticket,
    get_saved_themes,
    get_undelivered_messages,
    import_sync_ticket,
    join_room,
    leave_room,
    list_audio_devices,
    list_audio_input_devices,
    list_audio_output_devices,
    save_theme,
    send_direct_message,
    send_message,
    send_room_message,
    set_bypass_vpn,
    set_deafen,
    set_input_device,
    set_master_volume,
    set_microphone_gain,
    set_mute,
    set_output_device,
    set_peer_volume,
    set_rnnoise,
    start_direct_call,
    sync_devices,
    transfer_host,
    update_profile,
};

use network::file_transfer::FileTransferManager;
use network::node::NodeManager;
use network::room::RoomManager;
use storage::db::Database;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            std::fs::create_dir_all(&app_data_dir).ok();

            let db_path = app_data_dir.join("voxp2p.db");
            let db = Database::new(db_path).expect("failed to initialize sqlite database");
            let node_manager =
                NodeManager::new(db.clone()).expect("failed to initialize node manager");
            let room_manager = RoomManager::new(
                node_manager.clone(),
                db.clone(),
                Some(app.handle().clone()),
            );

            let audio_manager = AudioManager::new(db.clone(), Some(app.handle().clone()));

            let ft_manager = FileTransferManager::with_callbacks(
                db.clone(),
                app_data_dir.clone(),
                Some({
                    let app_handle = app.handle().clone();
                    Arc::new(move |p: &crate::ipc::events::TransferProgressPayload| {
                        use tauri::Emitter;
                        let _ = app_handle.emit(crate::ipc::events::EVENT_TRANSFER_PROGRESS, p);
                    })
                }),
                Some({
                    let app_handle = app.handle().clone();
                    Arc::new(move |m: &crate::storage::models::Message| {
                        use tauri::Emitter;
                        let _ = app_handle.emit(
                            crate::ipc::events::EVENT_CHAT_MESSAGE,
                            crate::ipc::events::ChatMessagePayload {
                                message: m.clone(),
                            },
                        );
                    })
                }),
                Some(app.handle().clone()),
            );

            // Register DM handler on node manager
            let dm_handler = ft_manager.create_dm_handler();
            let nm_dm = node_manager.clone();
            tauri::async_runtime::spawn(async move {
                nm_dm.set_dm_handler(dm_handler).await;
            });

            // Wire avatar handshake caching to room manager
            let rm_avatar = room_manager.clone();
            let ft_avatar = ft_manager.clone();
            tauri::async_runtime::spawn(async move {
                rm_avatar
                    .set_avatar_cb(Arc::new(move |peer_id, hash| {
                        let ft = ft_avatar.clone();
                        let pid = peer_id.to_string();
                        let h = hash.to_string();
                        tauri::async_runtime::spawn(async move {
                            ft.handle_avatar_handshake(&pid, &h).await;
                        });
                    }))
                    .await;
            });

            let nm_clone = node_manager.clone();
            let ft_clone = ft_manager.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = nm_clone.start().await {
                    eprintln!("Warning: Background P2P node start failed: {e}");
                } else if let (Some(ep), Some(store)) = (nm_clone.get_endpoint().await, nm_clone.get_blobs_store().await) {
                    let node_id = nm_clone.get_node_id().await;
                    ft_clone.set_endpoint_and_store(ep, store, node_id).await;
                }
            });

            let am_clone = audio_manager.clone();
            let nm_audio = node_manager.clone();
            tauri::async_runtime::spawn(async move {
                let node_id = nm_audio.get_node_id().await;
                am_clone.set_local_node_id(&node_id);
                if let Err(e) = am_clone.start() {
                    eprintln!("Warning: Audio pipeline start failed: {e}");
                }
            });

            // Start offline queue watcher to automatically flush pending DMs when contacts connect
            ft_manager.start_offline_queue_watcher();

            app.manage(db);
            app.manage(node_manager);
            app.manage(room_manager);
            app.manage(audio_manager);
            app.manage(ft_manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_my_profile,
            update_profile,
            add_contact,
            get_node_status,
            get_contacts,
            get_undelivered_messages,
            list_audio_devices,
            list_audio_input_devices,
            list_audio_output_devices,
            set_input_device,
            set_output_device,
            set_mute,
            set_deafen,
            set_peer_volume,
            set_microphone_gain,
            set_master_volume,
            transfer_host,
            get_room_state,
            get_room_ticket,
            join_room,
            leave_room,
            start_direct_call,
            attach_file,
            download_attachment,
            send_room_message,
            send_direct_message,
            send_message,
            get_messages,
            set_rnnoise,
            set_bypass_vpn,
            save_theme,
            get_saved_themes,
            sync_devices,
            import_sync_ticket
        ])

        .run(tauri::generate_context!())
        .expect("error while running voxp2p application");
}
