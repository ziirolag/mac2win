use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Protocol version for backward compatibility
pub const PROTOCOL_VERSION: u32 = 1;

/// Service type for mDNS discovery
pub const SERVICE_TYPE: &str = "_mac2win._tcp.local.";

/// Default ports
pub const CONTROL_PORT: u16 = 51200;
pub const VIDEO_PORT: u16 = 51201;
pub const INPUT_PORT: u16 = 51202;
pub const CLIPBOARD_PORT: u16 = 51203;
pub const AUDIO_PORT: u16 = 51204;

/// Peer identity broadcast via mDNS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub hostname: String,
    pub os: OperatingSystem,
    pub screen_width: u32,
    pub screen_height: u32,
    pub screen_scale: f32,
    pub app_version: String,
    pub control_port: u16,
    pub video_port: u16,
    pub input_port: u16,
    pub clipboard_port: u16,
    pub audio_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperatingSystem {
    Windows,
    MacOS,
    Linux,
}

impl OperatingSystem {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            OperatingSystem::Windows
        } else if cfg!(target_os = "macos") {
            OperatingSystem::MacOS
        } else {
            OperatingSystem::Linux
        }
    }
}

// ── Control Messages (TCP) ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Initial handshake when connecting
    Handshake {
        version: u32,
        peer: PeerInfo,
        auth_token: [u8; 32],
    },
    /// Acknowledgment of handshake
    HandshakeAck {
        version: u32,
        peer: PeerInfo,
    },
    /// Request current screen info from peer
    RequestScreenInfo,
    /// Response with screen info
    ScreenInfo {
        displays: Vec<DisplayInfo>,
    },
    /// Notify the remote side to start/stop capturing
    StreamControl {
        enabled: bool,
        region: Option<Rect>,
    },
    /// Heartbeat / keepalive
    Ping { timestamp: u64 },
    /// Pong response
    Pong { timestamp: u64 },
    /// Session parameters
    SessionParams {
        quality: u8,        // 1-100 JPEG quality
        max_fps: u8,        // target max framerate
        target_resolution: Option<(u32, u32)>,
    },
    /// Disconnect gracefully
    Disconnect { reason: String },
    /// Error
    Error { message: String },

    // ── Clipboard sync ──────────────────────────────────────────────
    /// Enable/disable clipboard sharing
    ClipboardSyncControl {
        enabled: bool,
        direction: ClipboardDirection,
    },
    /// Clipboard content update from peer
    ClipboardUpdate {
        content: ClipboardContent,
        /// Sequence number to avoid duplicate processing
        sequence: u64,
    },
    /// Clipboard change notification (asks peer for current content)
    ClipboardPoll {
        sequence: u64,
    },

    // ── Audio routing ───────────────────────────────────────────────
    /// Enable/disable audio streaming
    AudioStreamControl {
        enabled: bool,
        direction: AudioDirection,
    },
    /// Audio session parameters
    AudioParams {
        sample_rate: u32,
        channels: u8,
        format: AudioFormat,
    },
    /// Request available audio devices on peer
    RequestAudioDevices,
    /// Response with available audio devices
    AudioDevices {
        devices: Vec<AudioDevice>,
    },
    /// Select a specific audio device on the peer
    SelectAudioDevice {
        device_id: String,
        role: AudioDeviceRole,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

// ── Video Frames (UDP) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFrameHeader {
    pub frame_id: u64,
    pub timestamp: u64,
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    pub region: Option<Rect>,
    pub is_delta: bool,
    pub total_size: u32,
    pub chunk_index: u16,
    pub chunk_total: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrameFormat {
    /// JPEG compressed frame (fallback)
    Jpeg,
    /// H.264 encoded frame (preferred, needs hardware encoder)
    H264,
    /// WebP compressed frame
    WebP,
}

// ── Input Events (TCP) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputEvent {
    MouseMove {
        x: f64,
        y: f64,
        /// Absolute or relative positioning
        absolute: bool,
    },
    MouseButton {
        button: MouseButton,
        pressed: bool,
    },
    MouseScroll {
        dx: i32,
        dy: i32,
    },
    KeyPress {
        key_code: u32,
        modifiers: KeyModifiers,
    },
    KeyRelease {
        key_code: u32,
    },
    KeyChar {
        character: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

// ── Screen Positioning ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScreenEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenPosition {
    /// Which edge of the local screen the remote display attaches to
    pub edge: ScreenEdge,
    /// Offset along that edge (in pixels, from top/left of the edge)
    pub offset: i32,
    /// Scale factor for the remote display
    pub scale: f32,
}

impl Default for ScreenPosition {
    fn default() -> Self {
        Self {
            edge: ScreenEdge::Right,
            offset: 0,
            scale: 1.0,
        }
    }
}

// ── Clipboard Types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClipboardDirection {
    /// Local → Remote only (one-way push)
    LocalToRemote,
    /// Remote → Local only (one-way pull)
    RemoteToLocal,
    /// Both directions (bidirectional, default)
    Bidirectional,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipboardContent {
    Text(String),
    RichText {
        html: String,
        plain: String,
    },
    Image {
        /// PNG-encoded image data
        data: Vec<u8>,
        width: u32,
        height: u32,
    },
    FileList {
        /// URIs / paths of files on the source machine
        paths: Vec<String>,
    },
}

/// Tracks what type of content is on the clipboard to avoid unnecessary transfers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardSnapshot {
    pub content_type: ClipboardContentType,
    pub sequence: u64,
    pub timestamp: u64,
    /// Size in bytes of the clipboard payload
    pub size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClipboardContentType {
    Empty,
    Text,
    RichText,
    Image,
    FileList,
}

// ── Audio Types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AudioDirection {
    /// Stream local machine's audio to remote (speaker forwarding)
    LocalToRemote,
    /// Receive remote machine's audio on local (capture forwarding)
    RemoteToLocal,
    /// Bidirectional audio
    Bidirectional,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AudioFormat {
    /// Raw PCM f32 samples
    PcmF32,
    /// Opus-encoded (preferred — low latency, small packets)
    Opus,
    /// AAC-encoded
    Aac,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub role: AudioDeviceRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AudioDeviceRole {
    Output,
    Input,
    SystemCapture,
}

// ── Audio Frames (UDP) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFrameHeader {
    pub frame_id: u64,
    pub timestamp: u64,
    pub sample_rate: u32,
    pub channels: u8,
    pub format: AudioFormat,
    pub total_size: u32,
    pub chunk_index: u16,
    pub chunk_total: u16,
}

// ── App Settings (persisted, exposed to Tauri commands) ─────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub screen_position: ScreenPosition,
    pub clipboard: ClipboardSettings,
    pub audio: AudioSettings,
    pub network: NetworkSettings,
    pub display: DisplaySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardSettings {
    pub enabled: bool,
    pub direction: ClipboardDirection,
    /// Maximum clipboard content size in bytes (default 10 MB)
    pub max_content_size: u32,
    /// Whether to sync images (can be bandwidth-heavy)
    pub sync_images: bool,
    /// Whether to sync files (sends file URIs, not content)
    pub sync_files: bool,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            direction: ClipboardDirection::Bidirectional,
            max_content_size: 10 * 1024 * 1024,
            sync_images: true,
            sync_files: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub enabled: bool,
    pub direction: AudioDirection,
    pub output_device_id: Option<String>,
    pub input_device_id: Option<String>,
    /// Whether to use system audio capture (loopback on Windows, system capture on macOS)
    pub capture_system_audio: bool,
    /// Audio quality: Opus bitrate in kbps (32-510)
    pub bitrate_kbps: u32,
    /// Enable echo cancellation when bidirectional
    pub echo_cancellation: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            direction: AudioDirection::LocalToRemote,
            output_device_id: None,
            input_device_id: None,
            capture_system_audio: true,
            bitrate_kbps: 128,
            echo_cancellation: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSettings {
    pub auto_discover: bool,
    pub port: u16,
    pub require_auth: bool,
    /// Pre-shared key for authentication (HMAC-SHA256)
    pub auth_key: Option<String>,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            auto_discover: true,
            port: CONTROL_PORT,
            require_auth: true,
            auth_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySettings {
    pub position: ScreenPosition,
    /// Target FPS for screen capture
    pub target_fps: u8,
    /// JPEG quality 1-100
    pub quality: u8,
    /// Scale factor override (None = auto-match remote screen DPI)
    pub scale_override: Option<f32>,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            position: ScreenPosition::default(),
            target_fps: 30,
            quality: 80,
            scale_override: None,
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            screen_position: ScreenPosition::default(),
            clipboard: ClipboardSettings::default(),
            audio: AudioSettings::default(),
            network: NetworkSettings::default(),
            display: DisplaySettings::default(),
        }
    }
}
