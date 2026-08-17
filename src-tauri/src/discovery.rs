use crate::protocol::*;
use log::{debug, error, info, warn};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Handles mDNS service advertisement and peer discovery on the local network.
/// Both peers advertise themselves; when one is found, it's reported to the UI.
pub struct NetworkDiscovery {
    mdns: ServiceDaemon,
    service_info: ServiceInfo,
    discovered_peers: Arc<RwLock<HashMap<String, PeerInfo>>>,
    peer_event_tx: broadcast::Sender<DiscoveryEvent>,
}

#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    PeerFound {
        id: String,
        info: PeerInfo,
        addr: IpAddr,
    },
    PeerLost {
        id: String,
    },
}

impl NetworkDiscovery {
    /// Create a new discovery instance and begin advertising on the local network.
    pub fn new(peer: PeerInfo, local_ip: IpAddr) -> Result<Self, Box<dyn std::error::Error>> {
        let mdns = ServiceDaemon::new()?;

        let hostname = peer.hostname.clone();
        let service_name = format!("mac2win-{}", hostname);

        let mut properties = HashMap::new();
        properties.insert("os".to_string(), format!("{:?}", peer.os));
        properties.insert("version".to_string(), peer.app_version.clone());
        properties.insert(
            "resolution".to_string(),
            format!("{}x{}", peer.screen_width, peer.screen_height),
        );

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &service_name,
            &format!("{}.local.", hostname),
            local_ip,
            peer.control_port,
            properties,
        )?;

        let (peer_event_tx, _) = broadcast::channel(64);

        Ok(Self {
            mdns,
            service_info,
            discovered_peers: Arc::new(RwLock::new(HashMap::new())),
            peer_event_tx,
        })
    }

    /// Start advertising and browsing for peers on the network.
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Register our service
        self.mdns.register(self.service_info.clone())?;
        info!("Advertising service on mDNS: {}", self.service_info.get_fullname());

        // Browse for other peers
        let browser = self.mdns.browse(SERVICE_TYPE)?;
        let peers = self.discovered_peers.clone();
        let tx = self.peer_event_tx.clone();

        tokio::spawn(async move {
            while let Ok(event) = browser.recv_async().await {
                match event {
                    mdns_sd::ServiceEvent::ServiceResolved(info) => {
                        let id = info.get_fullname().to_string();
                        if let Some(addr) = info.get_addresses().iter().next().copied() {
                            // Parse peer info from properties
                            let props = info.get_properties();
                            let os_str = props.get("os").map(|s| s.to_string()).unwrap_or_default();
                            let os = match os_str.as_str() {
                                "Windows" => OperatingSystem::Windows,
                                "MacOS" => OperatingSystem::MacOS,
                                _ => OperatingSystem::Windows,
                            };
                            let version = props.get("version").map(|s| s.to_string()).unwrap_or_default();
                            let res = props.get("resolution").map(|s| s.to_string()).unwrap_or_default();
                            let parts: Vec<&str> = res.split('x').collect();
                            let w = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1920);
                            let h = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1080);

                            let peer = PeerInfo {
                                hostname: info.get_hostname().to_string(),
                                os,
                                screen_width: w,
                                screen_height: h,
                                screen_scale: 1.0,
                                app_version: version,
                                control_port: info.get_port(),
                                video_port: crate::protocol::VIDEO_PORT,
                                input_port: crate::protocol::INPUT_PORT,
                            };

                            info!("Discovered peer: {} ({:?}) at {}", peer.hostname, peer.os, addr);
                            peers.write().await.insert(id.clone(), peer.clone());
                            let _ = tx.send(DiscoveryEvent::PeerFound {
                                id,
                                info: peer,
                                addr,
                            });
                        }
                    }
                    mdns_sd::ServiceEvent::ServiceRemoved(info) => {
                        let id = info.get_fullname().to_string();
                        peers.write().await.remove(&id);
                        warn!("Peer lost: {}", id);
                        let _ = tx.send(DiscoveryEvent::PeerLost { id });
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    /// Subscribe to discovery events.
    pub fn subscribe(&self) -> broadcast::Receiver<DiscoveryEvent> {
        self.peer_event_tx.subscribe()
    }

    /// Get currently discovered peers.
    pub async fn get_peers(&self) -> Vec<(String, PeerInfo)> {
        self.discovered_peers
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Stop advertising and browsing.
    pub fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.mdns.unregister(&self.service_info)?;
        self.mdns.shutdown()?;
        Ok(())
    }
}

/// Get the local IP address that can reach the LAN (not loopback).
pub fn get_local_ip() -> Option<IpAddr> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip())
}
