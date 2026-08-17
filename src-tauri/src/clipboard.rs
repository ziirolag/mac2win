use crate::protocol::*;
use log::{debug, info, warn};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Clipboard synchronization manager.
/// Uses `arboard` for cross-platform clipboard access.
pub struct ClipboardManager {
    settings: Arc<RwLock<ClipboardSettings>>,
    local_sequence: Arc<RwLock<u64>>,
    remote_sequence: Arc<RwLock<u64>>,
    event_tx: broadcast::Sender<ClipboardEvent>,
    outbound_tx: mpsc::Sender<ClipboardContent>,
}

#[derive(Debug, Clone)]
pub enum ClipboardEvent {
    LocalChanged {
        content: ClipboardContent,
        sequence: u64,
    },
    RemoteReceived {
        content: ClipboardContent,
        sequence: u64,
    },
    SyncToggled { enabled: bool },
    Error(String),
}

impl ClipboardManager {
    pub fn new(
        settings: ClipboardSettings,
        outbound_tx: mpsc::Sender<ClipboardContent>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(32);
        Self {
            settings: Arc::new(RwLock::new(settings)),
            local_sequence: Arc::new(RwLock::new(0)),
            remote_sequence: Arc::new(RwLock::new(0)),
            event_tx,
            outbound_tx,
        }
    }

    /// Start the clipboard polling loop.
    pub async fn start_polling(&self) {
        let settings = self.settings.clone();
        let local_seq = self.local_sequence.clone();
        let event_tx = self.event_tx.clone();
        let outbound_tx = self.outbound_tx.clone();
        let mut last_hash: Option<u64> = None;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let current_settings = settings.read().await;
                if !current_settings.enabled {
                    continue;
                }

                match read_local_clipboard() {
                    Some((content, content_hash)) => {
                        if Some(content_hash) != last_hash {
                            let mut seq = local_seq.write().await;
                            *seq += 1;
                            let sequence = *seq;
                            drop(seq);
                            drop(current_settings);
                            last_hash = Some(content_hash);
                            debug!("Local clipboard changed (seq={})", sequence);
                            let _ = event_tx.send(ClipboardEvent::LocalChanged {
                                content: content.clone(),
                                sequence,
                            });
                            let _ = outbound_tx.send(content).await;
                        }
                    }
                    None => {
                        last_hash = None;
                    }
                }
            }
        });
    }

    /// Handle incoming clipboard content from the peer.
    pub async fn handle_incoming(
        &self,
        content: ClipboardContent,
        sequence: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let settings = self.settings.read().await;
        if !settings.enabled || matches!(&settings.direction, ClipboardDirection::LocalToRemote) {
            return Ok(());
        }

        let content_size = match &content {
            ClipboardContent::Text(s) => s.len(),
            ClipboardContent::RichText { html, plain } => html.len() + plain.len(),
            ClipboardContent::Image { data, .. } => data.len(),
            ClipboardContent::FileList { paths } => paths.iter().map(|p| p.len()).sum(),
        };
        if content_size > settings.max_content_size as usize {
            warn!("Clipboard content too large ({} bytes)", content_size);
            return Ok(());
        }
        drop(settings);

        let mut remote_seq = self.remote_sequence.write().await;
        if sequence <= *remote_seq {
            return Ok(());
        }
        *remote_seq = sequence;
        drop(remote_seq);

        info!("Applying incoming clipboard (seq={})", sequence);
        write_local_clipboard(&content)?;
        let _ = self.event_tx.send(ClipboardEvent::RemoteReceived { content, sequence });
        Ok(())
    }

    pub async fn update_settings(&self, new_settings: ClipboardSettings) {
        let mut settings = self.settings.write().await;
        let was_enabled = settings.enabled;
        *settings = new_settings.clone();
        drop(settings);
        if was_enabled != new_settings.enabled {
            let _ = self.event_tx.send(ClipboardEvent::SyncToggled { enabled: new_settings.enabled });
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ClipboardEvent> {
        self.event_tx.subscribe()
    }
}

/// Read the local clipboard via arboard.
fn read_local_clipboard() -> Option<(ClipboardContent, u64)> {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => {
            if let Ok(text) = clipboard.get_text() {
                if !text.is_empty() {
                    let hash = hash_str(&text);
                    return Some((ClipboardContent::Text(text), hash));
                }
            }
            if let Ok(img) = clipboard.get_image() {
                let width = img.width as u32;
                let height = img.height as u32;
                let rgba_data: Vec<u8> = img.bytes.chunks(4)
                    .flat_map(|bgra| {
                        if bgra.len() >= 4 { vec![bgra[2], bgra[1], bgra[0], bgra[3]] } else { vec![0,0,0,0] }
                    })
                    .collect();
                if let Some(rgba_img) = image::RgbaImage::from_raw(width, height, rgba_data) {
                    let mut buf = std::io::Cursor::new(Vec::new());
                    if rgba_img.write_to(&mut buf, image::ImageFormat::Png).is_ok() {
                        let data = buf.into_inner();
                        let hash = hash_bytes(&data);
                        return Some((ClipboardContent::Image { data, width, height }, hash));
                    }
                }
            }
            None
        }
        Err(e) => { warn!("Clipboard open failed: {}", e); None }
    }
}

/// Write content to the local clipboard via arboard.
fn write_local_clipboard(content: &ClipboardContent) -> Result<(), Box<dyn std::error::Error>> {
    let mut clipboard = arboard::Clipboard::new()?;
    match content {
        ClipboardContent::Text(text) => { clipboard.set_text(text.as_str())?; }
        ClipboardContent::RichText { plain, .. } => { clipboard.set_text(plain.as_str())?; }
        ClipboardContent::Image { data, .. } => {
            if let Ok(img) = image::load_from_memory(data) {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let img_data = arboard::ImageData { width: w as usize, height: h as usize, bytes: rgba.into_raw().into() };
                clipboard.set_image(img_data)?;
            }
        }
        ClipboardContent::FileList { paths } => { clipboard.set_text(&paths.join("\n"))?; }
    }
    Ok(())
}

fn hash_str(s: &str) -> u64 { let mut h = DefaultHasher::new(); s.hash(&mut h); h.finish() }
fn hash_bytes(d: &[u8]) -> u64 { let mut h = DefaultHasher::new(); d.hash(&mut h); h.finish() }
