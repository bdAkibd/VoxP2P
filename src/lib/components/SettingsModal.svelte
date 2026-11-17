<script lang="ts">
  import type {
    AudioDeviceInfo,
    DeviceSyncTicket,
    ImportSyncResult,
    DeviceSyncTicketPayload,
    DeviceSyncImportResult,
    SyncHostCompletedPayload,
    UserProfile,
  } from "../types";
  import type { ThemeConfig } from "../theme";
  import { api } from "../api";
  import ThemeStudio from "./ThemeStudio.svelte";
  import AvatarCropperModal from "./AvatarCropperModal.svelte";
  import { scan, Format } from "@tauri-apps/plugin-barcode-scanner";

  let {
    currentTheme = $bindable(),
    initialTab = "account",
    onClose,
    onSyncCompleted,
  }: {
    currentTheme: ThemeConfig;
    initialTab?: "account" | "audio" | "network" | "theme" | "sync";
    onClose: () => void;
    onSyncCompleted?: () => void;
  } = $props();

  let activeTab = $state<"account" | "audio" | "network" | "theme" | "sync">("account");

  $effect(() => {
    if (initialTab) {
      activeTab = initialTab;
    }
  });

  // Account State
  let profile = $state<UserProfile | null>(null);
  let nicknameInput = $state<string>("");
  let avatarDataUrl = $state<string | null>(null);
  let isSavingProfile = $state<boolean>(false);
  let profileSaveSuccess = $state<boolean>(false);
  let profileError = $state<string | null>(null);
  let copyNodeIdFeedback = $state<boolean>(false);
  let fileInputEl = $state<HTMLInputElement | null>(null);
  let cropperImageSrc = $state<string | null>(null);

  async function loadProfile() {
    try {
      const p = await api.getMyProfile();
      profile = p;
      nicknameInput = p.nickname;
      if (p.avatar_hash) {
        const url = await api.getAvatar(p.avatar_hash);
        avatarDataUrl = url;
      } else {
        avatarDataUrl = null;
      }
    } catch (e: any) {
      console.error("Failed to load profile:", e);
    }
  }

  function handleAvatarClick() {
    fileInputEl?.click();
  }

  function handleFileSelected(e: Event) {
    const input = e.target as HTMLInputElement;
    if (!input.files || input.files.length === 0) return;
    const file = input.files[0];
    const reader = new FileReader();
    reader.onload = () => {
      cropperImageSrc = reader.result as string;
      input.value = "";
    };
    reader.readAsDataURL(file);
  }

  async function handleCropperApply(bytes: Uint8Array) {
    try {
      profileError = null;
      const updated = await api.updateProfileAvatar(bytes);
      profile = updated;
      if (updated.avatar_hash) {
        const url = await api.getAvatar(updated.avatar_hash);
        avatarDataUrl = url;
      }
      cropperImageSrc = null;
      onSyncCompleted?.();
    } catch (err: any) {
      profileError = err?.toString() || "Failed to update avatar";
    }
  }

  async function handleRemoveAvatar() {
    if (!profile) return;
    try {
      profileError = null;
      const updated = await api.updateProfile(profile.nickname, null);
      profile = updated;
      avatarDataUrl = null;
      onSyncCompleted?.();
    } catch (err: any) {
      profileError = err?.toString() || "Failed to remove avatar";
    }
  }

  async function handleSaveNickname() {
    if (!profile || !nicknameInput.trim()) return;
    isSavingProfile = true;
    profileError = null;
    try {
      const updated = await api.updateProfile(nicknameInput.trim(), profile.avatar_hash);
      profile = updated;
      profileSaveSuccess = true;
      onSyncCompleted?.();
      setTimeout(() => {
        profileSaveSuccess = false;
      }, 2500);
    } catch (err: any) {
      profileError = err?.toString() || "Failed to update nickname";
    } finally {
      isSavingProfile = false;
    }
  }

  function handleCopyNodeId() {
    if (!profile?.node_id) return;
    navigator.clipboard.writeText(profile.node_id);
    copyNodeIdFeedback = true;
    setTimeout(() => {
      copyNodeIdFeedback = false;
    }, 2000);
  }

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

  // QR Code Host State
  let qrHostPayload = $state<DeviceSyncTicketPayload | null>(null);
  let qrHostLoading = $state<boolean>(false);
  let qrCountdown = $state<number>(0);
  let qrHostSuccess = $state<SyncHostCompletedPayload | null>(null);
  let qrHostError = $state<string | null>(null);
  let copyQrFeedback = $state<boolean>(false);
  let countdownTimer: any = null;

  // QR Code Client / Import State
  let qrImportInput = $state<string>("");
  let qrImportLoading = $state<boolean>(false);
  let qrImportError = $state<string | null>(null);
  let qrImportResult = $state<DeviceSyncImportResult | null>(null);
  let isScanningCamera = $state<boolean>(false);
  let cameraVideoEl = $state<HTMLVideoElement | null>(null);
  let cameraStream: MediaStream | null = null;
  let cameraScanInterval: any = null;

  // Legacy Blob Sync State
  let syncTicket = $state<DeviceSyncTicket | null>(null);
  let importTicketInput = $state<string>("");
  let syncResult = $state<ImportSyncResult | null>(null);
  let syncLoading = $state<boolean>(false);
  let syncError = $state<string | null>(null);
  let copyFeedback = $state<boolean>(false);

  $effect(() => {
    loadProfile();

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
    let unlistenSyncHost: (() => void) | undefined;

    // Listen for sync host completion
    api.onSyncHostCompleted((payload) => {
      qrHostSuccess = payload;
      if (countdownTimer) {
        clearInterval(countdownTimer);
        countdownTimer = null;
      }
      onSyncCompleted?.();
    }).then((u) => {
      unlistenSyncHost = u;
    });

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
      unlistenSyncHost?.();
      if (isTestingMic) {
        api.setMicTest(false).catch(console.error);
      }
      api.setMeterActive(false).catch(console.error);
      if (countdownTimer) {
        clearInterval(countdownTimer);
        countdownTimer = null;
      }
      stopCamera();
    };
  });

  function handleModalClose() {
    if (isTestingMic) {
      isTestingMic = false;
      api.setMicTest(false).catch(console.error);
    }
    api.setMeterActive(false).catch(console.error);
    if (countdownTimer) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
    stopCamera();
    onClose();
  }

  // QR Sync Host Methods
  async function handleStartQrHost() {
    qrHostLoading = true;
    qrHostError = null;
    qrHostSuccess = null;
    if (countdownTimer) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
    try {
      const payload = await api.startDeviceSyncHost();
      qrHostPayload = payload;
      qrCountdown = payload.expires_in_secs || 120;
      countdownTimer = setInterval(() => {
        if (qrCountdown > 1) {
          qrCountdown -= 1;
        } else {
          qrCountdown = 0;
          clearInterval(countdownTimer);
          countdownTimer = null;
        }
      }, 1000);
    } catch (err: any) {
      qrHostError = err?.toString() || "Failed to start sync host";
    } finally {
      qrHostLoading = false;
    }
  }

  function handleStopQrHost() {
    if (countdownTimer) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
    qrHostPayload = null;
    qrCountdown = 0;
    qrHostSuccess = null;
    qrHostError = null;
  }

  function copyQrTicket() {
    if (!qrHostPayload?.ticket) return;
    navigator.clipboard.writeText(qrHostPayload.ticket);
    copyQrFeedback = true;
    setTimeout(() => {
      copyQrFeedback = false;
    }, 2000);
  }

  // QR Sync Client / Import Methods
  async function handleImportQr(ticketStr?: string) {
    const target = (ticketStr || qrImportInput).trim();
    if (!target) return;
    stopCamera();
    qrImportLoading = true;
    qrImportError = null;
    qrImportResult = null;
    try {
      const res = await api.importFromSyncQr(target);
      qrImportResult = res;
      onSyncCompleted?.();
    } catch (err: any) {
      qrImportError = err?.toString() || "Failed to import account from QR sync ticket";
    } finally {
      qrImportLoading = false;
    }
  }

  async function handleScanQrWithCamera() {
    qrImportError = null;
    try {
      const scanned = await scan({
        formats: [Format.QRCode],
        cameraDirection: "back",
        windowed: false,
      });
      if (scanned && scanned.content) {
        qrImportInput = scanned.content;
        await handleImportQr(scanned.content);
        return;
      }
    } catch (err: any) {
      console.warn("Tauri barcode scanner unavailable or failed, falling back to camera stream:", err);
    }

    // Graceful Web/HTML5 camera stream fallback
    try {
      if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) {
        isScanningCamera = true;
        cameraStream = await navigator.mediaDevices.getUserMedia({
          video: { facingMode: "environment" },
        });
        setTimeout(async () => {
          if (cameraVideoEl && cameraStream) {
            cameraVideoEl.srcObject = cameraStream;
            await cameraVideoEl.play().catch(console.error);
            startVideoQrDetector();
          }
        }, 50);
      } else {
        qrImportError = "Camera not supported on this platform. Please paste the sync ticket manually below.";
      }
    } catch (camErr: any) {
      isScanningCamera = false;
      qrImportError = "Unable to access camera: " + (camErr?.message || camErr) + ". Please paste ticket manually.";
    }
  }

  function stopCamera() {
    isScanningCamera = false;
    if (cameraScanInterval) {
      clearInterval(cameraScanInterval);
      cameraScanInterval = null;
    }
    if (cameraStream) {
      cameraStream.getTracks().forEach((t) => t.stop());
      cameraStream = null;
    }
  }

  function startVideoQrDetector() {
    if (cameraScanInterval) clearInterval(cameraScanInterval);
    const BarcodeDetectorClass = (window as any).BarcodeDetector;
    if (BarcodeDetectorClass) {
      const detector = new BarcodeDetectorClass({ formats: ["qr_code"] });
      cameraScanInterval = setInterval(async () => {
        if (!cameraVideoEl || cameraVideoEl.readyState < 2) return;
        try {
          const barcodes = await detector.detect(cameraVideoEl);
          if (barcodes && barcodes.length > 0) {
            const content = barcodes[0].rawValue;
            if (content && content.startsWith("p2psync://")) {
              stopCamera();
              qrImportInput = content;
              await handleImportQr(content);
            }
          }
        } catch {
          // Continue detection loop
        }
      }, 300);
    }
  }

  async function handlePasteTicket() {
    try {
      const text = await navigator.clipboard.readText();
      if (text) {
        qrImportInput = text.trim();
      }
    } catch (err) {
      console.warn("Failed to read clipboard:", err);
    }
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
          class:active={activeTab === "account"}
          onclick={() => (activeTab = "account")}
        >
          👤 Аккаунт
        </button>
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
          📱 Device Sync & QR
        </button>
      </div>
      <button class="dialog-close-btn" onclick={handleModalClose}>&times;</button>
    </div>

    <!-- Dialog Content Body -->
    <div class="dialog-body">
      {#if activeTab === "account"}
        <div class="settings-group">
          <h4>👤 Мой аккаунт (My Account)</h4>
          <p class="section-desc">
            Настройте ваш P2P профиль. Изменения аватара и имени мгновенно синхронизируются с контактами и участниками комнат.
          </p>

          {#if profileError}
            <div class="alert-error">
              ⚠️ {profileError}
            </div>
          {/if}

          <!-- Avatar Section -->
          <div class="account-avatar-card">
            <div
              class="avatar-edit-wrapper"
              onclick={handleAvatarClick}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === "Enter" && handleAvatarClick()}
              title="Нажмите, чтобы изменить аватар"
            >
              <div class="avatar-large-preview">
                {#if avatarDataUrl}
                  <img src={avatarDataUrl} alt="Аватар" class="avatar-img-large" />
                {:else}
                  <div class="avatar-placeholder-large">
                    {profile?.nickname ? profile.nickname.slice(0, 1).toUpperCase() : "U"}
                  </div>
                {/if}
              </div>
              <!-- Hover Overlay (Change Avatar) -->
              <div class="avatar-hover-overlay">
                <span class="overlay-icon">📷</span>
                <span class="overlay-text">Сменить аватар</span>
              </div>
            </div>

            <!-- Hidden File Input -->
            <input
              type="file"
              bind:this={fileInputEl}
              onchange={handleFileSelected}
              accept="image/png, image/jpeg, image/webp, image/gif"
              style="display: none;"
            />

            <div class="avatar-actions-info">
              <div class="avatar-title">{profile?.nickname || "Пользователь"}</div>
              <div class="avatar-hint">
                Поддерживаются PNG, JPG, WEBP или GIF. Кликните для интерактивной обрезки (Steam/Discord стиль).
              </div>
              <div class="avatar-btn-row">
                <button class="btn-secondary-action" onclick={handleAvatarClick}>
                  📷 Загрузить фото
                </button>
                {#if avatarDataUrl || profile?.avatar_hash}
                  <button class="btn-danger-action" onclick={handleRemoveAvatar}>
                    🗑️ Удалить аватар
                  </button>
                {/if}
              </div>
            </div>
          </div>

          <!-- Nickname Editing -->
          <div class="setting-item">
            <label for="profile-nickname-input">Имя пользователя (Никнейм)</label>
            <div class="nickname-input-row">
              <input
                id="profile-nickname-input"
                type="text"
                bind:value={nicknameInput}
                placeholder="Введите никнейм..."
                maxlength="32"
                class="account-text-input"
                onkeydown={(e) => e.key === "Enter" && handleSaveNickname()}
              />
              <button
                class="btn-save-nickname"
                onclick={handleSaveNickname}
                disabled={isSavingProfile || !nicknameInput.trim() || nicknameInput.trim() === profile?.nickname}
              >
                {#if isSavingProfile}
                  Сохранение...
                {:else if profileSaveSuccess}
                  ✓ Сохранено!
                {:else}
                  Сохранить
                {/if}
              </button>
            </div>
            <span class="item-desc">
              Ваш публичный псевдоним, отображаемый в звонках и сообщениях.
            </span>
          </div>

          <!-- Account ID (NodeId) -->
          <div class="setting-item">
            <label for="profile-node-id-val">Уникальный P2P адрес (Node ID)</label>
            <div class="node-id-badge-card">
              <div class="node-id-key-icon">🔑</div>
              <div class="node-id-text-wrap">
                <code class="node-id-code" id="profile-node-id-val">{profile?.node_id || "Загрузка ID..."}</code>
                <span class="node-id-caption">Используется для прямого подключения (1-на-1 звонки и Briar DM) без серверов</span>
              </div>
              <button
                class="btn-copy-id"
                onclick={handleCopyNodeId}
                title="Копировать NodeId в буфер обмена"
              >
                {#if copyNodeIdFeedback}
                  ✓ Скопировано!
                {:else}
                  📋 Копировать ID
                {/if}
              </button>
            </div>
          </div>
        </div>
      {:else if activeTab === "audio"}
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
        <!-- QR CODE HOST & CLIENT SYNC -->
        <div class="settings-group">
          <h4>📱 Link Mobile Device (QR Code Sync)</h4>
          <p class="section-desc">
            Instantly clone your account identity, contacts, and message history to your Android phone or companion device with end-to-end P2P encryption over ALPN <code>/p2pchat/sync/1</code>.
          </p>

          <div class="sync-grid-container">
            <!-- Left Card: Host QR Generator -->
            <div class="sync-card">
              <div class="sync-card-header">
                <h5>🖥️ This Device (Host)</h5>
                <span class="sync-role-badge">Export Identity</span>
              </div>
              <p class="card-desc">
                Generate a temporary, one-time authenticated QR code to clone this account onto your mobile phone.
              </p>

              {#if !qrHostPayload}
                <button
                  class="btn-primary"
                  onclick={handleStartQrHost}
                  disabled={qrHostLoading}
                >
                  {qrHostLoading ? "Starting P2P Sync Host..." : "⚡ Generate Sync QR Code"}
                </button>
              {:else}
                <div class="qr-presentation">
                  <div class="qr-code-svg-wrap">
                    {@html qrHostPayload.qr_svg}
                  </div>

                  {#if qrCountdown > 0}
                    <div class="qr-status-indicator">
                      <span class="pulse-dot-green"></span>
                      <span>Active &bull; Valid for {qrCountdown}s</span>
                    </div>
                  {:else}
                    <div class="qr-status-expired">
                      ⚠️ Session Expired (120s limit). Click below to generate a new QR code.
                    </div>
                  {/if}

                  <div class="qr-ticket-actions">
                    <button class="btn-secondary qr-copy-btn" onclick={copyQrTicket}>
                      {copyQrFeedback ? "Copied! ✓" : "📋 Copy Sync Ticket"}
                    </button>
                    {#if qrCountdown === 0}
                      <button class="btn-primary" onclick={handleStartQrHost} disabled={qrHostLoading}>
                        🔄 Regenerate QR
                      </button>
                    {:else}
                      <button class="btn-cancel-sync" onclick={handleStopQrHost}>
                        ✕ Stop Session
                      </button>
                    {/if}
                  </div>
                </div>
              {/if}

              {#if qrHostSuccess}
                <div class="sync-success-box highlight">
                  ✓ Mobile device linked successfully! Cloned account with <strong>{qrHostSuccess.nickname}</strong>.
                </div>
              {/if}
              {#if qrHostError}
                <div class="sync-error-box">
                  ✕ {qrHostError}
                </div>
              {/if}
            </div>

            <!-- Right Card: Scan / Import -->
            <div class="sync-card">
              <div class="sync-card-header">
                <h5>📱 Companion Device (Client)</h5>
                <span class="sync-role-badge">Import Identity</span>
              </div>
              <p class="card-desc">
                Scan the QR code displayed on your PC, or paste the sync ticket to clone the identity directly.
              </p>

              <div class="scan-actions-group">
                <button
                  class="btn-primary scan-btn"
                  onclick={handleScanQrWithCamera}
                  disabled={qrImportLoading || isScanningCamera}
                >
                  📷 Scan QR with Camera
                </button>

                {#if isScanningCamera}
                  <div class="camera-preview-container">
                    <div class="camera-preview-header">
                      <span>Point camera at QR code</span>
                      <button class="camera-close-sm" onclick={stopCamera}>&times;</button>
                    </div>
                    <div class="camera-viewport">
                      <video
                        bind:this={cameraVideoEl}
                        playsinline
                        autoplay
                        muted
                        class="camera-stream-video"
                      ></video>
                      <div class="camera-scan-frame"></div>
                    </div>
                    <span class="camera-status-text">Scanning for QR code...</span>
                  </div>
                {/if}

                <div class="manual-input-section">
                  <label for="qr-ticket-paste" class="input-sublabel">Or paste Sync Ticket:</label>
                  <div class="manual-ticket-row">
                    <input
                      id="qr-ticket-paste"
                      type="text"
                      class="ticket-input"
                      placeholder="p2psync://<ticket>/<token>"
                      bind:value={qrImportInput}
                    />
                    <button class="btn-secondary" onclick={handlePasteTicket} title="Paste from clipboard">
                      📋 Paste
                    </button>
                  </div>
                  <button
                    class="btn-primary full-width"
                    onclick={() => handleImportQr()}
                    disabled={qrImportLoading || !qrImportInput.trim()}
                  >
                    {qrImportLoading ? "Cloning Account & History..." : "Import Account from Ticket"}
                  </button>
                </div>

                {#if qrImportResult}
                  <div class="sync-success-box highlight">
                    ✓ Account Cloned Successfully! Identity: <strong>{qrImportResult.nickname}</strong> ({qrImportResult.contacts_imported} contacts & {qrImportResult.messages_imported} messages synced).
                  </div>
                {/if}
                {#if qrImportError}
                  <div class="sync-error-box">
                    ✕ {qrImportError}
                  </div>
                {/if}
              </div>
            </div>
          </div>
        </div>

        <!-- Advanced Blob History Export (Legacy / Backup) -->
        <div class="settings-group collapsed-group">
          <details class="advanced-sync-details">
            <summary class="advanced-sync-summary">
              📦 Advanced: Blob Archive Export & Import
            </summary>
            <div class="advanced-sync-content">
              <p class="section-desc">
                Export chat history snapshot as an independent content-addressed iroh-blob ticket without overwriting your device identity.
              </p>
              <button class="btn-secondary" onclick={handleGenerateSyncTicket} disabled={syncLoading}>
                {syncLoading ? "Exporting blob..." : "Generate Blob Ticket"}
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

              <div class="import-blob-block">
                <label for="import-sync-ticket-input" class="input-sublabel">Import Blob History Ticket:</label>
                <textarea
                  id="import-sync-ticket-input"
                  class="sync-textarea"
                  placeholder="Paste blob ticket here..."
                  bind:value={importTicketInput}
                ></textarea>
                <button
                  class="btn-secondary"
                  onclick={handleImportSyncTicket}
                  disabled={syncLoading || !importTicketInput.trim()}
                >
                  {syncLoading ? "Importing..." : "Import Messages from Blob"}
                </button>
                {#if syncResult}
                  <div class="sync-success-box">
                    ✓ Synced {syncResult.contacts_imported} contacts & {syncResult.messages_imported} messages.
                  </div>
                {/if}
                {#if syncError}
                  <div class="sync-error-box">
                    ✕ {syncError}
                  </div>
                {/if}
              </div>
            </div>
          </details>
        </div>
      {/if}
    </div>
  </div>
</div>

{#if cropperImageSrc}
  <AvatarCropperModal
    imageSrc={cropperImageSrc}
    onApply={handleCropperApply}
    onCancel={() => {
      cropperImageSrc = null;
    }}
  />
{/if}

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

  /* Account Tab Styling */
  .account-avatar-card {
    display: flex;
    align-items: center;
    gap: 20px;
    background: var(--surface-card, var(--bg-secondary));
    padding: 18px 20px;
    border-radius: var(--border-radius, 10px);
    border: var(--border-width) solid var(--border-color);
    margin-bottom: 20px;
  }

  .avatar-edit-wrapper {
    position: relative;
    width: 96px;
    height: 96px;
    border-radius: 50%;
    overflow: hidden;
    cursor: pointer;
    flex-shrink: 0;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35);
    border: 2px solid var(--accent-primary, #6366f1);
    transition: transform 0.15s ease, box-shadow 0.15s ease;
  }

  .avatar-edit-wrapper:hover,
  .avatar-edit-wrapper:focus-visible {
    transform: scale(1.03);
    box-shadow: 0 6px 20px rgba(99, 102, 241, 0.4);
    outline: none;
  }

  .avatar-large-preview {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
  }

  .avatar-img-large {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .avatar-placeholder-large {
    font-size: 2.4rem;
    font-weight: 700;
    color: var(--text-primary);
    user-select: none;
  }

  .avatar-hover-overlay {
    position: absolute;
    inset: 0;
    background: rgba(15, 17, 23, 0.72);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: #ffffff;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.2s ease;
  }

  .avatar-edit-wrapper:hover .avatar-hover-overlay,
  .avatar-edit-wrapper:focus-visible .avatar-hover-overlay {
    opacity: 1;
  }

  .overlay-icon {
    font-size: 1.3rem;
    margin-bottom: 2px;
  }

  .overlay-text {
    font-size: 0.65rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    text-align: center;
    padding: 0 6px;
  }

  .avatar-actions-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .avatar-title {
    font-size: 1.15rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .avatar-hint {
    font-size: 0.8rem;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .avatar-btn-row {
    display: flex;
    gap: 10px;
    margin-top: 6px;
    flex-wrap: wrap;
  }

  .btn-secondary-action {
    padding: 7px 14px;
    background: var(--bg-tertiary);
    border: var(--border-width) solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-size: 0.85rem;
    font-weight: 500;
    transition: background 0.15s, border-color 0.15s;
  }

  .btn-secondary-action:hover {
    background: var(--surface-card, var(--bg-secondary));
    border-color: var(--accent-primary);
  }

  .btn-danger-action {
    padding: 7px 14px;
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.35);
    border-radius: 6px;
    color: #fca5a5;
    cursor: pointer;
    font-size: 0.85rem;
    font-weight: 500;
    transition: background 0.15s;
  }

  .btn-danger-action:hover {
    background: rgba(239, 68, 68, 0.22);
  }

  .nickname-input-row {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-top: 6px;
  }

  .account-text-input {
    flex: 1;
    height: 40px;
    padding: 0 12px;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.95rem;
    transition: border-color 0.15s;
  }

  .account-text-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .btn-save-nickname {
    height: 40px;
    padding: 0 18px;
    background: var(--accent-primary);
    border: none;
    border-radius: 6px;
    color: #ffffff;
    font-weight: 600;
    font-size: 0.9rem;
    cursor: pointer;
    min-width: 110px;
    transition: background 0.15s, opacity 0.15s;
  }

  .btn-save-nickname:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .btn-save-nickname:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .node-id-badge-card {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    background: var(--surface-card, var(--bg-secondary));
    border: var(--border-width) solid var(--border-color);
    border-radius: 8px;
    margin-top: 6px;
  }

  .node-id-key-icon {
    font-size: 1.4rem;
    flex-shrink: 0;
  }

  .node-id-text-wrap {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  .node-id-code {
    font-family: monospace;
    font-size: 0.82rem;
    color: var(--text-primary);
    word-break: break-all;
    background: var(--bg-primary);
    padding: 4px 8px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
  }

  .node-id-caption {
    font-size: 0.74rem;
    color: var(--text-muted);
  }

  .btn-copy-id {
    height: 38px;
    padding: 0 14px;
    background: var(--bg-tertiary);
    border: var(--border-width) solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.85rem;
    font-weight: 500;
    cursor: pointer;
    flex-shrink: 0;
    white-space: nowrap;
    transition: background 0.15s, border-color 0.15s;
  }

  .btn-copy-id:hover {
    background: var(--surface-card, var(--bg-secondary));
    border-color: var(--accent-primary);
  }

  .alert-error {
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.4);
    color: #fca5a5;
    padding: 10px 14px;
    border-radius: 6px;
    font-size: 0.85rem;
    margin-bottom: 12px;
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

  @media (max-width: 768px) {
    .settings-dialog {
      width: 100vw;
      max-width: 100vw;
      height: 100vh;
      max-height: 100vh;
      border-radius: 0;
      border: none;
    }

    .tabs-row {
      overflow-x: auto;
      padding-bottom: 4px;
      -webkit-overflow-scrolling: touch;
    }

    .tab-btn {
      white-space: nowrap;
      padding: 8px 10px;
      font-size: 0.8rem;
    }

    .dialog-close-btn {
      min-width: 44px;
      min-height: 44px;
      display: flex;
      align-items: center;
      justify-content: center;
    }

    .dialog-body {
      padding: 16px;
    }
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

  /* QR Code & Device Sync Styles */
  .sync-grid-container {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 16px;
    margin-top: 8px;
  }

  .sync-card {
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .sync-card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .sync-card-header h5 {
    margin: 0;
    font-size: 0.95rem;
    color: var(--text-primary);
  }

  .sync-role-badge {
    font-size: 0.7rem;
    padding: 3px 8px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    color: var(--accent-primary);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    border: var(--border-width) solid var(--border-color);
  }

  .card-desc {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .qr-presentation {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .qr-code-svg-wrap {
    background: #ffffff;
    padding: 14px;
    border-radius: 12px;
    width: 210px;
    height: 210px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    display: flex;
    justify-content: center;
    align-items: center;
  }

  .qr-code-svg-wrap :global(svg) {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }

  .qr-status-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--online-status-color);
    background: rgba(16, 185, 129, 0.1);
    padding: 4px 12px;
    border-radius: 14px;
    border: 1px solid rgba(16, 185, 129, 0.3);
  }

  .pulse-dot-green {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--online-status-color);
    box-shadow: 0 0 6px var(--online-status-color);
    animation: pulse 1.2s infinite alternate;
  }

  .qr-status-expired {
    font-size: 0.8rem;
    color: var(--danger-color);
    text-align: center;
    background: rgba(239, 68, 68, 0.1);
    padding: 6px 10px;
    border-radius: 6px;
    border: 1px solid rgba(239, 68, 68, 0.3);
  }

  .qr-ticket-actions {
    display: flex;
    gap: 8px;
    justify-content: center;
    width: 100%;
    flex-wrap: wrap;
  }

  .btn-cancel-sync {
    background: transparent;
    color: var(--danger-color);
    border: var(--border-width) solid var(--danger-color);
    padding: 7px 12px;
    border-radius: var(--border-radius);
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-cancel-sync:hover {
    background: var(--danger-color);
    color: #ffffff;
  }

  .scan-actions-group {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .scan-btn {
    width: 100%;
    padding: 10px 16px;
    font-size: 0.9rem;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .camera-preview-container {
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 10px;
  }

  .camera-preview-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .camera-close-sm {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 1.1rem;
    cursor: pointer;
    line-height: 1;
  }

  .camera-close-sm:hover {
    color: var(--text-primary);
  }

  .camera-viewport {
    position: relative;
    width: 100%;
    height: 170px;
    border-radius: 6px;
    overflow: hidden;
    background: #000000;
  }

  .camera-stream-video {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .camera-scan-frame {
    position: absolute;
    inset: 18px;
    border: 2px dashed var(--accent-primary);
    border-radius: 8px;
    pointer-events: none;
    box-shadow: 0 0 0 1000px rgba(0, 0, 0, 0.45);
  }

  .camera-status-text {
    font-size: 0.75rem;
    color: var(--text-muted);
    text-align: center;
    font-style: italic;
  }

  .manual-input-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .input-sublabel {
    font-size: 0.8rem;
    color: var(--text-muted);
    font-weight: 500;
  }

  .manual-ticket-row {
    display: flex;
    gap: 6px;
  }

  .full-width {
    width: 100%;
  }

  .highlight {
    font-weight: 500;
    font-size: 0.85rem;
  }

  .collapsed-group {
    background: transparent;
    border-top: var(--border-width) solid var(--border-color);
    padding-top: 14px;
    margin-top: 6px;
  }

  .advanced-sync-details {
    width: 100%;
  }

  .advanced-sync-summary {
    font-size: 0.85rem;
    color: var(--text-muted);
    cursor: pointer;
    font-weight: 600;
    user-select: none;
  }

  .advanced-sync-summary:hover {
    color: var(--text-primary);
  }

  .advanced-sync-content {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 10px;
    padding-left: 10px;
    border-left: 2px solid var(--border-color);
  }

  .import-blob-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 6px;
  }
</style>
