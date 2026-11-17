<script lang="ts">
  import type { AudioDeviceInfo, DeviceSyncTicket, ImportSyncResult } from "../types";
  import type { ThemeConfig } from "../theme";
  import { api } from "../api";
  import ThemeStudio from "./ThemeStudio.svelte";

  let {
    currentTheme = $bindable(),
    onClose,
    onSyncCompleted,
  }: {
    currentTheme: ThemeConfig;
    onClose: () => void;
    onSyncCompleted?: () => void;
  } = $props();

  let activeTab = $state<"audio" | "network" | "theme" | "sync">("audio");

  // Audio State
  let inputDevices = $state<AudioDeviceInfo[]>([]);
  let outputDevices = $state<AudioDeviceInfo[]>([]);
  let selectedInput = $state<string>("default");
  let selectedOutput = $state<string>("default");
  let micGain = $state<number>(1.0);
  let masterVolume = $state<number>(1.0);
  let rnnoiseEnabled = $state<boolean>(true);

  // Mic Test & VAD State
  let isTestingMic = $state<boolean>(false);
  let localInputLevel = $state<{ db: number; percent: number }>({ db: -60, percent: 0 });
  let vadThreshold = $state<number>(0.45);
  let smoothedPercent = $state<number>(0);

  // Network State
  let vpnBypassEnabled = $state<boolean>(true);

  // Sync State
  let syncTicket = $state<DeviceSyncTicket | null>(null);
  let importTicketInput = $state<string>("");
  let syncResult = $state<ImportSyncResult | null>(null);
  let syncLoading = $state<boolean>(false);
  let syncError = $state<string | null>(null);
  let copyFeedback = $state<boolean>(false);

  $effect(() => {
    // Fetch available audio devices
    api.listAudioDevices().then((res) => {
      inputDevices = res.input_devices || [];
      outputDevices = res.output_devices || [];

      const defIn = inputDevices.find((d) => d.is_default);
      if (defIn) selectedInput = defIn.name;

      const defOut = outputDevices.find((d) => d.is_default);
      if (defOut) selectedOutput = defOut.name;
    });

    let unlistenLocalLevel: (() => void) | undefined;

    // Activate local meter when in audio tab
    if (activeTab === "audio") {
      api.setMeterActive(true).catch(console.error);
      api.onLocalInputLevel((payload) => {
        localInputLevel = payload;
        smoothedPercent = payload.percent;
      }).then((u) => {
        unlistenLocalLevel = u;
      });
    } else {
      if (isTestingMic) {
        isTestingMic = false;
        api.setMicTest(false).catch(console.error);
      }
      api.setMeterActive(false).catch(console.error);
    }

    return () => {
      unlistenLocalLevel?.();
      if (isTestingMic) {
        api.setMicTest(false).catch(console.error);
      }
      api.setMeterActive(false).catch(console.error);
    };
  });

  function handleModalClose() {
    if (isTestingMic) {
      isTestingMic = false;
      api.setMicTest(false).catch(console.error);
    }
    api.setMeterActive(false).catch(console.error);
    onClose();
  }

  async function handleToggleMicTest() {
    isTestingMic = !isTestingMic;
    try {
      await api.setMicTest(isTestingMic);
    } catch (err) {
      console.error("Failed to toggle mic test:", err);
      isTestingMic = false;
    }
  }

  async function handleVadThresholdChange(threshold: number) {
    vadThreshold = threshold;
    try {
      await api.setVadThreshold(threshold);
    } catch (err) {
      console.error("Failed to set VAD threshold:", err);
    }
  }

  async function handleInputDeviceChange(name: string) {
    selectedInput = name;
    await api.setInputDevice(name);
  }

  async function handleOutputDeviceChange(name: string) {
    selectedOutput = name;
    await api.setOutputDevice(name);
  }

  async function handleMicGainChange(gain: number) {
    micGain = gain;
    await api.setMicrophoneGain(gain);
  }

  async function handleMasterVolumeChange(volume: number) {
    masterVolume = volume;
    await api.setMasterVolume(volume);
  }

  async function handleToggleRnnoise() {
    rnnoiseEnabled = !rnnoiseEnabled;
    await api.setRNNoise(rnnoiseEnabled);
  }

  async function handleToggleVpnBypass() {
    vpnBypassEnabled = !vpnBypassEnabled;
    await api.setBypassVpn(vpnBypassEnabled);
  }

  async function handleGenerateSyncTicket() {
    syncLoading = true;
    syncError = null;
    try {
      syncTicket = await api.syncDevices();
    } catch (err: any) {
      syncError = err?.toString() || "Failed to generate pairing ticket";
    } finally {
      syncLoading = false;
    }
  }

  function copySyncTicket() {
    if (!syncTicket?.ticket) return;
    navigator.clipboard.writeText(syncTicket.ticket);
    copyFeedback = true;
    setTimeout(() => {
      copyFeedback = false;
    }, 2000);
  }

  async function handleImportSyncTicket() {
    if (!importTicketInput.trim()) return;
    syncLoading = true;
    syncError = null;
    try {
      syncResult = await api.importSyncTicket(importTicketInput.trim());
      onSyncCompleted?.();
    } catch (err: any) {
      syncError = err?.toString() || "Failed to import sync ticket";
    } finally {
      syncLoading = false;
    }
  }
