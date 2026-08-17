import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { Settings } from "./pages/Settings";

type Tab = "dashboard" | "settings";

interface Peer {
  id: string;
  hostname: string;
  os: string;
  addr: string;
}

function App() {
  const [tab, setTab] = useState<Tab>("dashboard");

  return (
    <div className="app">
      <nav className="sidebar">
        <div className="sidebar-header">
          <h1 className="app-title">🖥️ Mac2Win</h1>
        </div>
        <ul className="nav-links">
          <li>
            <button
              className={tab === "dashboard" ? "active" : ""}
              onClick={() => setTab("dashboard")}
            >
              📊 Dashboard
            </button>
          </li>
          <li>
            <button
              className={tab === "settings" ? "active" : ""}
              onClick={() => setTab("settings")}
            >
              ⚙️ Settings
            </button>
          </li>
        </ul>
      </nav>
      <main className="content">
        {tab === "dashboard" && <Dashboard />}
        {tab === "settings" && <Settings />}
      </main>
    </div>
  );
}

function Dashboard() {
  const [peers, setPeers] = useState<Peer[]>([]);
  const [connected, setConnected] = useState(false);
  const [connectedPeer, setConnectedPeer] = useState<string | null>(null);

  useEffect(() => {
    const unlistenFound = listen<Peer>("peer-found", (event) => {
      console.log("Peer found:", event.payload);
      setPeers((prev) => {
        const exists = prev.some((p) => p.id === event.payload.id);
        if (exists) return prev;
        return [...prev, event.payload];
      });
    });

    const unlistenLost = listen<{ id: string }>("peer-lost", (event) => {
      console.log("Peer lost:", event.payload);
      setPeers((prev) => prev.filter((p) => p.id !== event.payload.id));
    });

    return () => {
      unlistenFound.then((fn) => fn());
      unlistenLost.then((fn) => fn());
    };
  }, []);

  const handleConnect = async (peer: Peer) => {
    try {
      await invoke("connect_to_peer", { addr: peer.addr });
      setConnected(true);
      setConnectedPeer(peer.hostname);
    } catch (e) {
      console.error("Connect failed:", e);
    }
  };

  const handleDisconnect = async () => {
    try {
      await invoke("disconnect");
      setConnected(false);
      setConnectedPeer(null);
    } catch (e) {
      console.error("Disconnect failed:", e);
    }
  };

  return (
    <div className="dashboard">
      <h1>Mac2Win — Extended Display</h1>
      <p className="subtitle">Connect your Windows and macOS machines over the local network.</p>

      <div className="status-grid">
        <div className="status-card">
          <span className="status-icon">🔍</span>
          <h3>Network Discovery</h3>
          <p className="status-label status-ok">
            {peers.length > 0 ? `${peers.length} peer(s) found` : "Searching…"}
          </p>
          <p className="status-detail">mDNS broadcasting on _mac2win._tcp.local.</p>
        </div>
        <div className="status-card">
          <span className="status-icon">🖥️</span>
          <h3>Display</h3>
          <p className={`status-label ${connected ? "status-ok" : ""}`}>
            {connected ? `Connected to ${connectedPeer}` : "Not connected"}
          </p>
          <p className="status-detail">
            {connected ? "Remote display active" : "No remote peer connected"}
          </p>
        </div>
        <div className="status-card">
          <span className="status-icon">📋</span>
          <h3>Clipboard</h3>
          <p className="status-label">{connected ? "Syncing" : "Ready"}</p>
          <p className="status-detail">
            {connected ? "Clipboard shared with peer" : "Will sync when connected"}
          </p>
        </div>
        <div className="status-card">
          <span className="status-icon">🔊</span>
          <h3>Audio</h3>
          <p className="status-label">Disabled</p>
          <p className="status-detail">Enable in Settings</p>
        </div>
      </div>

      {/* Discovered Peers */}
      {peers.length > 0 && (
        <div className="peers-section">
          <h2>Discovered Peers</h2>
          {peers.map((peer) => (
            <div key={peer.id} className="peer-card">
              <div className="peer-info">
                <span className="peer-icon">{peer.os === "MacOS" ? "🍎" : "🪟"}</span>
                <div>
                  <h3>{peer.hostname}</h3>
                  <p className="peer-detail">{peer.os} — {peer.addr}</p>
                </div>
              </div>
              {connected && connectedPeer === peer.hostname ? (
                <button className="btn-disconnect" onClick={handleDisconnect}>
                  Disconnect
                </button>
              ) : (
                <button className="btn-connect" onClick={() => handleConnect(peer)}>
                  Connect
                </button>
              )}
            </div>
          ))}
        </div>
      )}

      <div className="help-section">
        <h2>Getting Started</h2>
        <ol>
          <li>Install and run Mac2Win on both machines</li>
          <li>Both machines must be on the same local network</li>
          <li>The app will automatically discover the other machine via mDNS</li>
          <li>Click <strong>Connect</strong> when a peer appears above</li>
          <li>Configure clipboard sharing and audio in <strong>Settings</strong></li>
        </ol>
      </div>
    </div>
  );
}

export default App;
