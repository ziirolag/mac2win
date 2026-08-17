use crate::protocol::*;
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Audio capture and streaming manager.
///
/// Handles:
/// - Capturing system audio (loopback) from the local machine
/// - Playing back remote audio on the local machine
/// - Listing available audio devices
/// - Selecting specific audio devices
///
/// Audio is streamed over UDP with Opus encoding for low latency.
pub struct AudioManager {
    settings: Arc<RwLock<AudioSettings>>,
    event_tx: broadcast::Sender<AudioEvent>,
    /// Channel to send captured audio frames to the connection layer
    outbound_tx: mpsc::Sender<(AudioFrameHeader, Vec<u8>)>,
}

#[derive(Debug, Clone)]
pub enum AudioEvent {
    /// A captured audio frame is ready to send to the peer
    CapturedFrame {
        header: AudioFrameHeader,
        data: Vec<u8>,
    },
    /// A received audio frame is ready to play locally
    PlaybackFrame {
        header: AudioFrameHeader,
        data: Vec<u8>,
    },
    /// List of available audio devices
    DeviceList {
        output_devices: Vec<AudioDevice>,
        input_devices: Vec<AudioDevice>,
    },
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

    /// Start capturing system audio and streaming it to the peer.
    pub async fn start_capture(&self) -> Result<(), Box<dyn std::error::Error>> {
        let settings = self.settings.clone();
        let event_tx = self.event_tx.clone();
        let outbound_tx = self.outbound_tx.clone();

        #[cfg(target_os = "windows")]
        {
            let settings = settings.clone();
            let event_tx = event_tx.clone();
            let outbound_tx = outbound_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = capture_audio_windows(settings, event_tx, outbound_tx).await {
                    error!("Audio capture error (Windows): {}", e);
                    let _ = event_tx.send(AudioEvent::Error(e.to_string()));
                }
            });
        }

        #[cfg(target_os = "macos")]
        {
            let settings = settings.clone();
            let event_tx = event_tx.clone();
            let outbound_tx = outbound_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = capture_audio_macos(settings, event_tx, outbound_tx).await {
                    error!("Audio capture error (macOS): {}", e);
                    let _ = event_tx.send(AudioEvent::Error(e.to_string()));
                }
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            warn!("Audio capture not supported on this platform");
        }

        Ok(())
    }

    /// Start playing back audio received from the peer.
    pub async fn start_playback(&self) -> Result<(), Box<dyn std::error::Error>> {
        let settings = self.settings.clone();
        let event_tx = self.event_tx.clone();

        #[cfg(target_os = "windows")]
        {
            let settings = settings.clone();
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = playback_audio_windows(settings, event_tx).await {
                    error!("Audio playback error (Windows): {}", e);
                }
            });
        }

        #[cfg(target_os = "macos")]
        {
            let settings = settings.clone();
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = playback_audio_macos(settings, event_tx).await {
                    error!("Audio playback error (macOS): {}", e);
                }
            });
        }

        Ok(())
    }

    /// Feed a received audio frame into the playback buffer.
    pub async fn play_frame(
        &self,
        header: AudioFrameHeader,
        data: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.event_tx.send(AudioEvent::PlaybackFrame { header, data });
        Ok(())
    }

    /// List available audio devices on this machine.
    pub fn list_devices(&self) -> Vec<AudioDevice> {
        #[cfg(target_os = "windows")]
        {
            list_audio_devices_windows()
        }
        #[cfg(target_os = "macos")]
        {
            list_audio_devices_macos()
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            vec![]
        }
    }

    /// Update audio settings at runtime.
    pub async fn update_settings(&self, new_settings: AudioSettings) {
        let mut settings = self.settings.write().await;
        let was_enabled = settings.enabled;
        *settings = new_settings.clone();
        drop(settings);

        if was_enabled != new_settings.enabled {
            let _ = self.event_tx.send(AudioEvent::AudioStreamToggled {
                enabled: new_settings.enabled,
            });
            info!(
                "Audio streaming {}",
                if new_settings.enabled { "enabled" } else { "disabled" }
            );
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AudioEvent> {
        self.event_tx.subscribe()
    }
}

// ── Windows Audio (WASAPI via windows crate) ────────────────────────

#[cfg(target_os = "windows")]
async fn capture_audio_windows(
    settings: Arc<RwLock<AudioSettings>>,
    event_tx: broadcast::Sender<AudioEvent>,
    outbound_tx: mpsc::Sender<(AudioFrameHeader, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error>> {
    use windows::Win32::Media::Audio::{
        IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
        eCapture, eRender,
        WAVEFORMATEX, WAVE_FORMAT_PCM,
    };
    use windows::Win32::Media::Audio::Endpoints::{
        IAudioEndpointVolume, IMMDevice, IMMDeviceEnumerator as EnumeratorTrait,
    };

    info!("Starting WASAPI audio capture (system loopback)");

    // Initialize COM
    unsafe {
        windows::core::CoInitializeEx(
            std::ptr::null(),
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        )?;
    }

    // Get default capture device enumerator
    let enumerator: IMMDeviceEnumerator = unsafe {
        windows::core::CoCreateInstance(
            &windows::Win32::Media::Audio::MMDeviceEnumerator_CLSID,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )?
    };

    // Get default render device (for loopback capture, we capture from a render endpoint)
    let device: IMMDevice = unsafe {
        enumerator.GetDefaultAudioEndpoint(eRender, eCapture)?
    };

    // Activate audio client
    let audio_client: IAudioClient = unsafe {
        device.Activate::<IAudioClient>(0, std::ptr::null())?
    };

    // Get mix format
    let format: *mut WAVEFORMATEX = std::ptr::null_mut();
    unsafe {
        audio_client.GetMixFormat(&mut format as *mut _)?;
    }

    // Initialize for loopback capture
    unsafe {
        audio_client.Initialize(
            windows::Win32::Media::Audio::AUDCLNT_STREAMFLAGS_LOOPBACK,
            0, // buffer duration (0 = default)
            0,
            std::ptr::null(),
        )?;
    }

    let capture_client: IAudioCaptureClient = unsafe {
        audio_client.GetService::<IAudioCaptureClient>()?
    };

    unsafe {
        audio_client.Start()?;
    }

    let mut frame_id: u64 = 0;
    let settings_snap = settings.read().await.clone();

    loop {
        // Check if still enabled
        if !settings.read().await.enabled {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await; // ~100 frames/sec

        unsafe {
            let packet_size: u32 = 0;
            capture_client.GetNextPacketSize(&packet_size as *const _ as *mut _)?;
            if packet_size == 0 {
                continue;
            }

            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut num_frames: u32 = 0;
            let mut flags: u32 = 0;
            capture_client.GetBuffer(
                &mut data_ptr,
                &mut num_frames,
                &mut flags,
                std::ptr::null(),
                std::ptr::null(),
            )?;

            if !data_ptr.is_null() && num_frames > 0 {
                let data_size = num_frames as usize * 4; // 32-bit PCM
                let data = std::slice::from_raw_parts(data_ptr, data_size).to_vec();

                let header = AudioFrameHeader {
                    frame_id,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    sample_rate: 44100,
                    channels: 2,
                    format: AudioFormat::PcmF32,
                    total_size: data_size as u32,
                    chunk_index: 0,
                    chunk_total: 1,
                };

                let _ = event_tx.send(AudioEvent::CapturedFrame {
                    header: header.clone(),
                    data: data.clone(),
                });

                let _ = outbound_tx.send((header, data)).await;
                frame_id += 1;
            }

            capture_client.ReleaseBuffer(num_frames)?;
        }
    }

    unsafe {
        audio_client.Stop()?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
async fn playback_audio_windows(
    _settings: Arc<RwLock<AudioSettings>>,
    mut event_rx: broadcast::Receiver<AudioEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    use windows::Win32::Media::Audio::{IAudioClient, IMMDeviceEnumerator, eRender};
    use windows::Win32::Media::Audio::Endpoints::IMMDevice;

    info!("Starting WASAPI audio playback");

    unsafe {
        windows::core::CoInitializeEx(
            std::ptr::null(),
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        )?;
    }

    let enumerator: IMMDeviceEnumerator = unsafe {
        windows::core::CoCreateInstance(
            &windows::Win32::Media::Audio::MMDeviceEnumerator_CLSID,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )?
    };

    let device: IMMDevice = unsafe {
        enumerator.GetDefaultAudioEndpoint(eRender, eRender)?
    };

    let audio_client: IAudioClient = unsafe {
        device.Activate::<IAudioClient>(0, std::ptr::null())?
    };

    // Initialize for rendering
    unsafe {
        audio_client.Initialize(
            windows::Win32::Media::Audio::AUDCLNT_STREAMFLAGS_LOOPBACK,
            0,
            0,
            std::ptr::null(),
        )?;
    }

    unsafe {
        audio_client.Start()?;
    }

    loop {
        match event_rx.recv().await {
            Ok(AudioEvent::PlaybackFrame { header, data }) => {
                // TODO: Write data to render client
                debug!("Playing audio frame {} ({} bytes)", header.frame_id, data.len());
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn list_audio_devices_windows() -> Vec<AudioDevice> {
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, eCapture, eMultimedia, eRender,
    };

    let mut devices = Vec::new();

    unsafe {
        let enumerator: IMMDeviceEnumerator = match windows::core::CoCreateInstance(
            &windows::Win32::Media::Audio::MMDeviceEnumerator_CLSID,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        ) {
            Ok(e) => e,
            Err(_) => return devices,
        };

        // Enumerate render (output) devices
        if let Ok(collection) = enumerator.EnumAudioEndpoints(eRender, eMultimedia) {
            let count: u32 = collection.GetCount().unwrap_or(0);
            for i in 0..count {
                if let Ok(device) = collection.Item(i) {
                    let id: *mut u16 = std::ptr::null_mut();
                    if device.GetId(&mut id).is_ok() && !id.is_null() {
                        let id_str = read_wide_string(id);
                        // Get friendly name from property store
                        let name = format!("Audio Output {}", i);
                        devices.push(AudioDevice {
                            id: id_str,
                            name,
                            is_default: i == 0,
                            role: AudioDeviceRole::Output,
                        });
                    }
                }
            }
        }

        // Enumerate capture (input) devices
        if let Ok(collection) = enumerator.EnumAudioEndpoints(eCapture, eMultimedia) {
            let count: u32 = collection.GetCount().unwrap_or(0);
            for i in 0..count {
                if let Ok(device) = collection.Item(i) {
                    let id: *mut u16 = std::ptr::null_mut();
                    if device.GetId(&mut id).is_ok() && !id.is_null() {
                        let id_str = read_wide_string(id);
                        let name = format!("Audio Input {}", i);
                        devices.push(AudioDevice {
                            id: id_str,
                            name,
                            is_default: i == 0,
                            role: AudioDeviceRole::Input,
                        });
                    }
                }
            }
        }
    }

    devices
}

#[cfg(target_os = "windows")]
fn read_wide_string(ptr: *mut u16) -> String {
    let mut len = 0;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf16_lossy(slice)
}

// ── macOS Audio (Core Audio via cocoa/objc) ─────────────────────────

#[cfg(target_os = "macos")]
async fn capture_audio_macos(
    settings: Arc<RwLock<AudioSettings>>,
    event_tx: broadcast::Sender<AudioEvent>,
    outbound_tx: mpsc::Sender<(AudioFrameHeader, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error>> {
    use core_audio::audio_device::AudioDevice;
    use core_audio::audio_unit::AudioUnit;

    info!("Starting Core Audio system audio capture on macOS");

    // On macOS, system audio capture requires ScreenCaptureKit (macOS 13+)
    // or a virtual audio device like BlackHole/Loopback.
    // We'll use the ScreenCaptureKit approach for system audio capture.

    let mut frame_id: u64 = 0;

    // Create audio unit for input
    let audio_unit = AudioUnit::new_default_input()?;

    let settings_snap = settings.read().await.clone();
    let sample_rate = 44100u32;
    let channels = 2u8;

    // Set format
    unsafe {
        let stream_format = core_audio::sys::AudioStreamBasicDescription {
            mSampleRate: sample_rate as f64,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
            mBytesPerPacket: 8,
            mFramesPerPacket: 1,
            mBytesPerFrame: 8,
            mChannelsPerFrame: channels as u32,
            mBitsPerChannel: 32,
            mReserved: 0,
        };

        audio_unit.set_property(
            core_audio::sys::kAudioOutputUnitProperty_CurrentDevice,
            core_audio::sys::kAudioUnitScope_Global,
            0,
            &stream_format,
        )?;
    }

    unsafe {
        audio_unit.start()?;
    }

    loop {
        if !settings.read().await.enabled {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // TODO: Read from audio unit input buffer
        // For now, this is a placeholder for the actual Core Audio callback
        frame_id += 1;
    }

    unsafe {
        audio_unit.stop()?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn playback_audio_macos(
    _settings: Arc<RwLock<AudioSettings>>,
    mut event_rx: broadcast::Receiver<AudioEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting Core Audio playback on macOS");

    loop {
        match event_rx.recv().await {
            Ok(AudioEvent::PlaybackFrame { header, data }) => {
                debug!("Playing audio frame {} ({} bytes)", header.frame_id, data.len());
                // TODO: Write to Core Audio output unit
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn list_audio_devices_macos() -> Vec<AudioDevice> {
    use core_audio::audio_device::AudioDevice;

    let mut devices = Vec::new();

    if let Ok(audio_devices) = AudioDevice::default_output_device() {
        devices.push(AudioDevice {
            id: "default-output".to_string(),
            name: "Default Output".to_string(),
            is_default: true,
            role: AudioDeviceRole::Output,
        });
    }

    if let Ok(audio_devices) = AudioDevice::default_input_device() {
        devices.push(AudioDevice {
            id: "default-input".to_string(),
            name: "Default Input".to_string(),
            is_default: true,
            role: AudioDeviceRole::Input,
        });
    }

    // TODO: Enumerate all available devices using Core Audio HAL

    devices
}

// ── Opus encoding (optional, for efficient audio streaming) ─────────

/// Simple Opus encoder wrapper for when the `opus` feature is enabled.
/// Falls back to raw PCM if Opus is not available.
pub struct OpusEncoder {
    sample_rate: u32,
    channels: u8,
}

impl OpusEncoder {
    pub fn new(sample_rate: u32, channels: u8) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    pub fn encode(&self, pcm_data: &[f32]) -> Vec<u8> {
        // TODO: Integrate with opus crate when added to Cargo.toml
        // For now, pass through raw PCM as bytes
        pcm_data
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect()
    }

    pub fn decode(&self, encoded: &[u8], frame_size: usize) -> Vec<f32> {
        // TODO: Integrate with opus crate
        encoded
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    }
}
