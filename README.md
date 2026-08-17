# Mac2Win - Extended Display over Network

A cross-platform application that turns two machines (Windows 11 + macOS) into an extended display system over your local network. Control both machines from a single keyboard/mouse, share clipboards, and route audio between them.

## Features

### 🖥️ Extended Display
- View and control a remote machine's screen from your local machine
- Position the remote display relative to your local screens (left/right/top/bottom)
- Support for high-DPI displays with automatic scaling
- Configurable frame rate and quality

### 📋 Clipboard Sharing
- **Bidirectional clipboard sync** - Copy on one machine, paste on the other
- Three direction modes:
  - **Bidirectional** (default) - Both machines share clipboard
  - **Local → Remote** - Only push local clipboard to remote
  - **Remote → Local** - Only receive remote clipboard
- **Content types supported**:
  - Plain text
  - Rich text (HTML)
  - Images (PNG format)
  - File paths (references only, not file contents)
- **Configurable limits**:
  - Maximum clipboard size (1-50 MB)
  - Toggle image syncing on/off
  - Toggle file path syncing on/off

### 🔊 Audio Routing
- **Stream system audio** from one machine to another
- Three direction modes:
  - **Local → Remote** - Send your audio to the remote machine
  - **Remote → Local** - Capture remote audio on your machine
  - **Bidirectional** - Two-way audio (walkie-talkie style)
- **Audio features**:
  - System audio capture (loopback on Windows, ScreenCaptureKit on macOS)
  - Selectable input/output devices
  - Configurable audio quality (32-256 kbps)
  - Echo cancellation for bidirectional mode

### ⌨️ Input Control
- Seamless keyboard and mouse control across machines
- Mouse cursor movement, clicks, and scroll wheel
- Full keyboard support including modifiers (Ctrl, Alt, Shift, Meta)
- Cross-platform input translation (Windows API ↔ macOS CGEvent)

### 🔍 Network Discovery
- **Automatic peer discovery** via mDNS (Bonjour/Avahi)
- No manual IP entry required
- Service advertisement: `_mac2win._tcp.local.`
- Automatic connection when peers are on the same subnet

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Mac2Win App                              │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │  Clipboard  │  │    Audio    │  │   Display   │            │
│  │   Manager   │  │   Manager   │  │   Manager   │            │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘            │
│         │                │                │                    │
│  ┌──────▼──────────────▼──────────────▼──────┐               │
│  │           Connection Manager               │               │
│  │  ┌────────┐ ┌────────┐ ┌────────┐ ┌─────┐ │               │
│  │  │ Control│ │ Video  │ │Clipboard│ │Audio│ │               │
│  │  │  TCP   │ │  UDP   │ │  UDP   │ │ UDP │ │               │
│  │  │ :51200 │ │ :51201 │ │ :51203 │ │:51204│ │               │
│  │  └────────┘ └────────┘ └────────┘ └─────┘ │               │
│  └────────────────────────────────────────────┘               │
│                            │                                   │
│  ┌─────────────────────────▼─────────────────────────────┐    │
│  │              Network Discovery (mDNS)                  │    │
│  │         Service: _mac2win._tcp.local.                  │    │
│  └───────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Network Protocol

### Ports
| Port | Protocol | Purpose |
|------|----------|---------|
| 51200 | TCP | Control messages (handshake, settings, clipboard sync) |
| 51201 | UDP | Video frames (screen capture data) |
| 51202 | UDP | Input events (keyboard/mouse) |
| 51203 | UDP | Clipboard content |
| 51204 | UDP | Audio frames |

### Message Types

#### Control Messages (TCP)
- `Handshake` - Initial connection with peer info and auth token
- `ClipboardSyncControl` - Enable/disable clipboard sharing with direction
- `ClipboardUpdate` - Clipboard content from peer
- `AudioStreamControl` - Enable/disable audio streaming with direction
- `AudioParams` - Audio format parameters
- `RequestAudioDevices` - Query available audio devices

#### Video Frames (UDP)
- Chunked transmission for large frames
- JPEG/H.264/WebP encoding support
- Frame headers with dimensions, format, and sequence info

#### Input Events (UDP)
- Mouse position (absolute/relative)
- Mouse buttons (left/right/middle)
- Scroll wheel
- Key press/release with modifiers

## DHCP & Network Configuration

### How It Works with DHCP

1. **Automatic Discovery**: Mac2Win uses mDNS to broadcast its presence on the local network. When both machines are connected to the same network (via DHCP or static IP), they automatically discover each other.

