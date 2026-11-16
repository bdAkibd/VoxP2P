pub mod audio;
pub mod ipc;
pub mod network;
pub mod storage;

use ipc::commands::{
    get_node_status,
    list_audio_input_devices,
    list_audio_output_devices,
    set_master_volume,
    set_microphone_gain,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_node_status,
            list_audio_input_devices,
            list_audio_output_devices,
            set_microphone_gain,
            set_master_volume
        ])
        .run(tauri::generate_context!())
        .expect("error while running voxp2p application");
}
