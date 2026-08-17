pub mod protocol;
pub mod discovery;
pub mod connection;
pub mod clipboard;
pub mod audio;

use log::info;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    info!("Mac2Win starting up…");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_audio_devices,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn get_status() -> String {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
    .to_string()
}

#[tauri::command]
fn list_audio_devices() -> Vec<crate::protocol::AudioDevice> {
    crate::audio::list_devices()
}