2. **No Static IPs Required**: The mDNS service advertises using the machine's hostname, so you don't need to know IP addresses in advance.

3. **Subnet Requirements**: Both machines must be on the same subnet (typically the case when connected to the same router). If you have VLANs or complex network segmentation, you may need to adjust firewall rules.

4. **Firewall Configuration**: Windows and macOS firewalls may block mDNS and UDP traffic. You may need to:
   - **Windows**: Allow the app through Windows Defender Firewall
   - **macOS**: Allow the app through System Preferences → Security & Privacy → Firewall

### Network Architecture

```
┌──────────────────┐                    ┌──────────────────┐
│   Windows 11     │                    │     macOS        │
│   Machine        │                    │   Machine        │
├──────────────────┤                    ├──────────────────┤
│ IP: 192.168.1.x  │◄──────────────────►│ IP: 192.168.1.y  │
│ (DHCP)           │    Same Subnet     │ (DHCP)           │
├──────────────────┤    (192.168.1.0/24)├──────────────────┤
│ mDNS: _tcp.local │                    │ mDNS: _tcp.local │
│                  │                    │                  │
│ UDP: 51201-51204 │                    │ UDP: 51201-51204 │
│ TCP: 51200       │                    │ TCP: 51200       │
└──────────────────┘                    └──────────────────┘
```

### Advanced Network Setup

#### Static IP Configuration (Optional)
If you prefer static IPs for reliability:
1. Set static IPs on both machines (e.g., 192.168.1.100 and 192.168.1.101)
2. Ensure they're in the same subnet
3. mDNS will still work, but you can also connect directly by IP

#### VPN/Remote Access
For use over VPN:
1. Ensure the VPN allows multicast traffic (mDNS requires UDP multicast)
2. Some VPNs block multicast - you may need to use direct IP connection
3. Configure firewall rules to allow the app's ports

#### VLAN Segmentation
If machines are on different VLANs:
1. Configure inter-VLAN routing to allow traffic on ports 51200-51204
2. mDNS won't work across VLANs without mDNS reflector
3. Use direct IP connection instead

## Development

### Prerequisites
- **Windows**: Visual Studio Build Tools, Rust toolchain
- **macOS**: Xcode Command Line Tools, Rust toolchain
- **Both**: Node.js 18+, npm

### Building

#### Windows
```bash
# Install dependencies
npm install

# Development mode
cargo tauri dev

# Production build
cargo tauri build
```

#### macOS
```bash
# Install dependencies
npm install

# Development mode
cargo tauri dev

# Production build (creates .app bundle)
cargo tauri build
```

### Project Structure
```
Mac2Win/
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── main.rs         # Entry point
│   │   ├── protocol.rs     # Network protocol definitions
│   │   ├── discovery.rs    # mDNS peer discovery
│   │   ├── connection.rs   # TCP/UDP connection manager
│   │   ├── clipboard.rs    # Clipboard sync module
│   │   ├── audio.rs        # Audio capture/streaming
│   │   ├── capture/        # Screen capture (platform-specific)
│   │   ├── display/        # Screen rendering (platform-specific)
│   │   └── input/          # Input control (platform-specific)
│   └── Cargo.toml
├── src/                    # React frontend
│   ├── pages/
│   │   ├── Dashboard.tsx   # Main connection UI
│   │   └── Settings.tsx    # Settings with clipboard/audio controls
│   └── components/
└── README.md
```

## Platform-Specific Notes

### Windows
- **Screen Capture**: Uses DXGI Desktop Duplication API (requires GPU with driver support)
- **Clipboard**: Win32 API (OpenClipboard, GetClipboardData, SetClipboardData)
- **Audio**: WASAPI for system audio capture (loopback) and playback
- **Input**: SendInput API for keyboard/mouse control

### macOS
- **Screen Capture**: ScreenCaptureKit (macOS 12.3+) or CGDisplayStream
- **Clipboard**: NSPasteboard via Cocoa/AppKit
- **Audio**: Core Audio + ScreenCaptureKit for system audio
- **Input**: CGEvent API for keyboard/mouse control

## Security

- **Authentication**: HMAC-SHA256 token exchange during handshake
- **Optional encryption**: Can be added via TLS for control channel
- **Clipboard content**: Transmitted as-is (consider encryption for sensitive data)
- **Audio**: Unencrypted PCM/Opus (add encryption for sensitive audio)

## License

MIT License - See LICENSE file for details.