</script>

<div
  class="modal-backdrop"
  onclick={(e) => { if (e.target === e.currentTarget) handleModalClose(); }}
  onkeydown={(e) => e.key === "Escape" && handleModalClose()}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="settings-dialog" role="document">
    <!-- Dialog Header -->
    <div class="dialog-header">
      <div class="tabs-row">
        <button
          class="tab-btn"
          class:active={activeTab === "audio"}
          onclick={() => (activeTab = "audio")}
        >
          🎙️ Voice & Audio
        </button>
        <button
          class="tab-btn"
          class:active={activeTab === "network"}
          onclick={() => (activeTab = "network")}
        >
          🌐 Network & VPN
        </button>
        <button
          class="tab-btn"
          class:active={activeTab === "theme"}
          onclick={() => (activeTab = "theme")}
        >
          🎨 Theme Studio
        </button>
        <button
          class="tab-btn"
          class:active={activeTab === "sync"}
          onclick={() => (activeTab = "sync")}
        >
          📱 Device Linking
        </button>
      </div>
      <button class="dialog-close-btn" onclick={handleModalClose}>&times;</button>
    </div>

    <!-- Dialog Content Body -->
    <div class="dialog-body">
      {#if activeTab === "audio"}
        <div class="settings-group">
          <h4>Audio Devices</h4>

          <div class="setting-item">
            <label for="setting-input-dev">Input Device (Microphone)</label>
            <select
              id="setting-input-dev"
              class="settings-select"
              value={selectedInput}
              onchange={(e) => handleInputDeviceChange((e.target as HTMLSelectElement).value)}
            >
              <option value="default">Default Input Device</option>
              {#each inputDevices as dev}
                <option value={dev.name}>{dev.name} {dev.is_default ? "(Default)" : ""}</option>
              {/each}
            </select>
          </div>

          <div class="setting-item">
            <label for="setting-output-dev">Output Device (Playback)</label>
            <select
              id="setting-output-dev"
              class="settings-select"
              value={selectedOutput}
              onchange={(e) => handleOutputDeviceChange((e.target as HTMLSelectElement).value)}
            >
              <option value="default">Default Output Device</option>
              {#each outputDevices as dev}
                <option value={dev.name}>{dev.name} {dev.is_default ? "(Default)" : ""}</option>
              {/each}
            </select>
          </div>
        </div>

        <div class="settings-group">
          <h4>Volume & Gain Controls</h4>

          <div class="setting-item">
            <div class="setting-label-row">
              <label for="setting-mic-gain">Microphone Input Gain</label>
              <span class="value-badge">{Math.round(micGain * 100)}%</span>
            </div>
            <input
              id="setting-mic-gain"
              type="range"
              min="0"
              max="2"
              step="0.05"
              value={micGain}
              oninput={(e) => handleMicGainChange(parseFloat((e.target as HTMLInputElement).value))}
            />
          </div>

          <div class="setting-item">
            <div class="setting-label-row">
              <label for="setting-master-vol">Master Output Volume</label>
              <span class="value-badge">{Math.round(masterVolume * 100)}%</span>
            </div>
            <input
              id="setting-master-vol"
              type="range"
              min="0"
              max="2"
              step="0.05"
              value={masterVolume}
              oninput={(e) => handleMasterVolumeChange(parseFloat((e.target as HTMLInputElement).value))}
            />
          </div>
        </div>

        <div class="settings-group">
          <h4>Voice Processing</h4>

          <div class="toggle-setting-row">
            <div>
              <div class="toggle-title">RNNoise Noise Suppression</div>
              <div class="toggle-desc">Real-time recurrent neural network audio filter (removes fans, clicks, and background noise).</div>
            </div>
            <button
              class="switch-btn"
              class:checked={rnnoiseEnabled}
              onclick={handleToggleRnnoise}
              aria-label="Toggle RNNoise Noise Suppression"
            >
              <span class="switch-handle"></span>
            </button>
          </div>
        </div>

        <div class="settings-group">
          <div class="setting-header-with-action">
            <div>
              <h4>Microphone Test & Input Sensitivity</h4>
              <div class="toggle-desc">Test your microphone input and listen to your processed voice in real-time.</div>
            </div>
            <button
              class="mic-test-btn"
              class:active={isTestingMic}
              onclick={handleToggleMicTest}
            >
              {#if isTestingMic}
                <span class="pulse-dot"></span> Stop Testing / Остановить
              {:else}
                🎤 Let's Check / Проверить микрофон
              {/if}
            </button>
          </div>

          <div class="mic-meter-card">
            <div class="meter-labels-row">
              <span class="meter-label-title">INPUT LEVEL</span>
              <span class="meter-level-val" class:speaking={localInputLevel.percent >= vadThreshold * 100}>
                {localInputLevel.db <= -59.0 ? "Quiet (-∞ dB)" : `${localInputLevel.db.toFixed(1)} dB (${Math.round(localInputLevel.percent)}%)`}
              </span>
            </div>

            <div class="meter-track-wrapper">
              <div class="segmented-track">
                {#each Array(32) as _, idx}
                  {@const segPercent = (idx / 32) * 100}
                  {@const isActive = smoothedPercent >= segPercent}
                  {@const isSpeechOpen = smoothedPercent >= (vadThreshold * 100)}
                  <div
                    class="meter-segment"
                    class:active={isActive}
                    class:speech-open={isActive && isSpeechOpen}
                    class:gated={isActive && !isSpeechOpen}
                    class:tier-yellow={idx >= 20 && idx < 27}
                    class:tier-red={idx >= 27}
                  ></div>
                {/each}
              </div>

              <!-- Draggable / Visual VAD threshold notch marker -->
              <div
                class="vad-marker-notch"
                style="left: {vadThreshold * 100}%"
                title="Input Sensitivity Threshold: {Math.round(vadThreshold * 100)}%"
              >
                <div class="vad-notch-caret"></div>
                <div class="vad-notch-line"></div>
              </div>
            </div>

            <div class="threshold-control-row">
              <div class="threshold-label-wrap">
                <label for="vad-threshold-slider">Input Sensitivity Threshold (VAD Gate)</label>
                <span class="threshold-val-badge" class:open={localInputLevel.percent >= vadThreshold * 100}>
                  {Math.round(vadThreshold * 100)}% ({localInputLevel.percent >= vadThreshold * 100 ? "VOICE ACTIVE" : "GATE CLOSED"})
                </span>
              </div>
              <input
                id="vad-threshold-slider"
                type="range"
                min="0.05"
                max="0.95"
                step="0.01"
                value={vadThreshold}
                oninput={(e) => handleVadThresholdChange(parseFloat((e.target as HTMLInputElement).value))}
                class="vad-slider"
              />
              <div class="threshold-hint">
                Drag the marker so ambient room noise stays below the gate, and speaking lights up the meter.
              </div>
            </div>
          </div>
        </div>

      {:else if activeTab === "network"}
        <div class="settings-group">
          <h4>P2P Connectivity & Routing</h4>

          <div class="toggle-setting-row">
            <div>
              <div class="toggle-title">Physical Adapter Direct Binding (VPN Bypass)</div>
              <div class="toggle-desc">
                Binds P2P QUIC sockets directly to your physical network interface to prevent TUN/TAP VPN conflicts and ensure direct local hole-punching.
              </div>
            </div>
            <button
              class="switch-btn"
              class:checked={vpnBypassEnabled}
              onclick={handleToggleVpnBypass}
              aria-label="Toggle Physical Adapter VPN Bypass"
            >
              <span class="switch-handle"></span>
            </button>
          </div>
        </div>

        <div class="settings-group">
          <h4>Relay & Topology</h4>
          <div class="info-card">
            <div class="info-item">
              <strong>Topology:</strong> Full-Mesh QUIC Unreliable Datagrams (32kbps Opus)
            </div>
            <div class="info-item">
              <strong>NAT Fallback:</strong> Automatic Host UDP Packet Relay for Symmetric NAT peers
            </div>
            <div class="info-item">
              <strong>Discovery:</strong> Mainline DHT + Pkarr DNS fallback (100% serverless)
            </div>
          </div>
        </div>

      {:else if activeTab === "theme"}
        <ThemeStudio bind:currentTheme {onClose} />
      {:else if activeTab === "sync"}
        <div class="settings-group">
          <h4>Export & Link to Mobile / Second Device</h4>
          <p class="section-desc">
            Export all contacts and message history into an encrypted P2P pairing ticket via iroh-blobs. Share this ticket with your Android phone or another PC to sync your history directly over LAN/P2P.
          </p>
          <button class="btn-primary" onclick={handleGenerateSyncTicket} disabled={syncLoading}>
            {syncLoading ? "Exporting history..." : "Generate Pairing Ticket"}
          </button>

          {#if syncTicket}
            <div class="sync-ticket-box">
              <div class="sync-stats">
                <span>✓ Exported {syncTicket.contacts_count} contacts & {syncTicket.messages_count} messages</span>
              </div>
              <div class="ticket-row">
                <input type="text" class="ticket-input" readonly value={syncTicket.ticket} />
                <button class="btn-secondary" onclick={copySyncTicket}>
                  {copyFeedback ? "Copied! ✓" : "Copy Ticket"}
                </button>
              </div>
            </div>
          {/if}
        </div>

        <div class="settings-group">
          <h4>Import & Synchronize from Another Device</h4>
          <p class="section-desc">
            Paste a pairing ticket generated on another VoxP2P instance to import all contacts and chat history into this device.
          </p>
          <label for="import-sync-ticket-input" class="visually-hidden">Pairing Ticket</label>
          <textarea
            id="import-sync-ticket-input"
            class="sync-textarea"
            placeholder="Paste pairing ticket here..."
            bind:value={importTicketInput}
          ></textarea>
          <button
            class="btn-primary"
            onclick={handleImportSyncTicket}
            disabled={syncLoading || !importTicketInput.trim()}
          >
            {syncLoading ? "Syncing..." : "Sync from Device"}
          </button>

          {#if syncResult}
            <div class="sync-success-box">
              ✓ Successfully synced! Imported {syncResult.contacts_imported} contacts and {syncResult.messages_imported} messages.
            </div>
          {/if}
          {#if syncError}
            <div class="sync-error-box">
              ✕ {syncError}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.65);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
  }

  .settings-dialog {
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    width: 680px;
    max-width: 92vw;
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.5);
  }

  .dialog-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 18px;
    background: var(--bg-secondary);
    border-bottom: var(--border-width) solid var(--border-color);
  }

  .tabs-row {
    display: flex;
    gap: 8px;
  }

  .tab-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-weight: 600;
    font-size: 0.88rem;
    padding: 8px 14px;
    border-radius: var(--border-radius);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .tab-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .tab-btn.active {
    color: #ffffff;
    background: var(--accent-primary);
  }

  .dialog-close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 1.5rem;
    cursor: pointer;
    line-height: 1;
  }

  .dialog-close-btn:hover {
    color: var(--text-primary);
  }

  .dialog-body {
    padding: 22px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .settings-group {
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .settings-group h4 {
    margin: 0;
    font-size: 0.95rem;
    color: var(--text-primary);
  }

  .setting-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .setting-item label {
    font-size: 0.82rem;
    color: var(--text-muted);
  }

  .setting-label-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .value-badge {
    font-size: 0.78rem;
    color: var(--text-primary);
    font-family: monospace;
  }

  .settings-select {
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 8px 12px;
    color: var(--text-primary);
    font-size: 0.85rem;
  }

  .settings-select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .setting-item input[type="range"] {
    accent-color: var(--accent-primary);
  }

  .toggle-setting-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
  }

  .toggle-title {
    font-size: 0.88rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .toggle-desc {
    font-size: 0.78rem;
    color: var(--text-muted);
    margin-top: 2px;
    max-width: 440px;
  }

  .switch-btn {
    width: 46px;
    height: 24px;
    background: var(--bg-tertiary);
    border: var(--border-width) solid var(--border-color);
    border-radius: 12px;
    position: relative;
    cursor: pointer;
    transition: background 0.2s ease;
    flex-shrink: 0;
  }

  .switch-btn.checked {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .switch-handle {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 18px;
    height: 18px;
    background: #ffffff;
    border-radius: 50%;
    transition: transform 0.2s ease;
  }

  .switch-btn.checked .switch-handle {
    transform: translateX(22px);
  }

  .info-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--surface-card);
    padding: 12px;
    border-radius: var(--border-radius);
    font-size: 0.82rem;
    color: var(--text-primary);
  }

  .info-item strong {
    color: var(--accent-primary);
  }

  .section-desc {
    margin: 0;
    font-size: 0.82rem;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .btn-primary {
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    border-radius: var(--border-radius);
    padding: 8px 16px;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
    align-self: flex-start;
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 8px 14px;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .sync-ticket-box {
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--surface-card);
    padding: 12px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
  }

  .sync-stats {
    font-size: 0.82rem;
    color: var(--online-status-color);
    font-weight: 600;
  }

  .ticket-row {
    display: flex;
    gap: 8px;
  }

  .ticket-input {
    flex: 1;
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 8px 10px;
    color: var(--text-primary);
    font-size: 0.8rem;
    font-family: monospace;
  }

  .sync-textarea {
    width: 100%;
    min-height: 70px;
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 10px;
    color: var(--text-primary);
    font-size: 0.82rem;
    font-family: monospace;
    resize: vertical;
  }

  .sync-textarea:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .sync-success-box {
    background: rgba(16, 185, 129, 0.15);
    border: 1px solid var(--online-status-color);
    border-radius: var(--border-radius);
    padding: 10px 14px;
    color: var(--online-status-color);
    font-size: 0.85rem;
  }

  .sync-error-box {
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid var(--danger-color);
    border-radius: var(--border-radius);
    padding: 10px 14px;
    color: var(--danger-color);
    font-size: 0.85rem;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  /* Mic Test & Discord Meter Styles */
  .setting-header-with-action {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }

  .mic-test-btn {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 8px 14px;
    font-size: 0.84rem;
    font-weight: 600;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 8px;
    transition: all 0.2s ease;
  }

  .mic-test-btn:hover {
    border-color: var(--accent-primary);
    background: var(--surface-card);
  }

  .mic-test-btn.active {
    background: rgba(239, 68, 68, 0.15);
    border-color: var(--danger-color);
    color: var(--danger-color);
  }

  .pulse-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--danger-color);
    box-shadow: 0 0 8px var(--danger-color);
    animation: pulse 1s infinite alternate;
  }

  @keyframes pulse {
    0% { opacity: 0.4; transform: scale(0.9); }
    100% { opacity: 1; transform: scale(1.2); }
  }

  .mic-meter-card {
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .meter-labels-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.76rem;
    font-weight: 700;
    letter-spacing: 0.05em;
  }

  .meter-label-title {
    color: var(--text-muted);
  }

  .meter-level-val {
    color: var(--text-muted);
    font-family: monospace;
    transition: color 0.1s ease;
  }

  .meter-level-val.speaking {
    color: var(--online-status-color);
    font-weight: 700;
  }

  .meter-track-wrapper {
    position: relative;
    width: 100%;
    height: 24px;
    display: flex;
    align-items: center;
  }

  .segmented-track {
    display: flex;
    width: 100%;
    height: 16px;
    gap: 3px;
    background: var(--bg-primary);
    padding: 3px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
    box-sizing: border-box;
  }

  .meter-segment {
    flex: 1;
    height: 100%;
    border-radius: 1px;
    background: rgba(255, 255, 255, 0.06);
    transition: background 0.04s ease-out;
  }

  /* Segment Lighting Levels */
  .meter-segment.active.gated {
    background: rgba(140, 145, 155, 0.5); /* Ambient sound below threshold */
  }

  .meter-segment.active.speech-open {
    background: #23a55a; /* Discord green */
    box-shadow: 0 0 3px rgba(35, 165, 90, 0.8);
  }

  .meter-segment.active.speech-open.tier-yellow {
    background: #f0b232; /* Discord yellow/gold */
    box-shadow: 0 0 4px rgba(240, 178, 50, 0.8);
  }

  .meter-segment.active.speech-open.tier-red {
    background: #f23f43; /* Discord red peak */
    box-shadow: 0 0 5px rgba(242, 63, 67, 0.9);
  }

  .vad-marker-notch {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    transform: translateX(-50%);
    pointer-events: none;
    z-index: 2;
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .vad-notch-caret {
    width: 0;
    height: 0;
    border-left: 5px solid transparent;
    border-right: 5px solid transparent;
    border-top: 6px solid #ffffff;
  }

  .vad-notch-line {
    flex: 1;
    width: 2px;
    background: #ffffff;
    opacity: 0.85;
    box-shadow: 0 0 4px rgba(255, 255, 255, 0.6);
  }

  .threshold-control-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 4px;
  }

  .threshold-label-wrap {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .threshold-val-badge {
    font-size: 0.74rem;
    font-weight: 700;
    font-family: monospace;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    color: var(--text-muted);
  }

  .threshold-val-badge.open {
    background: rgba(35, 165, 90, 0.2);
    color: #23a55a;
    border: 1px solid rgba(35, 165, 90, 0.4);
  }

  .vad-slider {
    width: 100%;
    accent-color: var(--accent-primary);
    cursor: pointer;
  }

  .threshold-hint {
    font-size: 0.74rem;
    color: var(--text-muted);
    font-style: italic;
  }
</style>
