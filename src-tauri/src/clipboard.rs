use crate::protocol::*;
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Clipboard synchronization manager.
///
/// Polls the local clipboard for changes and notifies the peer.
/// Receives clipboard updates from the peer and writes them locally.
///
/// Platform-specific clipboard access is handled via `#[cfg]` blocks.
pub struct ClipboardManager {
    settings: Arc<RwLock<ClipboardSettings>>,
    local_sequence: Arc<RwLock<u64>>,
    remote_sequence: Arc<RwLock<u64>>,
    event_tx: broadcast::Sender<ClipboardEvent>,
    /// Channel to send clipboard content to the connection layer for transmission
    outbound_tx: mpsc::Sender<ClipboardContent>,
}

#[derive(Debug, Clone)]
pub enum ClipboardEvent {
    /// Local clipboard changed and needs to be sent to peer
    LocalChanged {
        content: ClipboardContent,
        sequence: u64,
    },
    /// Received clipboard content from peer
    RemoteReceived {
        content: ClipboardContent,
        sequence: u64,
    },
    /// Clipboard sync was enabled/disabled
    SyncToggled {
        enabled: bool,
    },
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
    /// This checks the local clipboard periodically and sends updates to the peer.
    pub async fn start_polling(&self) {
        let settings = self.settings.clone();
        let local_seq = self.local_sequence.clone();
        let event_tx = self.event_tx.clone();
        let outbound_tx = self.outbound_tx.clone();

        // Hash of the last known clipboard content to detect changes
        let mut last_hash: Option<u64> = None;

        tokio::spawn(async move {
            loop {
                // Check every 500ms for clipboard changes
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                let current_settings = settings.read().await;
                if !current_settings.enabled {
                    continue;
                }

                // Read the local clipboard
                match read_local_clipboard(&current_settings) {
                    Some((content, content_hash)) => {
                        if Some(content_hash) != last_hash {
                            // Clipboard changed!
                            let mut seq = local_seq.write().await;
                            *seq += 1;
                            let sequence = *seq;
                            drop(seq);
                            drop(current_settings);

                            last_hash = Some(content_hash);

                            debug!("Local clipboard changed (seq={}), sending to peer", sequence);

                            let _ = event_tx.send(ClipboardEvent::LocalChanged {
                                content: content.clone(),
                                sequence,
                            });

                            // Send to connection layer
                            if let Err(e) = outbound_tx.send(content).await {
                                error!("Failed to send clipboard content to outbound channel: {}", e);
                            }
                        }
                    }
                    None => {
                        // Clipboard is empty or unreadable
                        if last_hash.is_some() {
                            last_hash = None;
                        }
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
        if !settings.enabled {
            debug!("Ignoring incoming clipboard: sync disabled");
            return Ok(());
        }

        // Check direction
        match &settings.direction {
            ClipboardDirection::LocalToRemote => {
                debug!("Ignoring incoming clipboard: direction is LocalToRemote");
                return Ok(());
            }
            _ => {}
        }

        // Check size limit
        let content_size = match &content {
            ClipboardContent::Text(s) => s.len(),
            ClipboardContent::RichText { html, plain } => html.len() + plain.len(),
            ClipboardContent::Image { data, .. } => data.len(),
            ClipboardContent::FileList { paths } => paths.iter().map(|p| p.len()).sum(),
        };

        if content_size > settings.max_content_size as usize {
            warn!(
                "Ignoring incoming clipboard: content size {} exceeds max {}",
                content_size, settings.max_content_size
            );
            return Ok(());
        }

        // Check if this is a newer sequence
        let mut remote_seq = self.remote_sequence.write().await;
        if sequence <= *remote_seq {
            debug!(
                "Ignoring duplicate clipboard (seq={}, last={})",
                sequence, *remote_seq
            );
            return Ok(());
        }
        *remote_seq = sequence;
        drop(remote_seq);
        drop(settings);

        info!(
            "Applying incoming clipboard content (seq={}, type={:?})",
            sequence,
            std::mem::discriminant(&content)
        );

        // Write to local clipboard
        write_local_clipboard(&content)?;

        let _ = self.event_tx.send(ClipboardEvent::RemoteReceived {
            content,
            sequence,
        });

        Ok(())
    }

    /// Update clipboard sync settings at runtime.
    pub async fn update_settings(&self, new_settings: ClipboardSettings) {
        let mut settings = self.settings.write().await;
        let was_enabled = settings.enabled;
        *settings = new_settings.clone();
        drop(settings);

        if was_enabled != new_settings.enabled {
            let _ = self.event_tx.send(ClipboardEvent::SyncToggled {
                enabled: new_settings.enabled,
            });
            info!("Clipboard sync {}", if new_settings.enabled { "enabled" } else { "disabled" });
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ClipboardEvent> {
        self.event_tx.subscribe()
    }
}

// ── Platform-specific clipboard access ──────────────────────────────

/// Returns (content, hash) if clipboard has readable text content, None if empty.
fn read_local_clipboard(settings: &ClipboardSettings) -> Option<(ClipboardContent, u64)> {
    #[cfg(target_os = "windows")]
    {
        read_clipboard_windows(settings)
    }
    #[cfg(target_os = "macos")]
    {
        read_clipboard_macos(settings)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

fn write_local_clipboard(content: &ClipboardContent) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    {
        write_clipboard_windows(content)
    }
    #[cfg(target_os = "macos")]
    {
        write_clipboard_macos(content)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err("Unsupported platform for clipboard access".into())
    }
}

// ── Windows clipboard (Win32 API) ───────────────────────────────────

#[cfg(target_os = "windows")]
fn read_clipboard_windows(settings: &ClipboardSettings) -> Option<(ClipboardContent, u64)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    use windows::Win32::UI::WindowsAndMessaging::{
        CloseClipboard, GetClipboardData, OpenClipboard, CF_UNICODETEXT, CF_DIB,
    };

    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }

        // Try text first
        let text_handle = GetClipboardData(CF_UNICODETEXT.0 as u32);
        if let Ok(handle) = text_handle {
            if !handle.is_invalid() {
                let ptr = handle.0 as *const u16;
                if !ptr.is_null() {
                    let mut len = 0;
                    while *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    if let Ok(text) = String::from_utf16(slice) {
                        let hash = hash_content(&text);
                        CloseClipboard();
                        return Some((ClipboardContent::Text(text), hash));
                    }
                }
            }
        }

        // Try image if enabled
        if settings.sync_images {
            let img_handle = GetClipboardData(CF_DIB.0 as u32);
            if let Ok(handle) = img_handle {
                if !handle.is_invalid() {
                    let ptr = handle.0 as *const u8;
                    let mut size = 0;
                    while *ptr.add(size) != 0 {
                        size += 1;
                    }
                    if size > 0 {
                        // Convert DIB to PNG for efficient transfer
                        let slice = std::slice::from_raw_parts(ptr, size);
                        let dib_data = slice.to_vec();
                        // Hash the raw DIB data
                        let hash = hash_bytes(&dib_data);
                        CloseClipboard();
                        // TODO: Convert DIB to proper image format
                        // For now, store raw data with metadata
                        return Some((
                            ClipboardContent::Image {
                                data: dib_data,
                                width: 0, // Will be parsed from BITMAPINFOHEADER
                                height: 0,
                            },
                            hash,
                        ));
                    }
                }
            }
        }

        CloseClipboard();
        None
    }
}

#[cfg(target_os = "windows")]
fn write_clipboard_windows(content: &ClipboardContent) -> Result<(), Box<dyn std::error::Error>> {
    use windows::Win32::Foundation::{CloseHandle, GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::WindowsAndMessaging::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData, CF_UNICODETEXT,
    };

    match content {
        ClipboardContent::Text(text) | ClipboardContent::RichText { plain: text, .. } => {
            unsafe {
                if OpenClipboard(None).is_err() {
                    return Err("Failed to open clipboard".into());
                }

                EmptyClipboard();

                // Convert text to UTF-16
                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let size = wide.len() * std::mem::size_of::<u16>();

                let h_mem = GlobalAlloc(GMEM_MOVEABLE, size)?;
                let ptr = GlobalLock(h_mem);
                if ptr.is_null() {
                    GlobalUnlock(h_mem);
                    CloseClipboard();
                    return Err("Failed to lock memory".into());
                }

                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
                GlobalUnlock(h_mem);

                SetClipboardData(CF_UNICODETEXT.0 as u32, h_mem);
                CloseClipboard();
            }
            Ok(())
        }
        ClipboardContent::Image { data, .. } => {
            // TODO: Set clipboard image from PNG data
            warn!("Setting clipboard image not yet implemented on Windows");
            Ok(())
        }
        ClipboardContent::FileList { paths } => {
            warn!("Setting clipboard file list not yet implemented on Windows");
            Ok(())
        }
    }
}

// ── macOS clipboard (Core Graphics / AppKit via FFI) ────────────────

#[cfg(target_os = "macos")]
fn read_clipboard_macos(settings: &ClipboardSettings) -> Option<(ClipboardContent, u64)> {
    use cocoa::appkit::NSPasteboard;
    use cocoa::base::{id, nil};

    unsafe {
        let pasteboard: id = NSPasteboard::generalPasteboard(nil);
        if pasteboard.is_null() {
            return None;
        }

        let change_count = NSPasteboard::changeCount(pasteboard);
        if change_count == 0 {
            return None;
        }

        // Try string type first
        let string_type = cocoa::foundation::NSString::alloc(nil)
            .init_str("public.utf8-plain-text");
        let has_string: bool = msg_send![pasteboard, availableTypeForType: string_type];

        if has_string {
            let str_data: id = msg_send![pasteboard, stringForType: string_type];
            if !str_data.is_null() {
                let cstr: *const std::os::raw::c_char = msg_send![str_data, UTF8String];
                if !cstr.is_null() {
                    let text = std::ffi::CStr::from_ptr(cstr)
                        .to_string_lossy()
                        .to_string();
                    let hash = hash_content(&text);
                    return Some((ClipboardContent::Text(text), hash));
                }
            }
        }

        // Try image type
        if settings.sync_images {
            let image_type = cocoa::foundation::NSString::alloc(nil)
                .init_str("public.png");
            let has_image: bool = msg_send![pasteboard, availableTypeForType: image_type];

            if has_image {
                let img_data: id = msg_send![pasteboard, dataForType: image_type];
                if !img_data.is_null() {
                    let len: usize = msg_send![img_data, length];
                    let bytes: *const u8 = msg_send![img_data, bytes];
                    if !bytes.is_null() && len > 0 {
                        let data = std::slice::from_raw_parts(bytes, len).to_vec();
                        let hash = hash_bytes(&data);
                        return Some((
                            ClipboardContent::Image {
                                data,
                                width: 0,
                                height: 0,
                            },
                            hash,
                        ));
                    }
                }
            }
        }

        None
    }
}

#[cfg(target_os = "macos")]
fn write_clipboard_macos(content: &ClipboardContent) -> Result<(), Box<dyn std::error::Error>> {
    use cocoa::appkit::NSPasteboard;
    use cocoa::base::{id, nil};
    use cocoa::foundation::{NSArray, NSString};

    unsafe {
        let pasteboard: id = NSPasteboard::generalPasteboard(nil);
        if pasteboard.is_null() {
            return Err("Failed to get pasteboard".into());
        }

        NSPasteboard::clearContents(pasteboard);

        match content {
            ClipboardContent::Text(text) | ClipboardContent::RichText { plain: text, .. } => {
                let ns_string = NSString::alloc(nil).init_str(text);
                let string_type = NSString::alloc(nil).init_str("public.utf8-plain-text");
                let types = NSArray::arrayWithObject(nil, string_type);
                let _: bool = msg_send![pasteboard, writeObjects: types];

                let arr = NSArray::arrayWithObject(nil, ns_string);
                let _: bool = msg_send![pasteboard, writeObjects: arr];
                Ok(())
            }
            ClipboardContent::Image { data, .. } => {
                let bytes_ptr = data.as_ptr() as *const std::os::raw::c_void;
                let ns_data: id = msg_send![class!(NSData), dataWithBytes:bytes_ptr length:data.len()];
                let image_type = NSString::alloc(nil).init_str("public.png");
                let _: bool = msg_send![pasteboard, setData:ns_data forType:image_type];
                Ok(())
            }
            ClipboardContent::FileList { paths } => {
                warn!("Setting clipboard file list not yet implemented on macOS");
                Ok(())
            }
        }
    }
}

// ── Content hashing (for change detection) ──────────────────────────

fn hash_content(text: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn hash_bytes(data: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}
