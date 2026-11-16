<script lang="ts">
  import type { AudioDeviceInfo } from "../types";
  import type { ThemeConfig } from "../theme";
  import { api } from "../api";
  import ThemeStudio from "./ThemeStudio.svelte";

  let {
    currentTheme = $bindable(),
    onClose,
  }: {
    currentTheme: ThemeConfig;
    onClose: () => void;
  } = $props();

  let activeTab = $state<"audio" | "network" | "theme">("audio");

  // Audio State
  let inputDevices = $state<AudioDeviceInfo[]>([]);
  let outputDevices = $state<AudioDeviceInfo[]>([]);
  let selectedInput = $state<string>("default");
  let selectedOutput = $state<string>("default");
  let micGain = $state<number>(1.0);
  let masterVolume = $state<number>(1.0);
  let rnnoiseEnabled = $state<boolean>(true);

  // Network State
  let vpnBypassEnabled = $state<boolean>(true);

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
  });

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
</script>

<div
  class="modal-backdrop"
  onclick={(e) => { if (e.target === e.currentTarget) onClose(); }}
  onkeydown={(e) => e.key === "Escape" && onClose()}
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
      </div>
      <button class="dialog-close-btn" onclick={onClose}>&times;</button>
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
</style>
