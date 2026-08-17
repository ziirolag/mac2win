pub mod protocol;
pub mod discovery;
pub mod connection;
pub mod clipboard;
pub mod audio;

use log::{info, error};
use std::sync::Arc;
use tokio::sync::RwLock;
use tauri::Emitter;
use protocol::*;

pub struct AppState {
    pub settings: Arc<RwLock<AppSettings>>,
    pub connected_peer: Arc<RwLock<Option<PeerInfo>>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    info!("Mac2Win starting up…");

    let state = AppState {
        settings: Arc::new(RwLock::new(AppSettings::default())),
        connected_peer: Arc::new(RwLock::new(None)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_audio_devices,
            get_settings,
            save_settings,
            get_discovered_peers,
            connect_to_peer,
            disconnect,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            // NetworkDiscovery (mdns-sd ServiceDaemon) is not Send,
            // so we run it on a dedicated OS thread with its own tokio runtime.
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to create discovery runtime");
                rt.block_on(start_discovery_and_server(handle));
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Background task: start mDNS advertising + browsing, and TCP/UDP listeners.
async fn start_discovery_and_server(handle: tauri::AppHandle) {
    // Get local IP
    let local_ip = discovery::get_local_ip().unwrap_or_else(|| {
        "127.0.0.1".parse().unwrap()
    });
    info!("Local IP: {}", local_ip);

    // Build our peer info
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let local_peer = PeerInfo {
        hostname: hostname.clone(),
        os: OperatingSystem::current(),
        screen_width: 1920,
        screen_height: 1080,
        screen_scale: 1.0,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        control_port: protocol::CONTROL_PORT,
        video_port: protocol::VIDEO_PORT,
        input_port: protocol::INPUT_PORT,
        clipboard_port: protocol::CLIPBOARD_PORT,
        audio_port: protocol::AUDIO_PORT,
    };

    // Start mDNS discovery
    let mut disc_rx = None;
    match discovery::NetworkDiscovery::new(local_peer.clone(), local_ip) {
        Ok(disc) => {
            if let Err(e) = disc.start().await {
                error!("mDNS discovery failed to start: {}", e);
            } else {
                info!("mDNS discovery started");
                disc_rx = Some(disc.subscribe());
            }
        }
        Err(e) => {
            error!("Failed to create mDNS discovery: {}", e);
        }
    }

    // Start TCP/UDP listener
    match connection::ConnectionManager::new().await {
        Ok(mut conn_mgr) => {
            if let Err(e) = conn_mgr.start_listening().await {
                error!("Connection listener failed: {}", e);
            } else {
                info!("Connection listener started on TCP {}", protocol::CONTROL_PORT);
            }
        }
        Err(e) => {
            error!("Failed to create connection manager: {}", e);
        }
    }

    // Listen for peer discovery events and emit to frontend
    if let Some(mut rx) = disc_rx {
        while let Ok(event) = rx.recv().await {
            match event {
                discovery::DiscoveryEvent::PeerFound { id, info, addr } => {
                    info!("Discovered peer: {} at {}", info.hostname, addr);
                    let _ = handle.emit("peer-found", serde_json::json!({
                        "id": id,
                        "hostname": info.hostname,
                        "os": format!("{:?}", info.os),
                        "addr": addr.to_string(),
                    }));
                }
                discovery::DiscoveryEvent::PeerLost { id } => {
                    info!("Peer lost: {}", id);
                    let _ = handle.emit("peer-lost", serde_json::json!({
                        "id": id,
                    }));
                }
            }
        }
    }
}

// ── Tauri Commands ──────────────────────────────────────────────────

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
fn list_audio_devices() -> Vec<AudioDevice> {
    audio::list_devices()
}

#[tauri::command]
async fn get_settings(
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings.read().await;
    serde_json::to_string(&*settings).map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_settings(
    state: tauri::State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    let mut s = state.settings.write().await;
    *s = settings;
    Ok(())
}

#[tauri::command]
async fn get_discovered_peers() -> Result<String, String> {
    Ok("[]".to_string())
}

#[tauri::command]
async fn connect_to_peer(
    _state: tauri::State<'_, AppState>,
    _addr: String,
) -> Result<String, String> {
    // TODO: wire up actual connection via ConnectionManager
    Ok("connected".to_string())
}

#[tauri::command]
async fn disconnect(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut peer = state.connected_peer.write().await;
    *peer = None;
    Ok(())
}
