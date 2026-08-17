use crate::protocol::*;
use cpal::traits::{HostTrait, DeviceTrait};
use log::info;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Audio capture and streaming manager.
/// Uses `cpal` for cross-platform audio I/O.
pub struct AudioManager {
    settings: Arc<RwLock<AudioSettings>>,
    event_tx: broadcast::Sender<AudioEvent>,
    outbound_tx: mpsc::Sender<(AudioFrameHeader, Vec<u8>)>,
}

#[derive(Debug, Clone)]
pub enum AudioEvent {
    CapturedFrame { header: AudioFrameHeader, data: Vec<u8> },
    PlaybackFrame { header: AudioFrameHeader, data: Vec<u8> },
    AudioStreamToggled { enabled: bool },
    Error(String),
}

impl AudioManager {
    pub fn new(
        settings: AudioSettings,
        outbound_tx: mpsc::Sender<(AudioFrameHeader, Vec<u8>)>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            settings: Arc::new(RwLock::new(settings)),
            event_tx,
            outbound_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AudioEvent> {
        self.event_tx.subscribe()
    }

    pub async fn update_settings(&self, new_settings: AudioSettings) {
        let mut settings = self.settings.write().await;
        let was_enabled = settings.enabled;
        *settings = new_settings.clone();
        drop(settings);
        if was_enabled != new_settings.enabled {
            let _ = self.event_tx.send(AudioEvent::AudioStreamToggled { enabled: new_settings.enabled });
            info!("Audio {}", if new_settings.enabled { "enabled" } else { "disabled" });
        }
    }
}

/// List all available audio devices using cpal.
pub fn list_devices() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    if let Ok(outs) = host.output_devices() {
        for (i, d) in outs.enumerate() {
            let name = d.name().unwrap_or_else(|_| format!("Output {}", i));
            let is_def = host.default_output_device().map(|x| x.name().ok() == Some(name.clone())).unwrap_or(false);
            devices.push(AudioDevice { id: name.clone(), name, is_default: is_def, role: AudioDeviceRole::Output });
        }
    }
    if let Ok(ins) = host.input_devices() {
        for (i, d) in ins.enumerate() {
            let name = d.name().unwrap_or_else(|_| format!("Input {}", i));
            let is_def = host.default_input_device().map(|x| x.name().ok() == Some(name.clone())).unwrap_or(false);
            devices.push(AudioDevice { id: name.clone(), name, is_default: is_def, role: AudioDeviceRole::Input });
        }
    }

    devices
}
