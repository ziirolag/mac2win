use crate::protocol::*;
use log::{debug, error, info, warn};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{broadcast, mpsc, RwLock};

/// Manages TCP control connections and UDP video/input/clipboard/audio streams.
pub struct ConnectionManager {
    control_listener: Option<TcpListener>,
    video_socket: Option<Arc<UdpSocket>>,
    input_socket: Option<Arc<UdpSocket>>,
    clipboard_socket: Option<Arc<UdpSocket>>,
    audio_socket: Option<Arc<UdpSocket>>,
    control_tx: mpsc::Sender<ControlMessage>,
    control_rx: Arc<RwLock<mpsc::Receiver<ControlMessage>>>,
    event_tx: broadcast::Sender<ConnectionEvent>,
}

#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    Connected { peer: PeerInfo },
    Disconnected { reason: String },
    ControlMessage(ControlMessage),
    VideoFrame { header: VideoFrameHeader, data: Vec<u8> },
    InputEvent(InputEvent),
    ClipboardFrame { data: Vec<u8> },
    AudioFrame { header: AudioFrameHeader, data: Vec<u8> },
    Error(String),
}

impl ConnectionManager {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let video_socket = Arc::new(UdpSocket::bind(format!("0.0.0.0:{}", VIDEO_PORT)).await?);
        let input_socket = Arc::new(UdpSocket::bind(format!("0.0.0.0:{}", INPUT_PORT)).await?);
        let clipboard_socket = Arc::new(UdpSocket::bind(format!("0.0.0.0:{}", CLIPBOARD_PORT)).await?);
        let audio_socket = Arc::new(UdpSocket::bind(format!("0.0.0.0:{}", AUDIO_PORT)).await?);
        let (event_tx, _) = broadcast::channel(256);
        let (control_tx, control_rx) = mpsc::channel(64);

        info!(
            "ConnectionManager listening on UDP {} (video), {} (input), {} (clipboard), {} (audio)",
            VIDEO_PORT, INPUT_PORT, CLIPBOARD_PORT, AUDIO_PORT
        );

