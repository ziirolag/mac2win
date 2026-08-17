import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface ClipboardSettings {
  enabled: boolean;
  direction: 'LocalToRemote' | 'RemoteToLocal' | 'Bidirectional';
  max_content_size: number;
  sync_images: boolean;
  sync_files: boolean;
}

interface AudioSettings {
  enabled: boolean;
  direction: 'LocalToRemote' | 'RemoteToLocal' | 'Bidirectional';
  output_device_id: string | null;
  input_device_id: string | null;
  capture_system_audio: boolean;
  bitrate_kbps: number;
  echo_cancellation: boolean;
}

interface AudioDevice {
  id: string;
  name: string;
  is_default: boolean;
  role: 'Output' | 'Input' | 'SystemCapture';
}

export function Settings() {
  const [clipboard, setClipboard] = useState<ClipboardSettings>({
    enabled: true,
    direction: 'Bidirectional',
    max_content_size: 10 * 1024 * 1024,
    sync_images: true,
    sync_files: false,
  });

  const [audio, setAudio] = useState<AudioSettings>({
    enabled: false,
    direction: 'LocalToRemote',
    output_device_id: null,
    input_device_id: null,
    capture_system_audio: true,
    bitrate_kbps: 128,
    echo_cancellation: true,
  });

  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [isSaving, setIsSaving] = useState(false);
  const [saveMessage, setSaveMessage] = useState('');

  useEffect(() => {
    loadSettings();
    loadAudioDevices();
  }, []);

  const loadSettings = async () => {
    try {
      const settings = await invoke<any>('get_settings');
      if (settings.clipboard) setClipboard(settings.clipboard);
      if (settings.audio) setAudio(settings.audio);
    } catch (e) {
      console.error('Failed to load settings:', e);
    }
  };

  const loadAudioDevices = async () => {
    try {
      const devices = await invoke<AudioDevice[]>('list_audio_devices');
      setAudioDevices(devices);
    } catch (e) {
      console.error('Failed to load audio devices:', e);
    }
  };

  const saveSettings = async () => {
    setIsSaving(true);
    setSaveMessage('');
    try {
      await invoke('save_settings', {
        clipboard,
        audio,
      });
      setSaveMessage('Settings saved successfully!');
      setTimeout(() => setSaveMessage(''), 3000);
    } catch (e) {
      setSaveMessage('Failed to save settings');
      console.error('Failed to save settings:', e);
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="settings-container">
      <h1>Settings</h1>

      {/* Clipboard Section */}
      <section className="settings-section">
        <h2>📋 Clipboard Sharing</h2>
        
        <div className="setting-row">
          <label>
            <input
              type="checkbox"
              checked={clipboard.enabled}
              onChange={(e) => setClipboard({ ...clipboard, enabled: e.target.checked })}
            />
            Enable Clipboard Sharing
          </label>
          <span className="setting-hint">
            Sync clipboard between machines for seamless copy-paste
          </span>
        </div>

        <div className="setting-row">
          <label>Direction:</label>
          <select
            value={clipboard.direction}
            onChange={(e) => setClipboard({ 
              ...clipboard, 
              direction: e.target.value as ClipboardSettings['direction'] 
            })}
            disabled={!clipboard.enabled}
          >
            <option value="Bidirectional">Both Ways (Recommended)</option>
            <option value="LocalToRemote">This Computer → Remote Only</option>
            <option value="RemoteToLocal">Remote → This Computer Only</option>
          </select>
        </div>

        <div className="setting-row">
          <label>
            <input
              type="checkbox"
              checked={clipboard.sync_images}
              onChange={(e) => setClipboard({ ...clipboard, sync_images: e.target.checked })}
              disabled={!clipboard.enabled}
            />
            Sync Images
          </label>
          <span className="setting-hint">
            Share screenshots and images via clipboard (uses more bandwidth)
          </span>
        </div>

        <div className="setting-row">
          <label>
            <input
              type="checkbox"
              checked={clipboard.sync_files}
              onChange={(e) => setClipboard({ ...clipboard, sync_files: e.target.checked })}
              disabled={!clipboard.enabled}
            />
            Sync File References
          </label>
          <span className="setting-hint">
            Share file paths between machines (files are not transferred, only paths)
          </span>
        </div>

        <div className="setting-row">
          <label>Max Clipboard Size:</label>
          <select
            value={clipboard.max_content_size}
            onChange={(e) => setClipboard({ 
              ...clipboard, 
              max_content_size: Number(e.target.value) 
            })}
            disabled={!clipboard.enabled}
          >
            <option value={1024 * 1024}>1 MB</option>
            <option value={5 * 1024 * 1024}>5 MB</option>
            <option value={10 * 1024 * 1024}>10 MB (Default)</option>
            <option value={50 * 1024 * 1024}>50 MB</option>
          </select>
        </div>
      </section>

      {/* Audio Section */}
      <section className="settings-section">
        <h2>🔊 Audio Routing</h2>
        
        <div className="setting-row">
          <label>
            <input
              type="checkbox"
              checked={audio.enabled}
              onChange={(e) => setAudio({ ...audio, enabled: e.target.checked })}
            />
            Enable Audio Streaming
          </label>
          <span className="setting-hint">
            Route audio between machines (requires audio devices on both ends)
          </span>
        </div>

        <div className="setting-row">
          <label>Audio Direction:</label>
          <select
            value={audio.direction}
            onChange={(e) => setAudio({ 
              ...audio, 
              direction: e.target.value as AudioSettings['direction'] 
            })}
            disabled={!audio.enabled}
          >
            <option value="LocalToRemote">This Computer → Remote (Speaker)</option>
            <option value="RemoteToLocal">Remote → This Computer (Capture)</option>
            <option value="Bidirectional">Both Ways (Walkie-Talkie)</option>
          </select>
        </div>

        <div className="setting-row">
          <label>
            <input
              type="checkbox"
              checked={audio.capture_system_audio}
              onChange={(e) => setAudio({ ...audio, capture_system_audio: e.target.checked })}
              disabled={!audio.enabled}
            />
            Capture System Audio
          </label>
          <span className="setting-hint">
            Capture all system audio output (not just microphone)
          </span>
        </div>

        {audio.direction === 'RemoteToLocal' || audio.direction === 'Bidirectional' ? (
          <div className="setting-row">
            <label>Output Device:</label>
            <select
              value={audio.output_device_id || ''}
              onChange={(e) => setAudio({ 
                ...audio, 
                output_device_id: e.target.value || null 
              })}
              disabled={!audio.enabled}
            >
              <option value="">Default Device</option>
              {audioDevices
                .filter(d => d.role === 'Output')
                .map(device => (
                  <option key={device.id} value={device.id}>
                    {device.name} {device.is_default ? '(Default)' : ''}
                  </option>
                ))
              }
            </select>
          </div>
        ) : null}

        {audio.direction === 'LocalToRemote' || audio.direction === 'Bidirectional' ? (
          <div className="setting-row">
            <label>Input Device:</label>
            <select
              value={audio.input_device_id || ''}
              onChange={(e) => setAudio({ 
                ...audio, 
                input_device_id: e.target.value || null 
              })}
              disabled={!audio.enabled}
            >
              <option value="">Default Device</option>
              {audioDevices
                .filter(d => d.role === 'Input')
                .map(device => (
                  <option key={device.id} value={device.id}>
                    {device.name} {device.is_default ? '(Default)' : ''}
                  </option>
                ))
              }
            </select>
          </div>
        ) : null}

        <div className="setting-row">
          <label>Audio Quality:</label>
          <select
            value={audio.bitrate_kbps}
            onChange={(e) => setAudio({ 
              ...audio, 
              bitrate_kbps: Number(e.target.value) 
            })}
            disabled={!audio.enabled}
          >
            <option value={32}>Low (32 kbps)</option>
            <option value={64}>Medium (64 kbps)</option>
            <option value={128}>High (128 kbps, Default)</option>
            <option value={256}>Very High (256 kbps)</option>
          </select>
        </div>

        <div className="setting-row">
          <label>
            <input
              type="checkbox"
              checked={audio.echo_cancellation}
              onChange={(e) => setAudio({ ...audio, echo_cancellation: e.target.checked })}
              disabled={!audio.enabled || audio.direction !== 'Bidirectional'}
            />
            Echo Cancellation
          </label>
          <span className="setting-hint">
            Reduce echo when using bidirectional audio
          </span>
        </div>
      </section>

      {/* Save Button */}
      <div className="settings-actions">
        <button 
          className="save-button"
          onClick={saveSettings}
          disabled={isSaving}
        >
          {isSaving ? 'Saving...' : 'Save Settings'}
        </button>
        {saveMessage && (
          <span className={`save-message ${saveMessage.includes('Failed') ? 'error' : 'success'}`}>
            {saveMessage}
          </span>
        )}
      </div>
    </div>
  );
}

export default Settings;
