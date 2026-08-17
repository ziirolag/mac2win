import { useState } from "react";
import { Settings } from "./pages/Settings";

type Tab = "dashboard" | "settings";

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
  return (
    <div className="dashboard">
      <h1>Mac2Win — Extended Display</h1>
      <p className="subtitle">Connect your Windows and macOS machines over the local network.</p>

      <div className="status-grid">
        <div className="status-card">
          <span className="status-icon">🔍</span>
          <h3>Network Discovery</h3>
          <p className="status-label status-ok">Searching…</p>
          <p className="status-detail">mDNS broadcasting on _mac2win._tcp.local.</p>
        </div>
        <div className="status-card">
          <span className="status-icon">🖥️</span>
          <h3>Display</h3>
          <p className="status-label">Not connected</p>
          <p className="status-detail">No remote peer connected</p>
        </div>
        <div className="status-card">
          <span className="status-icon">📋</span>
          <h3>Clipboard</h3>
          <p className="status-label">Ready</p>
          <p className="status-detail">Will sync when connected</p>
        </div>
        <div className="status-card">
          <span className="status-icon">🔊</span>
          <h3>Audio</h3>
          <p className="status-label">Disabled</p>
          <p className="status-detail">Enable in Settings</p>
        </div>
      </div>

      <div className="help-section">
        <h2>Getting Started</h2>
        <ol>
          <li>Install and run Mac2Win on both machines</li>
          <li>Both machines must be on the same local network</li>
          <li>The app will automatically discover the other machine via mDNS</li>
          <li>Click <strong>Connect</strong> when a peer appears</li>
          <li>Configure clipboard sharing and audio in <strong>Settings</strong></li>
        </ol>
      </div>
    </div>
  );
}

export default App;
