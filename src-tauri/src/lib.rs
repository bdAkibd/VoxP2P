pub mod audio;
pub mod ipc;
pub mod network;
pub mod storage;

use tauri::Manager;
use ipc::commands::{
    add_contact,
    get_contacts,
    get_my_profile,
    get_node_status,
    get_undelivered_messages,
    list_audio_input_devices,
    list_audio_output_devices,
    set_master_volume,
    set_microphone_gain,
    update_profile,
};
use network::node::NodeManager;
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

            let nm_clone = node_manager.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = nm_clone.start().await {
                    eprintln!("Warning: Background P2P node start failed: {e}");
                }
            });

            app.manage(db);
            app.manage(node_manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_my_profile,
            update_profile,
            add_contact,
            get_node_status,
            get_contacts,
            get_undelivered_messages,
            list_audio_input_devices,
            list_audio_output_devices,
            set_microphone_gain,
            set_master_volume
        ])
        .run(tauri::generate_context!())
        .expect("error while running voxp2p application");
}
