<script lang="ts">
  import { t, i18n } from "../i18n";

  let {
    onNavigateTab,
  }: {
    onNavigateTab?: (tab: "account" | "audio" | "network" | "theme" | "sync" | "language") => void;
  } = $props();

  let activeSectionIndex = $state<number>(0);

  const sectionsMeta = [
    { id: "concept-security", icon: "🛡️", key: "section1" as const },
    { id: "voice-audio", icon: "🎙️", key: "section2" as const },
    { id: "messaging-offline", icon: "💬", key: "section3" as const },
    { id: "file-blobs", icon: "📦", key: "section4" as const },
    { id: "device-pairing", icon: "📱", key: "section5" as const },
    { id: "android-background", icon: "⚡", key: "section6" as const },
  ];

  let currentSection = $derived(
    i18n.dictionary.manual.sections[sectionsMeta[activeSectionIndex].key]
  );
</script>

<div class="manual-container">
  <!-- Header Banner -->
  <div class="manual-header-banner">
    <div class="header-icon-badge">📖</div>
    <div class="header-text-col">
      <h3>{t("manual.title")}</h3>
      <p class="header-subtitle">{t("manual.subtitle")}</p>
    </div>
  </div>

  <!-- Interactive Section Navigation Tabs -->
  <div class="manual-nav-tabs" role="tablist">
    {#each sectionsMeta as sec, idx}
      {@const sectionData = i18n.dictionary.manual.sections[sec.key]}
      <button
        class="nav-tab-pill"
        class:active={activeSectionIndex === idx}
        onclick={() => (activeSectionIndex = idx)}
        role="tab"
        aria-selected={activeSectionIndex === idx}
      >
        <span class="tab-icon">{sec.icon}</span>
        <span class="tab-label">{sectionData.badge}</span>
      </button>
    {/each}
  </div>

  <!-- Active Section Display Area -->
  <div class="manual-section-card" role="tabpanel">
    <div class="section-card-header">
      <div class="title-with-badge">
        <span class="section-badge">{currentSection.badge}</span>
        <h4>{currentSection.title}</h4>
      </div>
      <p class="section-summary">{currentSection.summary}</p>
    </div>

    <!-- Illustrated Section Visual Diagram -->
    <div class="diagram-container">
      {#if activeSectionIndex === 0}
        <!-- Section 1 Diagram: Serverless Topology vs Centralized -->
        <div class="diagram-box">
          <div class="topology-grid">
            <div class="topo-card peer-node left">
              <span class="topo-icon">💻</span>
              <span class="topo-name">Peer A (You)</span>
              <span class="topo-detail">Ed25519 Node ID</span>
              <span class="topo-detail">Local SQLite DB</span>
            </div>

            <div class="topo-middle">
              <div class="connection-line">
                <span class="mesh-badge">QUIC Encrypted Streams</span>
                <div class="double-arrow">◀─────────▶</div>
                <span class="mesh-sub">100% Serverless Mesh</span>
              </div>
              <div class="serverless-callout">
                <span class="cross-icon">🚫</span>
                <span class="cross-text">No Central Server / Cloud</span>
              </div>
            </div>

            <div class="topo-card peer-node right">
              <span class="topo-icon">📱</span>
              <span class="topo-name">Peer B (Contact)</span>
              <span class="topo-detail">Ed25519 Node ID</span>
              <span class="topo-detail">Local SQLite DB</span>
            </div>
          </div>
        </div>

      {:else if activeSectionIndex === 1}
        <!-- Section 2 Diagram: Voice Audio Pipeline -->
        <div class="diagram-box">
          <div class="pipeline-flow">
            <div class="pipeline-step">
              <span class="step-icon">🎤</span>
              <span class="step-title">Microphone</span>
              <span class="step-desc">CPAL / AAudio Hardware</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="pipeline-step highlight">
              <span class="step-icon">🧠</span>
              <span class="step-title">RNNoise AI</span>
              <span class="step-desc">48kHz Deep Neural Filter</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="pipeline-step">
              <span class="step-icon">📊</span>
              <span class="step-title">VAD Gate</span>
              <span class="step-desc">Voice Activity Threshold</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="pipeline-step">
              <span class="step-icon">🎵</span>
              <span class="step-title">Opus Codec</span>
              <span class="step-desc">20ms Frame Ultra-low Latency</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="pipeline-step success">
              <span class="step-icon">🔊</span>
              <span class="step-title">QUIC Mesh</span>
              <span class="step-desc">UDP Direct & DERP Relay</span>
            </div>
          </div>
        </div>

      {:else if activeSectionIndex === 2}
        <!-- Section 3 Diagram: Store-and-Forward Offline Queue -->
        <div class="diagram-box">
          <div class="store-forward-flow">
            <div class="sf-box sender">
              <span class="sf-icon">👤</span>
              <span class="sf-title">Sender</span>
              <span class="sf-sub">Sends message</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="sf-box queue">
              <span class="sf-icon">🕒</span>
              <span class="sf-title">Store-and-Forward</span>
              <span class="sf-sub">Stored in Local Encrypted Outbox</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="sf-box sync">
              <span class="sf-icon">⚡</span>
              <span class="sf-title">Peer Online Event</span>
              <span class="sf-sub">NAT Hole Punching Established</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="sf-box delivered">
              <span class="sf-icon">✓✓</span>
              <span class="sf-title">Delivered</span>
              <span class="sf-sub">Instant Receipt Confirmed</span>
            </div>
          </div>
        </div>

      {:else if activeSectionIndex === 3}
        <!-- Section 4 Diagram: iroh-blobs & BLAKE3 -->
        <div class="diagram-box">
          <div class="blobs-flow">
            <div class="blob-card">
              <span class="blob-icon">📁</span>
              <span class="blob-label">Source File</span>
              <span class="blob-note">Any size • No cloud limits</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="blob-card highlight">
              <span class="blob-icon">🔒</span>
              <span class="blob-label">BLAKE3 Hash Tree</span>
              <span class="blob-note">Verified Chunk Integrity</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="blob-card success">
              <span class="blob-icon">📦</span>
              <span class="blob-label">iroh-blobs Stream</span>
              <span class="blob-note">Direct peer-to-peer download</span>
            </div>
          </div>
        </div>

      {:else if activeSectionIndex === 4}
        <!-- Section 5 Diagram: Device Pairing Flow -->
        <div class="diagram-box">
          <div class="pairing-steps-row">
            <div class="pair-step-box">
              <span class="step-num">1</span>
              <span class="step-pic">💻</span>
              <span class="step-txt">PC: Generate Ticket</span>
              <span class="step-sub">Temporary QR Code</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="pair-step-box highlight">
              <span class="step-num">2</span>
              <span class="step-pic">📱📷</span>
              <span class="step-txt">Phone: Scan with Camera</span>
              <span class="step-sub">p2psync:// Handshake</span>
            </div>
            <span class="pipe-arrow">➜</span>
            <div class="pair-step-box success">
              <span class="step-num">3</span>
              <span class="step-pic">✨</span>
              <span class="step-txt">Instant Sync</span>
              <span class="step-sub">Keys, Contacts, Chats Cloned</span>
            </div>
          </div>
        </div>

      {:else if activeSectionIndex === 5}
        <!-- Section 6 Diagram: Android Foreground Service -->
        <div class="diagram-box">
          <div class="android-box-row">
            <div class="android-card">
              <span class="and-icon">🔔</span>
              <span class="and-title">Foreground Service</span>
              <span class="and-desc">Sticky notification keeps process priority high</span>
            </div>
            <div class="android-card highlight">
              <span class="and-icon">🔋</span>
              <span class="and-title">WakeLock Active</span>
              <span class="and-desc">Prevents deep CPU sleep during voice conversations</span>
            </div>
            <div class="android-card success">
              <span class="and-icon">🎙️</span>
              <span class="and-title">AAudio Audio Focus</span>
              <span class="and-desc">Low-latency hardware stream retained in background</span>
            </div>
          </div>
        </div>
      {/if}
    </div>

    <!-- Explanatory Points List -->
    <div class="points-list">
      {#each currentSection.points as point}
        <div class="point-item">
          <div class="point-bullet">🔹</div>
          <div class="point-content">
            <span class="point-title">{point.title}</span>
            <p class="point-desc">{point.desc}</p>
          </div>
        </div>
      {/each}
    </div>

    <!-- Technical Note Callout Box -->
    {#if currentSection.technicalNote}
      <div class="technical-callout-box">
        <span class="callout-icon">⚙️</span>
        <div class="callout-body">
          <span class="callout-title">Technical Specification:</span>
          <span class="callout-text">{currentSection.technicalNote}</span>
        </div>
      </div>
    {/if}

    <!-- Contextual Quick Action Footer -->
    <div class="section-footer-actions">
      {#if activeSectionIndex === 0}
        <button
          class="manual-action-btn"
          onclick={() => onNavigateTab?.("account")}
        >
          👤 View My Node ID & Profile
        </button>
      {:else if activeSectionIndex === 1}
        <button
          class="manual-action-btn"
          onclick={() => onNavigateTab?.("audio")}
        >
          🎙️ Configure Audio & RNNoise
        </button>
      {:else if activeSectionIndex === 4}
        <button
          class="manual-action-btn"
          onclick={() => onNavigateTab?.("sync")}
        >
          📱 Open Device Pairing & QR Sync
        </button>
      {:else if activeSectionIndex === 5}
        <button
          class="manual-action-btn"
          onclick={() => onNavigateTab?.("network")}
        >
          🌐 View Network & VPN Bypass
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .manual-container {
    display: flex;
    flex-direction: column;
    gap: 16px;
    color: var(--text-primary);
  }

  .manual-header-banner {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 16px 20px;
    background: linear-gradient(135deg, rgba(79, 70, 229, 0.16), rgba(99, 102, 241, 0.05));
    border: var(--border-width) solid var(--accent-primary);
    border-radius: var(--border-radius);
  }

  .header-icon-badge {
    font-size: 2.2rem;
    line-height: 1;
    flex-shrink: 0;
  }

  .header-text-col h3 {
    margin: 0 0 4px 0;
    font-size: 1.18rem;
    font-weight: 700;
    color: var(--text-primary);
    word-break: break-word;
  }

  .header-subtitle {
    margin: 0;
    font-size: 0.86rem;
    color: var(--text-muted);
    word-break: break-word;
  }

  .manual-nav-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-bottom: 4px;
  }

  .nav-tab-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 44px;
    padding: 8px 14px;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    color: var(--text-muted);
    font-size: 0.86rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s ease;
    word-break: break-word;
    text-align: left;
  }

  .nav-tab-pill:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  .nav-tab-pill.active {
    background: var(--accent-primary);
    color: #ffffff;
    border-color: var(--accent-primary);
    box-shadow: 0 4px 12px rgba(79, 70, 229, 0.35);
  }

  .tab-icon {
    font-size: 1.1rem;
    flex-shrink: 0;
  }

  .manual-section-card {
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .section-card-header {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .title-with-badge {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .section-badge {
    display: inline-block;
    padding: 3px 8px;
    font-size: 0.72rem;
    font-weight: 700;
    text-transform: uppercase;
    background: rgba(79, 70, 229, 0.25);
    color: var(--accent-hover);
    border-radius: 4px;
    border: 1px solid rgba(99, 102, 241, 0.3);
  }

  .title-with-badge h4 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 700;
    color: var(--text-primary);
    word-break: break-word;
  }

  .section-summary {
    margin: 0;
    font-size: 0.9rem;
    color: var(--text-muted);
    line-height: 1.45;
    word-break: break-word;
  }

  /* Illustrated Diagram Container */
  .diagram-container {
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 16px;
    overflow-x: auto;
  }

  .diagram-box {
    min-width: 280px;
  }

  /* Topology Grid (Section 1) */
  .topology-grid {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  @media (max-width: 600px) {
    .topology-grid {
      flex-direction: column;
    }
  }

  .topo-card {
    flex: 1;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 4px;
  }

  .topo-icon {
    font-size: 2rem;
  }

  .topo-name {
    font-weight: 700;
    font-size: 0.88rem;
    color: var(--text-primary);
  }

  .topo-detail {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .topo-middle {
    flex: 1.2;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
  }

  .connection-line {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }

  .mesh-badge {
    font-size: 0.75rem;
    font-weight: 700;
    color: var(--accent-hover);
    background: rgba(99, 102, 241, 0.15);
    padding: 2px 8px;
    border-radius: 4px;
  }

  .double-arrow {
    font-size: 1rem;
    color: var(--accent-hover);
    letter-spacing: -2px;
  }

  .mesh-sub {
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  .serverless-callout {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.35);
    border-radius: 4px;
    font-size: 0.75rem;
    color: #fca5a5;
  }

  /* Audio Pipeline (Section 2) */
  .pipeline-flow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    overflow-x: auto;
    padding: 4px 0;
  }

  .pipeline-step {
    flex: 1;
    min-width: 90px;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 10px 8px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 3px;
  }

  .pipeline-step.highlight {
    border-color: var(--accent-primary);
    background: rgba(79, 70, 229, 0.1);
  }

  .pipeline-step.success {
    border-color: #10b981;
    background: rgba(16, 185, 129, 0.1);
  }

  .step-icon {
    font-size: 1.5rem;
  }

  .step-title {
    font-size: 0.78rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .step-desc {
    font-size: 0.68rem;
    color: var(--text-muted);
    line-height: 1.2;
  }

  .pipe-arrow {
    font-size: 1rem;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  /* Store & Forward (Section 3) */
  .store-forward-flow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    overflow-x: auto;
  }

  .sf-box {
    flex: 1;
    min-width: 100px;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 12px 8px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 4px;
  }

  .sf-box.queue {
    border-color: #f59e0b;
    background: rgba(245, 158, 11, 0.1);
  }

  .sf-box.delivered {
    border-color: #10b981;
    background: rgba(16, 185, 129, 0.1);
  }

  .sf-icon {
    font-size: 1.4rem;
  }

  .sf-title {
    font-size: 0.8rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .sf-sub {
    font-size: 0.68rem;
    color: var(--text-muted);
    line-height: 1.2;
  }

  /* Blobs (Section 4) */
  .blobs-flow {
    display: flex;
    align-items: center;
    justify-content: space-around;
    gap: 12px;
  }

  .blob-card {
    flex: 1;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 4px;
  }

  .blob-card.highlight {
    border-color: var(--accent-primary);
  }

  .blob-card.success {
    border-color: #10b981;
  }

  .blob-icon {
    font-size: 1.8rem;
  }

  .blob-label {
    font-size: 0.85rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .blob-note {
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  /* Pairing (Section 5) */
  .pairing-steps-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .pair-step-box {
    flex: 1;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 12px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 4px;
    position: relative;
  }

  .step-num {
    position: absolute;
    top: 6px;
    left: 8px;
    font-size: 0.7rem;
    font-weight: 800;
    color: var(--accent-hover);
  }

  .step-pic {
    font-size: 1.6rem;
  }

  .step-txt {
    font-size: 0.8rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .step-sub {
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  /* Android (Section 6) */
  .android-box-row {
    display: flex;
    gap: 12px;
    justify-content: space-between;
  }

  @media (max-width: 600px) {
    .android-box-row {
      flex-direction: column;
    }
  }

  .android-card {
    flex: 1;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .android-card.highlight {
    border-color: #f59e0b;
  }

  .android-card.success {
    border-color: #10b981;
  }

  .and-icon {
    font-size: 1.4rem;
  }

  .and-title {
    font-size: 0.85rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .and-desc {
    font-size: 0.74rem;
    color: var(--text-muted);
    line-height: 1.3;
  }

  /* Points List */
  .points-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .point-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  .point-bullet {
    font-size: 0.9rem;
    line-height: 1.4;
    flex-shrink: 0;
  }

  .point-content {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .point-title {
    font-size: 0.92rem;
    font-weight: 700;
    color: var(--text-primary);
    word-break: break-word;
  }

  .point-desc {
    margin: 0;
    font-size: 0.84rem;
    color: var(--text-muted);
    line-height: 1.45;
    word-break: break-word;
  }

  /* Technical Callout */
  .technical-callout-box {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    background: rgba(79, 70, 229, 0.08);
    border: var(--border-width) solid rgba(99, 102, 241, 0.25);
    border-radius: var(--border-radius);
  }

  .callout-icon {
    font-size: 1.2rem;
    flex-shrink: 0;
  }

  .callout-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .callout-title {
    font-size: 0.75rem;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--accent-hover);
    letter-spacing: 0.5px;
  }

  .callout-text {
    font-size: 0.82rem;
    color: var(--text-primary);
    line-height: 1.4;
    word-break: break-word;
  }

  /* Action Buttons */
  .section-footer-actions {
    display: flex;
    justify-content: flex-end;
    padding-top: 8px;
    border-top: var(--border-width) solid var(--border-color);
  }

  .manual-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 44px;
    padding: 8px 18px;
    background: var(--bg-tertiary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    color: var(--text-primary);
    font-size: 0.86rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .manual-action-btn:hover {
    background: var(--accent-primary);
    color: #ffffff;
    border-color: var(--accent-primary);
  }
</style>
