<script lang="ts">
  import { onMount, onDestroy } from "svelte";
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
  import { t, i18n, SUPPORTED_LOCALES, setLocale } from "../i18n";
  import ThemeStudio from "./ThemeStudio.svelte";
  import AvatarCropperModal from "./AvatarCropperModal.svelte";
  import Avatar from "./Avatar.svelte";
  import UserManual from "./UserManual.svelte";
  import { scan, Format, checkPermissions, requestPermissions } from "@tauri-apps/plugin-barcode-scanner";

  let {
    currentTheme = $bindable(),
    initialTab = "account",
    onClose,
    onSyncCompleted,
  }: {
    currentTheme: ThemeConfig;
    initialTab?: "account" | "audio" | "network" | "theme" | "sync" | "language" | "guide";
    onClose: () => void;
    onSyncCompleted?: () => void;
  } = $props();

  let activeTab = $state<"account" | "audio" | "network" | "theme" | "sync" | "language" | "guide">("account");

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

  async function handleFileSelected(e: Event) {
    const input = e.target as HTMLInputElement;
    if (!input.files || input.files.length === 0) return;
    const file = input.files[0];
    input.value = "";
    profileError = null;

    const MAX_AVATAR_FILE_BYTES = 50 * 1024 * 1024;
    const MAX_AVATAR_SOURCE_DIM = 1280;

    if (file.size > MAX_AVATAR_FILE_BYTES) {
      profileError = "Image file is too large (max 50 MB)";
      return;
    }

    const objectUrl = URL.createObjectURL(file);
    try {
      const img = await new Promise<HTMLImageElement>((resolve, reject) => {
        const el = new Image();
        el.onload = () => resolve(el);
        el.onerror = () => reject(new Error("Failed to decode selected image"));
        el.src = objectUrl;
      });

      const w = img.naturalWidth;
      const h = img.naturalHeight;
      if (!w || !h) {
        throw new Error("Invalid image dimensions");
      }

      const maxDim = Math.max(w, h);
      const scale = maxDim > MAX_AVATAR_SOURCE_DIM ? MAX_AVATAR_SOURCE_DIM / maxDim : 1;
      const targetW = Math.max(1, Math.round(w * scale));
      const targetH = Math.max(1, Math.round(h * scale));

      const canvas = document.createElement("canvas");
      canvas.width = targetW;
      canvas.height = targetH;
      const ctx = canvas.getContext("2d");
      if (!ctx) {
        throw new Error("Canvas 2D context unavailable");
      }
      ctx.drawImage(img, 0, 0, targetW, targetH);
      cropperImageSrc = canvas.toDataURL("image/png");
    } catch (err: any) {
      console.error("[VoxP2P] Avatar image processing failed:", err);
      profileError = err?.message || "Failed to process selected image";
    } finally {
      URL.revokeObjectURL(objectUrl);
    }
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

  async function loadPersistedSettings() {
    try {
      const settings = await api.getAppSettings();
      if (settings["bypass_vpn"] !== undefined) {
        vpnBypassEnabled = settings["bypass_vpn"] === "true";
      }
      if (settings["rnnoise_enabled"] !== undefined) {
        rnnoiseEnabled = settings["rnnoise_enabled"] === "true";
      }
      if (settings["vad_threshold"] !== undefined) {
        const parsed = parseFloat(settings["vad_threshold"]);
        if (!isNaN(parsed)) vadThreshold = parsed;
      }
      if (settings["input_device"]) {
        selectedInput = settings["input_device"];
      }
      if (settings["output_device"]) {
        selectedOutput = settings["output_device"];
      }
    } catch (e) {
      console.error("Failed to load persisted settings:", e);
    }
  }

  let unlistenSyncHost: (() => void) | undefined;

  onMount(() => {
    loadProfile();

    loadPersistedSettings().finally(() => {
      // Fetch available audio devices
      api.listAudioDevices().then((res) => {
        inputDevices = res.input_devices || [];
        outputDevices = res.output_devices || [];

        if (selectedInput === "default" || !inputDevices.some((d) => d.name === selectedInput)) {
          const defIn = inputDevices.find((d) => d.is_default);
          if (defIn) selectedInput = defIn.name;
        }

        if (selectedOutput === "default" || !outputDevices.some((d) => d.name === selectedOutput)) {
          const defOut = outputDevices.find((d) => d.is_default);
          if (defOut) selectedOutput = defOut.name;
        }
      });
    });

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
  });

  onDestroy(() => {
    unlistenSyncHost?.();
    if (countdownTimer) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
    stopCamera();
    if (isTestingMic) {
      api.setMicTest(false).catch(console.error);
    }
    api.setMeterActive(false).catch(console.error);
  });

  // Dedicated effect for audio meter in the audio tab
  $effect(() => {
    let unlistenLocalLevel: (() => void) | undefined;

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
        isTestingMic = false;
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
      const initialSecs = payload.expires_in_secs || 120;
      qrCountdown = initialSecs;
      const targetTime = Date.now() + initialSecs * 1000;

      countdownTimer = setInterval(() => {
        const remaining = Math.max(0, Math.ceil((targetTime - Date.now()) / 1000));
        qrCountdown = remaining;
        if (remaining <= 0) {
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
      let scannerAvailable = true;
      try {
        let perm = await checkPermissions();
        if (perm !== "granted") {
          perm = await requestPermissions();
        }
        if (perm !== "granted") {
          qrImportError = "Доступ к камере отклонен. Разрешите доступ к камере в системных настройках устройства для сканирования QR-кода.";
          return;
        }
      } catch (permErr) {
        const msg = String(permErr || "");
        if (/not implemented|not supported|unknown command|not found/i.test(msg)) {
          scannerAvailable = false;
        } else {
          console.warn("Could not check/request barcode scanner permissions:", permErr);
        }
      }

      if (scannerAvailable) {
        const scanned = await scan({
          formats: [Format.QRCode],
          cameraDirection: "back",
          windowed: false,
        });
        if (scanned && scanned.content) {
          qrImportInput = scanned.content;
          await handleImportQr(scanned.content);
        }
        // If scanned is empty/null, the user cancelled or closed the native scanner.
        return;
      }
    } catch (err: any) {
      const msg = String(err?.message || err || "");
      if (/cancel|dismiss|abort/i.test(msg)) {
        // User cancelled the native barcode scanner — exit cleanly without fallback or error.
        return;
      }
      console.warn("Tauri barcode scanner unavailable or failed, falling back to camera stream:", err);
    }

    // Graceful Web/HTML5 camera stream fallback
    const BarcodeDetectorClass = (window as any).BarcodeDetector;
    if (typeof BarcodeDetectorClass !== "function") {
      qrImportError =
        "Сканирование QR-кода через камеру не поддерживается в этом браузере/WebView (отсутствует BarcodeDetector). Пожалуйста, вставьте sync-тикет вручную ниже.";
      return;
    }

    if (typeof BarcodeDetectorClass.getSupportedFormats === "function") {
      try {
        const supported: string[] = await BarcodeDetectorClass.getSupportedFormats();
        if (Array.isArray(supported) && !supported.includes("qr_code")) {
          qrImportError =
            "Встроенный BarcodeDetector не поддерживает формат QR-кодов. Пожалуйста, вставьте sync-тикет вручную ниже.";
          return;
        }
      } catch {
        // Proceed and let constructor validate
      }
    }

    try {
      if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) {
        isScanningCamera = true;
        cameraStream = await navigator.mediaDevices.getUserMedia({
          video: { facingMode: "environment" },
        });
        setTimeout(async () => {
          if (!cameraVideoEl || !cameraStream) {
            stopCamera();
            return;
          }
          try {
            cameraVideoEl.srcObject = cameraStream;
            await cameraVideoEl.play();
            startVideoQrDetector();
          } catch (playErr: any) {
            stopCamera();
            qrImportError =
              "Не удалось запустить видеопоток камеры: " + (playErr?.message || playErr);
          }
        }, 50);
      } else {
        qrImportError = "Camera not supported on this platform. Please paste the sync ticket manually below.";
      }
    } catch (camErr: any) {
      stopCamera();
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
    if (typeof BarcodeDetectorClass !== "function") {
      stopCamera();
      qrImportError =
        "Сканирование QR-кода через камеру не поддерживается (отсутствует BarcodeDetector). Пожалуйста, вставьте sync-тикет вручную ниже.";
      return;
    }

    let detector: any;
    try {
      detector = new BarcodeDetectorClass({ formats: ["qr_code"] });
    } catch (err: any) {
      stopCamera();
      qrImportError =
        "Не удалось инициализировать BarcodeDetector: " + (err?.message || err);
      return;
    }

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
    const nextState = !isTestingMic;
    isTestingMic = nextState;
    try {
      if (nextState) {
        const granted = await api.requestMicPermission();
        if (!granted) {
          isTestingMic = false;
          return;
        }
      }
      await api.setMicTest(nextState);
    } catch (err) {
      console.error("Failed to toggle mic test:", err);
      isTestingMic = false;
    }
  }

  async function handleVadThresholdChange(threshold: number) {
    vadThreshold = threshold;
    try {
      await api.setVadThreshold(threshold);
      await api.setAppSetting("vad_threshold", threshold.toString());
    } catch (err) {
      console.error("Failed to set VAD threshold:", err);
    }
  }

  async function handleInputDeviceChange(name: string) {
    selectedInput = name;
    await api.setInputDevice(name);
    await api.setAppSetting("input_device", name);
  }

  async function handleOutputDeviceChange(name: string) {
    selectedOutput = name;
    await api.setOutputDevice(name);
    await api.setAppSetting("output_device", name);
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
    await api.setAppSetting("rnnoise_enabled", rnnoiseEnabled ? "true" : "false");
  }

  async function handleToggleVpnBypass() {
    vpnBypassEnabled = !vpnBypassEnabled;
    await api.setBypassVpn(vpnBypassEnabled);
    await api.setAppSetting("bypass_vpn", vpnBypassEnabled ? "true" : "false");
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

  function getActiveTabTitle(tab: typeof activeTab): string {
    switch (tab) {
      case "account": return t("settings.tabAccount");
      case "audio": return t("settings.tabAudio");
      case "network": return t("settings.tabNetwork");
      case "theme": return t("settings.tabTheme");
      case "sync": return t("settings.tabSync");
      case "language": return t("settings.tabLanguage");
      case "guide": return t("settings.tabGuide");
      default: return t("settings.title");
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
    <!-- Left Navigation Column -->
    <aside class="settings-nav">
      <div class="settings-nav-header">
        <span class="settings-nav-title">{t("settings.title")}</span>
      </div>
      <div class="nav-tabs-list" role="tablist">
        <button
          class="nav-tab-btn"
          class:active={activeTab === "account"}
          onclick={() => (activeTab = "account")}
          role="tab"
          aria-selected={activeTab === "account"}
        >
          {t("settings.tabAccount")}
        </button>
        <button
          class="nav-tab-btn"
          class:active={activeTab === "audio"}
          onclick={() => (activeTab = "audio")}
          role="tab"
          aria-selected={activeTab === "audio"}
        >
          {t("settings.tabAudio")}
        </button>
        <button
          class="nav-tab-btn"
          class:active={activeTab === "network"}
          onclick={() => (activeTab = "network")}
          role="tab"
          aria-selected={activeTab === "network"}
        >
          {t("settings.tabNetwork")}
        </button>
        <button
          class="nav-tab-btn"
          class:active={activeTab === "theme"}
          onclick={() => (activeTab = "theme")}
          role="tab"
          aria-selected={activeTab === "theme"}
        >
          {t("settings.tabTheme")}
        </button>
        <button
          class="nav-tab-btn"
          class:active={activeTab === "sync"}
          onclick={() => (activeTab = "sync")}
          role="tab"
          aria-selected={activeTab === "sync"}
        >
          {t("settings.tabSync")}
        </button>
        <button
          class="nav-tab-btn"
          class:active={activeTab === "language"}
          onclick={() => (activeTab = "language")}
          role="tab"
          aria-selected={activeTab === "language"}
        >
          {t("settings.tabLanguage")}
        </button>
        <button
          class="nav-tab-btn"
          class:active={activeTab === "guide"}
          onclick={() => (activeTab = "guide")}
          role="tab"
          aria-selected={activeTab === "guide"}
        >
          {t("settings.tabGuide")}
        </button>
      </div>
    </aside>

    <!-- Right Main Content Column -->
    <main class="settings-main">
      <div class="settings-main-header">
        <h3 class="active-tab-title">{getActiveTabTitle(activeTab)}</h3>
        <button
          class="dialog-close-btn"
          onclick={handleModalClose}
          aria-label={t("common.close") || "Close"}
          title={t("common.close") || "Close"}
        >
          &times;
        </button>
      </div>

      <div class="settings-content-body">
      {#if activeTab === "account"}
        <div class="settings-group">
          <h4>{t("settings.accountTitle")}</h4>
          <p class="section-desc">
            {t("settings.accountDesc")}
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
              title={t("settings.changeAvatar")}
            >
              <div class="avatar-large-preview">
                <Avatar
                  src={avatarDataUrl}
                  hash={profile?.avatar_hash}
                  nickname={profile?.nickname}
                  size={96}
                />
              </div>
              <!-- Hover Overlay (Change Avatar) -->
              <div class="avatar-hover-overlay">
                <span class="overlay-icon">📷</span>
                <span class="overlay-text">{t("settings.changeAvatar")}</span>
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
              <div class="avatar-title">{profile?.nickname || t("common.peer")}</div>
              <div class="avatar-hint">
                Supports PNG, JPG, WEBP, or GIF. Click to crop (Steam/Discord style).
              </div>
              <div class="avatar-btn-row">
                <button class="btn-secondary-action" onclick={handleAvatarClick}>
                  📷 {t("settings.changeAvatar")}
                </button>
                {#if avatarDataUrl || profile?.avatar_hash}
                  <button class="btn-danger-action" onclick={handleRemoveAvatar}>
                    🗑️ {t("settings.removeAvatar")}
                  </button>
                {/if}
              </div>
            </div>
          </div>

          <!-- Nickname Editing -->
          <div class="setting-item">
            <label for="profile-nickname-input">{t("settings.nicknameLabel")}</label>
            <div class="nickname-input-row">
              <input
                id="profile-nickname-input"
                type="text"
                bind:value={nicknameInput}
                placeholder={t("settings.nicknamePlaceholder")}
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
                  {t("common.loading")}
                {:else if profileSaveSuccess}
                  ✓ {t("settings.profileSaved")}
                {:else}
                  {t("settings.saveProfile")}
                {/if}
              </button>
            </div>
            <span class="item-desc">
              {t("settings.nicknameLabel")}
            </span>
          </div>

          <!-- Account ID -->
          <div class="setting-item">
            <label for="profile-node-id-val">{t("settings.nodeIdLabel")}</label>
            <div class="node-id-badge-card">
              <div class="node-id-key-icon">🔑</div>
              <div class="node-id-text-wrap">
                <code class="node-id-code" id="profile-node-id-val">{profile?.node_id || t("common.loading")}</code>
                <span class="node-id-caption">{t("settings.nodeIdDesc")}</span>
              </div>
              <button
                class="btn-copy-id"
                onclick={handleCopyNodeId}
                title={t("settings.copyNodeId")}
              >
                {#if copyNodeIdFeedback}
                  ✓ {t("common.copied")}
                {:else}
                  📋 {t("settings.copyNodeId")}
                {/if}
              </button>
            </div>
          </div>
        </div>
      {:else if activeTab === "audio"}
        <div class="settings-group">
          <h4>{t("settings.audioTitle")}</h4>

          <div class="setting-item">
            <label for="setting-input-dev">{t("settings.inputDevice")}</label>
            <select
              id="setting-input-dev"
              class="settings-select"
              value={selectedInput}
              onchange={(e) => handleInputDeviceChange((e.target as HTMLSelectElement).value)}
            >
              <option value="default">{t("settings.inputDevice")}</option>
              {#each inputDevices as dev}
                <option value={dev.name}>{dev.name} {dev.is_default ? "(Default)" : ""}</option>
              {/each}
            </select>
          </div>

          <div class="setting-item">
            <label for="setting-output-dev">{t("settings.outputDevice")}</label>
            <select
              id="setting-output-dev"
              class="settings-select"
              value={selectedOutput}
              onchange={(e) => handleOutputDeviceChange((e.target as HTMLSelectElement).value)}
            >
              <option value="default">{t("settings.outputDevice")}</option>
              {#each outputDevices as dev}
                <option value={dev.name}>{dev.name} {dev.is_default ? "(Default)" : ""}</option>
              {/each}
            </select>
          </div>
        </div>

        <div class="settings-group">
          <h4>{t("settings.audioTitle")}</h4>

          <div class="setting-item">
            <div class="setting-label-row">
              <label for="setting-mic-gain">{t("settings.micGain", { val: Math.round(micGain * 100) })}</label>
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
              <label for="setting-master-vol">{t("settings.masterVolume", { val: Math.round(masterVolume * 100) })}</label>
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
          <h4>{t("settings.rnnoiseTitle")}</h4>

          <div class="toggle-setting-row">
            <div>
              <div class="toggle-title">{t("settings.rnnoiseTitle")}</div>
              <div class="toggle-desc">{t("settings.rnnoiseDesc")}</div>
            </div>
            <button
              class="switch-btn"
              class:checked={rnnoiseEnabled}
              onclick={handleToggleRnnoise}
              aria-label={t("settings.rnnoiseTitle")}
            >
              <span class="switch-handle"></span>
            </button>
          </div>
        </div>

        <div class="settings-group">
          <div class="setting-header-with-action">
            <div>
              <h4>{t("settings.micTest")}</h4>
              <div class="toggle-desc">{t("settings.micTestDesc")}</div>
            </div>
            <button
              class="mic-test-btn"
              class:active={isTestingMic}
              onclick={handleToggleMicTest}
            >
              {#if isTestingMic}
                <span class="pulse-dot"></span> {t("settings.stopTest")}
              {:else}
                🎤 {t("settings.startTest")}
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
                title="{t('settings.vadThreshold', { val: Math.round(vadThreshold * 100) })}"
              >
                <div class="vad-notch-caret"></div>
                <div class="vad-notch-line"></div>
              </div>
            </div>

            <div class="threshold-control-row">
              <div class="threshold-label-wrap">
                <label for="vad-threshold-slider">{t("settings.vadThreshold", { val: Math.round(vadThreshold * 100) })}</label>
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
                {t("settings.vadDesc")}
              </div>
            </div>
          </div>
        </div>

      {:else if activeTab === "network"}
        <div class="settings-group">
          <h4>{t("settings.networkTitle")}</h4>

          <div class="toggle-setting-row">
            <div>
              <div class="toggle-title">{t("settings.vpnBypassTitle")}</div>
              <div class="toggle-desc">
                {t("settings.vpnBypassDesc")}
              </div>
            </div>
            <button
              class="switch-btn"
              class:checked={vpnBypassEnabled}
              onclick={handleToggleVpnBypass}
              aria-label={t("settings.vpnBypassTitle")}
            >
              <span class="switch-handle"></span>
            </button>
          </div>
        </div>

        <div class="settings-group">
          <h4>{t("settings.p2pStatusTitle")}</h4>
          <p class="section-desc">{t("settings.p2pStatusDesc")}</p>
          <div class="info-card">
            <div class="info-item">
              <strong>{t("settings.natType")}</strong>
            </div>
            <div class="info-item">
              <strong>{t("settings.cryptoEngine")}</strong>
            </div>
            <div class="info-item">
              <strong>{t("settings.voiceEngine")}</strong>
            </div>
          </div>
        </div>

      {:else if activeTab === "theme"}
        <ThemeStudio bind:currentTheme {onClose} />
      {:else if activeTab === "sync"}
        <!-- QR CODE HOST & CLIENT SYNC -->
        <div class="settings-group">
          <h4>{t("settings.syncTitle")}</h4>
          <p class="section-desc">
            {t("settings.syncDesc")}
          </p>

          <div class="sync-grid-container">
            <!-- Left Card: Host QR Generator -->
            <div class="sync-card">
              <div class="sync-card-header">
                <h5>{t("settings.qrHostTitle")}</h5>
                <span class="sync-role-badge">Export Identity</span>
              </div>
              <p class="card-desc">
                {t("settings.qrHostDesc")}
              </p>

              {#if !qrHostPayload}
                <button
                  class="btn-primary"
                  onclick={handleStartQrHost}
                  disabled={qrHostLoading}
                >
                  {qrHostLoading ? t("common.loading") : "⚡ " + t("settings.generateQr")}
                </button>
              {:else}
                <div class="qr-presentation">
                  <div class="qr-code-svg-wrap">
                    {@html qrHostPayload.qr_svg}
                  </div>

                  {#if qrCountdown > 0}
                    <div class="qr-status-indicator">
                      <span class="pulse-dot-green"></span>
                      <span>Active &bull; {t("settings.qrExpiresIn", { seconds: qrCountdown })}</span>
                    </div>
                  {:else}
                    <div class="qr-status-expired">
                      ⚠️ Session Expired (120s limit). Click below to generate a new QR code.
                    </div>
                  {/if}

                  <div class="qr-ticket-actions">
                    <button class="btn-secondary qr-copy-btn" onclick={copyQrTicket}>
                      {copyQrFeedback ? "✓ " + t("common.copied") : "📋 " + t("settings.copyTicketPayload")}
                    </button>
                    {#if qrCountdown === 0}
                      <button class="btn-primary" onclick={handleStartQrHost} disabled={qrHostLoading}>
                        🔄 {t("settings.generateQr")}
                      </button>
                    {:else}
                      <button class="btn-cancel-sync" onclick={handleStopQrHost}>
                        ✕ {t("common.cancel")}
                      </button>
                    {/if}
                  </div>
                </div>
              {/if}

              {#if qrHostSuccess}
                <div class="sync-success-box highlight">
                  ✓ {t("settings.qrHostSuccessTitle")} (<strong>{qrHostSuccess.nickname}</strong>)
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
                <h5>{t("settings.qrImportTitle")}</h5>
                <span class="sync-role-badge">Import Identity</span>
              </div>
              <p class="card-desc">
                {t("settings.qrImportDesc")}
              </p>

              <div class="scan-actions-group">
                <button
                  class="btn-primary scan-btn"
                  onclick={handleScanQrWithCamera}
                  disabled={qrImportLoading || isScanningCamera}
                >
                  {isScanningCamera ? t("settings.stopCamera") : t("settings.scanCamera")}
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
                  <label for="qr-ticket-paste" class="input-sublabel">{t("settings.pasteTicket")}:</label>
                  <div class="manual-ticket-row">
                    <input
                      id="qr-ticket-paste"
                      type="text"
                      class="ticket-input"
                      placeholder="p2psync://<ticket>/<token>"
                      bind:value={qrImportInput}
                    />
                    <button class="btn-secondary" onclick={handlePasteTicket} title={t("settings.pasteTicket")}>
                      📋 {t("chat.paste")}
                    </button>
                  </div>
                  <button
                    class="btn-primary full-width"
                    onclick={() => handleImportQr()}
                    disabled={qrImportLoading || !qrImportInput.trim()}
                  >
                    {qrImportLoading ? t("common.loading") : t("settings.importSync")}
                  </button>
                </div>

                {#if qrImportResult}
                  <div class="sync-success-box highlight">
                    ✓ {t("settings.syncSuccess", { contacts: qrImportResult.contacts_imported, messages: qrImportResult.messages_imported })}
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
              📦 {t("settings.legacySyncTitle")}
            </summary>
            <div class="advanced-sync-content">
              <p class="section-desc">
                {t("settings.legacySyncDesc")}
              </p>
              <button class="btn-secondary" onclick={handleGenerateSyncTicket} disabled={syncLoading}>
                {syncLoading ? t("common.loading") : t("settings.generateLegacyTicket")}
              </button>
              {#if syncTicket}
                <div class="sync-ticket-box">
                  <div class="sync-stats">
                    <span>✓ Exported {syncTicket.contacts_count} contacts & {syncTicket.messages_count} messages</span>
                  </div>
                  <div class="ticket-row">
                    <input type="text" class="ticket-input" readonly value={syncTicket.ticket} />
                    <button class="btn-secondary" onclick={copySyncTicket}>
                      {copyFeedback ? "✓ " + t("common.copied") : t("common.copy")}
                    </button>
                  </div>
                </div>
              {/if}

              <div class="import-blob-block">
                <label for="import-sync-ticket-input" class="input-sublabel">{t("settings.importLegacyTicket")}:</label>
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
                  {syncLoading ? t("common.loading") : t("settings.importLegacyTicket")}
                </button>
                {#if syncResult}
                  <div class="sync-success-box">
                    ✓ {t("settings.syncSuccess", { contacts: syncResult.contacts_imported, messages: syncResult.messages_imported })}
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

      {:else if activeTab === "language"}
        <div class="settings-group">
          <h4>{t("settings.languageTitle")}</h4>
          <p class="section-desc">
            {t("settings.languageDesc")}
          </p>

          <div class="language-selection-grid">
            {#each SUPPORTED_LOCALES as loc}
              {@const isActive = i18n.currentLocale === loc.code}
              <button
                class="language-card"
                class:active={isActive}
                onclick={() => setLocale(loc.code)}
                type="button"
              >
                <div class="lang-flag-badge">{loc.flag}</div>
                <div class="lang-info-col">
                  <span class="lang-native-name">{loc.name}</span>
                  <span class="lang-english-name">{loc.englishName}</span>
                </div>
                {#if isActive}
                  <span class="lang-active-check">✓</span>
                {/if}
              </button>
            {/each}
          </div>

          <div class="technical-notice-box">
            <span class="notice-icon">ℹ️</span>
            <p class="notice-text">
              {t("settings.technicalTermsNotice")}
            </p>
          </div>
        </div>

      {:else if activeTab === "guide"}
        <UserManual onNavigateTab={(tab) => activeTab = tab} />
      {/if}
      </div>
    </main>
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
    width: 1040px;
    max-width: 94vw;
    height: 760px;
    max-height: 88vh;
    display: flex;
    flex-direction: row;
    overflow: hidden;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.5);
  }

  .settings-nav {
    width: 250px;
    min-width: 250px;
    background: var(--bg-secondary);
    border-right: var(--border-width) solid var(--border-color);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    flex-shrink: 0;
  }

  .settings-nav-header {
    padding: 20px 20px 14px;
    border-bottom: var(--border-width) solid var(--border-color);
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }

  .settings-nav-title {
    font-size: 1.05rem;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: 0.02em;
  }

  .nav-tabs-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px;
    overflow-y: auto;
    flex: 1;
  }

  .nav-tab-btn {
    text-align: left;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    white-space: nowrap;
    padding: 10px 14px;
    font-size: 0.9rem;
    font-weight: 500;
    border-radius: var(--border-radius);
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
    box-sizing: border-box;
  }

  .nav-tab-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .nav-tab-btn.active {
    color: #ffffff;
    background: var(--accent-primary);
    font-weight: 600;
  }

  .settings-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-primary);
  }

  .settings-main-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 32px;
    border-bottom: var(--border-width) solid var(--border-color);
    background: var(--bg-primary);
    flex-shrink: 0;
  }

  .active-tab-title {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 700;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    gap: 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dialog-close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 1.5rem;
    cursor: pointer;
    line-height: 1;
    padding: 6px 10px;
    border-radius: var(--border-radius);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.15s ease, background 0.15s ease;
  }

  .dialog-close-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .settings-content-body {
    flex: 1;
    overflow-y: auto;
    padding: 24px 32px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  @media (max-width: 768px) {
    .settings-dialog {
      width: 100vw;
      max-width: 100vw;
      height: 100vh;
      max-height: 100vh;
      border-radius: 0;
      border: none;
      flex-direction: column;
    }

    .settings-nav {
      width: 100%;
      min-width: 0;
      border-right: none;
      border-bottom: var(--border-width) solid var(--border-color);
      flex-shrink: 0;
    }

    .settings-nav-header {
      padding: 10px 14px 6px;
      border-bottom: none;
    }

    .settings-nav-title {
      font-size: 0.95rem;
    }

    .nav-tabs-list {
      flex-direction: row;
      overflow-x: auto;
      padding: 4px 10px 8px;
      gap: 6px;
      -webkit-overflow-scrolling: touch;
    }

    .nav-tab-btn {
      width: auto;
      flex-shrink: 0;
      padding: 7px 12px;
      font-size: 0.82rem;
    }

    .settings-main-header {
      padding: 12px 16px;
    }

    .active-tab-title {
      font-size: 1rem;
    }

    .dialog-close-btn {
      min-width: 44px;
      min-height: 44px;
      display: flex;
      align-items: center;
      justify-content: center;
    }

    .settings-content-body {
      padding: 16px;
      gap: 16px;
    }
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
    width: 100%;
    box-sizing: border-box;
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 8px 12px;
    color: var(--text-primary);
    font-size: 0.85rem;
    text-overflow: ellipsis;
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
    min-height: 44px;
    font-size: 0.92rem;
    font-weight: 600;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    user-select: none;
    -webkit-user-select: none;
    -webkit-tap-highlight-color: transparent;
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

  /* Language Selection Tab */
  .language-selection-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 12px;
    margin: 16px 0;
  }

  .language-card {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
    min-height: 48px;
    background: var(--surface-card, var(--bg-secondary));
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    transition: all 0.15s ease;
    word-break: break-word;
    outline: none;
  }

  .language-card:hover {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
    transform: translateY(-1px);
  }

  .language-card.active {
    background: rgba(79, 70, 229, 0.14);
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 1px var(--accent-primary);
  }

  .lang-flag-badge {
    font-size: 1.8rem;
    line-height: 1;
    flex-shrink: 0;
  }

  .lang-info-col {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .lang-native-name {
    font-size: 0.95rem;
    font-weight: 700;
    color: var(--text-primary);
    word-break: break-word;
    overflow-wrap: anywhere;
  }

  .lang-english-name {
    font-size: 0.78rem;
    color: var(--text-muted);
  }

  .lang-active-check {
    font-size: 1.2rem;
    font-weight: 800;
    color: var(--accent-hover);
    flex-shrink: 0;
  }

  .technical-notice-box {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 14px 16px;
    background: var(--bg-secondary);
    border-left: 3px solid var(--accent-primary);
    border-radius: 4px;
    margin-top: 18px;
  }

  .technical-notice-box .notice-icon {
    font-size: 1.25rem;
    flex-shrink: 0;
  }

  .technical-notice-box .notice-text {
    margin: 0;
    font-size: 0.84rem;
    color: var(--text-muted);
    line-height: 1.45;
    word-break: break-word;
  }
</style>