        Ok(Self {
            control_listener: None,
            video_socket: Some(video_socket),
            input_socket: Some(input_socket),
            clipboard_socket: Some(clipboard_socket),
            audio_socket: Some(audio_socket),
            control_tx,
            control_rx: Arc::new(RwLock::new(control_rx)),
            event_tx,
        })
    }

    /// Start listening for incoming TCP control connections.
    pub async fn start_listening(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", CONTROL_PORT)).await?;
        info!("Listening for control connections on TCP {}", CONTROL_PORT);

        let event_tx = self.event_tx.clone();
        let video_socket = self.video_socket.clone().unwrap();
        let input_socket = self.input_socket.clone().unwrap();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        info!("Incoming connection from {}", addr);
                        let tx = event_tx.clone();
                        let video_sock = video_socket.clone();
                        let input_sock = input_socket.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_connection(stream, addr, tx, video_sock, input_sock).await
                            {
                                error!("Connection handler error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                    }
                }
            }
        });

        self.control_listener = Some(listener);
        Ok(())
    }

    /// Connect to a remote peer.
    pub async fn connect_to_peer(
        &self,
        addr: SocketAddr,
        local_peer: PeerInfo,
        auth_token: [u8; 32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let stream = TcpStream::connect(addr).await?;
        let peer_addr = addr;

        info!("Connected to peer at {}", peer_addr);

        let event_tx = self.event_tx.clone();

        // Send handshake
        let handshake = ControlMessage::Handshake {
            version: PROTOCOL_VERSION,
            peer: local_peer,
            auth_token,
        };
        let handshake_bytes = bincode::serialize(&handshake)?;
        let len = (handshake_bytes.len() as u32).to_le_bytes();

        let mut stream_clone = stream.clone();
        stream_clone.write_all(&len).await?;
        stream_clone.write_all(&handshake_bytes).await?;

        // Handle incoming messages
        let tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer_addr, tx, 
                Arc::new(UdpSocket::bind("0.0.0.0:0").await.unwrap()),
                Arc::new(UdpSocket::bind("0.0.0.0:0").await.unwrap())
            ).await {
                error!("Connection handler error: {}", e);
            }
        });

        Ok(())
    }

    /// Send a control message to the connected peer.
    pub async fn send_control(&self, msg: ControlMessage) -> Result<(), Box<dyn std::error::Error>> {
        self.control_tx.send(msg).await?;
        Ok(())
    }

    /// Subscribe to connection events.
    pub fn subscribe(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.event_tx.subscribe()
    }

    /// Send a video frame over UDP to the peer.
    pub async fn send_video_frame(
        &self,
        header: &VideoFrameHeader,
        data: &[u8],
        target: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sock = self.video_socket.as_ref().ok_or("Video socket not initialized")?;
        let header_bytes = bincode::serialize(header)?;

        // Send header first
        let mut packet = Vec::with_capacity(4 + header_bytes.len() + data.len());
        packet.extend_from_slice(&(header_bytes.len() as u32).to_le_bytes());
        packet.extend_from_slice(&header_bytes);
        packet.extend_from_slice(data);

        // Split into UDP-safe chunks (max ~64KB)
        const CHUNK_SIZE: usize = 60000;
        for (i, chunk) in packet.chunks(CHUNK_SIZE).enumerate() {
            let mut udp_packet = Vec::with_capacity(8 + chunk.len());
            udp_packet.extend_from_slice(&(i as u16).to_le_bytes());
            udp_packet.extend_from_slice(&((packet.len() / CHUNK_SIZE + 1) as u16).to_le_bytes());
            udp_packet.extend_from_slice(chunk);
            sock.send_to(&udp_packet, target).await?;
        }

        Ok(())
    }

    /// Send an input event over UDP to the peer.
    pub async fn send_input_event(
        &self,
        event: &InputEvent,
        target: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sock = self.input_socket.as_ref().ok_or("Input socket not initialized")?;
        let data = bincode::serialize(event)?;
        sock.send_to(&data, target).await?;
        Ok(())
    }

    /// Send clipboard content over UDP to the peer.
    pub async fn send_clipboard(
        &self,
        content: &ClipboardContent,
        sequence: u64,
        target: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sock = self.clipboard_socket.as_ref().ok_or("Clipboard socket not initialized")?;
        let msg = ControlMessage::ClipboardUpdate {
            content: content.clone(),
            sequence,
        };
        let data = bincode::serialize(&msg)?;
        sock.send_to(&data, target).await?;
        Ok(())
    }

    /// Send an audio frame over UDP to the peer.
    pub async fn send_audio_frame(
        &self,
        header: &AudioFrameHeader,
        data: &[u8],
        target: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sock = self.audio_socket.as_ref().ok_or("Audio socket not initialized")?;
        let header_bytes = bincode::serialize(header)?;

        let mut packet = Vec::with_capacity(4 + header_bytes.len() + data.len());
        packet.extend_from_slice(&(header_bytes.len() as u32).to_le_bytes());
        packet.extend_from_slice(&header_bytes);
        packet.extend_from_slice(data);

        // Split into UDP-safe chunks
        const CHUNK_SIZE: usize = 60000;
        for (i, chunk) in packet.chunks(CHUNK_SIZE).enumerate() {
            let mut udp_packet = Vec::with_capacity(8 + chunk.len());
            udp_packet.extend_from_slice(&(i as u16).to_le_bytes());
            udp_packet.extend_from_slice(&((packet.len() / CHUNK_SIZE + 1) as u16).to_le_bytes());
            udp_packet.extend_from_slice(chunk);
            sock.send_to(&udp_packet, target).await?;
        }

        Ok(())
    }
}

/// Handle a single TCP control connection.
async fn handle_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    event_tx: broadcast::Sender<ConnectionEvent>,
    _video_socket: Arc<UdpSocket>,
    _input_socket: Arc<UdpSocket>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Read message length
        let mut len_buf = [0u8; 4];
        match stream.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    info!("Peer {} disconnected", addr);
                    let _ = event_tx.send(ConnectionEvent::Disconnected {
                        reason: "Connection closed".to_string(),
                    });
                } else {
                    error!("Read error from {}: {}", addr, e);
                    let _ = event_tx.send(ConnectionEvent::Error(e.to_string()));
                }
                return Ok(());
            }
        }

        let len = u32::from_le_bytes(len_buf) as usize;
        if len > 10 * 1024 * 1024 {
            error!("Message too large from {}: {} bytes", addr, len);
            break;
        }

        let mut msg_buf = vec![0u8; len];
        stream.read_exact(&mut msg_buf).await?;

        let msg: ControlMessage = bincode::deserialize(&msg_buf)?;
        debug!("Received from {}: {:?}", addr, msg);

        let _ = event_tx.send(ConnectionEvent::ControlMessage(msg));
    }

    Ok(())
}
