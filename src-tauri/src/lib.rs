pub mod audio;
pub mod ipc;
pub mod network;
pub mod storage;

#[cfg(desktop)]
fn setup_system_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton};
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::Manager;
    use tauri::Emitter;

    let open_item = MenuItem::with_id(app, "open", "Open VoxP2P", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let mute_item = MenuItem::with_id(app, "toggle_mute", "Toggle Mute", true, None::<&str>)?;
    let disconnect_item = MenuItem::with_id(app, "disconnect_call", "Disconnect Call", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&open_item, &sep1, &mute_item, &disconnect_item, &sep2, &quit_item],
    )?;

    let mute_item_clone = mute_item.clone();

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            match event.id().as_ref() {
                "open" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
                "toggle_mute" => {
                    if let Some(audio_manager) = app.try_state::<Arc<AudioManager>>() {
                        let is_muted = audio_manager.is_muted();
                        let next_mute = !is_muted;
                        audio_manager.set_mute(next_mute);
                        let _ = mute_item_clone.set_text(if next_mute {
                            "Unmute Microphone"
                        } else {
                            "Mute Microphone"
                        });
                        let _ = app.emit("audio://mute-toggled", next_mute);
                    }
                }
                "disconnect_call" => {
                    if let Some(room_manager) = app.try_state::<RoomManager>() {
                        let rm = room_manager.inner().clone();
                        tauri::async_runtime::spawn(async move {
                            rm.leave_room().await;
                        });
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;

    Ok(())
}

use std::sync::Arc;
use tauri::Manager;
use audio::AudioManager;
use ipc::commands::{
    add_contact,
    attach_file,
    attach_file_bytes,
    delete_contact,
    delete_message,
    download_attachment,
    edit_message,
    ensure_audio_started,
    get_attachment_preview,
    get_contacts,
    get_messages,
    get_my_profile,
    get_network_status,
    get_node_status,
    get_room_state,
    get_room_ticket,
    get_saved_themes,
    get_undelivered_messages,
    import_from_sync_qr,
    import_sync_ticket,
    join_room,
    leave_room,
    list_audio_devices,
    list_audio_input_devices,
    list_audio_output_devices,
    request_mic_permission,
    save_theme,
    send_direct_message,
    send_message,
    send_room_message,
    set_bypass_vpn,
    set_deafen,
    set_input_device,
    set_master_volume,
    set_meter_active,
    set_mic_test,
    set_microphone_gain,
    set_mute,
    set_output_device,
    set_peer_volume,
    set_rnnoise,
    set_vad_threshold,
    start_device_sync_host,
    start_direct_call,
    sync_devices,
    transfer_host,
    update_profile,
    update_profile_avatar,
    get_avatar,
    get_app_settings,
    set_app_setting,
    start_call_foreground_service,
    stop_call_foreground_service,
};

use network::device_sync::DeviceSyncManager;
use network::file_transfer::FileTransferManager;
use network::node::NodeManager;
use network::room::RoomManager;
use storage::db::Database;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("[VoxP2P][PANIC] {panic_info}");
    }));
    eprintln!("[VoxP2P] Starting VoxP2P runtime");

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init());

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }));

    #[cfg(desktop)]
    let builder = builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window.hide();
        }
    });

    #[cfg(any(target_os = "android", target_os = "ios"))]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());

    builder
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            std::fs::create_dir_all(&app_data_dir).ok();
            eprintln!("[VoxP2P] Setup initialized with app_data_dir={}", app_data_dir.display());

            let db_path = app_data_dir.join("voxp2p.db");
            let db_key = Database::get_or_create_db_key(&app_data_dir);
            let db = Database::new_with_key(db_path, Some(&db_key)).expect("failed to initialize sqlite database");
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

            let (_, initial_node_id) = node_manager.get_identity_sync();
            ft_manager.set_local_node_id_sync(&initial_node_id);
            audio_manager.set_local_node_id(&initial_node_id);

            // Load saved settings before starting network/audio pipelines
            let mut bypass_vpn_setting: Option<bool> = None;
            if let Ok(settings) = db.get_all_settings() {
                if let Some(val) = settings.get("bypass_vpn") {
                    bypass_vpn_setting = Some(val == "true");
                }
                if let Some(val) = settings.get("rnnoise_enabled") {
                    audio_manager.set_rnnoise(val == "true");
                }
                if let Some(val) = settings.get("vad_threshold") {
                    if let Ok(thresh) = val.parse::<f32>() {
                        audio_manager.set_vad_threshold(thresh);
                    }
                }
                if let Some(val) = settings.get("input_device") {
                    if !val.is_empty() && val != "default" {
                        let _ = audio_manager.set_input_device(Some(val.as_str()));
                    }
                }
                if let Some(val) = settings.get("output_device") {
                    if !val.is_empty() && val != "default" {
                        let _ = audio_manager.set_output_device(Some(val.as_str()));
                    }
                }
            }

            if let Err(e) = audio_manager.start() {
                eprintln!("[VoxP2P] Warning: Audio pipeline start failed: {e}");
            } else {
                eprintln!("[VoxP2P] Audio pipeline started");
            }

            let dm_handler = ft_manager.create_dm_handler();
            let audio_handler = room_manager.create_audio_handler();

            let nm_clone = node_manager.clone();
            let ft_clone = ft_manager.clone();
            let rm_clone = room_manager.clone();
            let am_clone = audio_manager.clone();
            let app_handle_for_node = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                nm_clone.set_app_handle(app_handle_for_node).await;
                ft_clone.set_memory_lookup(nm_clone.get_memory_lookup().await).await;
                if let Some(bypass) = bypass_vpn_setting {
                    nm_clone.set_bypass_vpn(bypass).await;
                }
                rm_clone.set_audio_manager(am_clone).await;
                {
                    let ft_avatar = ft_clone.clone();
                    rm_clone
                        .set_avatar_cb(Arc::new(move |peer_id, hash| {
                            let ft = ft_avatar.clone();
                            let pid = peer_id.to_string();
                            let h = hash.to_string();
                            tauri::async_runtime::spawn(async move {
                                ft.handle_avatar_handshake(&pid, &h).await;
                            });
                        }))
                        .await;
                }

                // Register DM and Audio handlers BEFORE starting NodeManager/Router
                if let Err(e) = nm_clone.start_with_handlers(dm_handler, audio_handler).await {
                    eprintln!("[VoxP2P] Warning: Background P2P node start failed: {e}");
                } else if let (Some(ep), Some(store)) = (
                    nm_clone.get_endpoint().await,
                    nm_clone.get_blobs_store().await,
                ) {
                    let node_id = nm_clone.get_node_id().await;
                    eprintln!("[VoxP2P] Background P2P node started (node_id={node_id})");
                    ft_clone.set_endpoint_and_store(ep, store, node_id).await;
                    // Start outbox watcher only after endpoint, blobs_store, and real local_node_id are ready
                    ft_clone.start_offline_queue_watcher();
                }
            });

            let sync_manager = DeviceSyncManager::new(
                node_manager.clone(),
                db.clone(),
                app_data_dir.clone(),
                ft_manager.clone(),
                audio_manager.clone(),
                Some(app.handle().clone()),
                node_manager.get_sync_handler(),
            );

            app.manage(db);
            app.manage(node_manager);
            app.manage(room_manager);
            app.manage(audio_manager);
            app.manage(ft_manager);
            app.manage(sync_manager);

            #[cfg(desktop)]
            {
                if let Err(e) = setup_system_tray(app) {
                    eprintln!("Warning: Failed to setup system tray: {e}");
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_my_profile,
            update_profile,
            add_contact,
            get_node_status,
            get_network_status,
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
            set_mic_test,
            set_meter_active,
            set_vad_threshold,
            transfer_host,
            get_room_state,
            get_room_ticket,
            join_room,
            leave_room,
            start_direct_call,
            attach_file,
            attach_file_bytes,
            download_attachment,
            get_attachment_preview,
            send_room_message,
            send_direct_message,
            send_message,
            get_messages,
            set_rnnoise,
            set_bypass_vpn,
            save_theme,
            get_saved_themes,
            sync_devices,
            import_sync_ticket,
            start_device_sync_host,
            import_from_sync_qr,
            update_profile_avatar,
            get_avatar,
            get_app_settings,
            set_app_setting,
            edit_message,
            delete_message,
            delete_contact,
            start_call_foreground_service,
            stop_call_foreground_service,
            ensure_audio_started,
            request_mic_permission
        ])

        .run(tauri::generate_context!())
        .expect("error while running voxp2p application");
}
