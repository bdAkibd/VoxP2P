# Architecture Specification

## Framework
- **Tauri v2** – supports Desktop (Windows, Linux, macOS) and Mobile (Android, iOS).

## Frontend
- **Svelte 5** + **TypeScript** + **Vite**
- **CSS Custom Properties** for dynamic theming.

## Backend (Rust Core)
- **Networking (100% Serverless)**: `iroh` (Ed25519 `NodeId` identity, Mainline DHT & Pkarr discovery, QUIC NAT hole‑punching, public relay fallback), `iroh-gossip` (room state & text broadcast), `iroh-blobs` (chunked BLAKE3 file/media/avatar transfers).
- **VPN Bypass**: `socket2` to bind UDP sockets directly to the physical network interface (`IP_UNICAST_IF` on Windows, `SO_BINDTODEVICE` on Linux, `IP_BOUND_IF` on macOS) to avoid virtual TUN/TAP VPN conflicts, plus `mdns-sd` for LAN discovery.
- **Audio Pipeline**: `cpal` (cross‑platform audio I/O), `nnnoiseless` (RNNoise noise suppression), `audiopus` (Opus 24‑64 kbps encoding/decoding), custom lock‑free ring buffer (`ringbuf`) for jitter buffer and PCM audio mixing. Audio never passes through Tauri IPC — only lightweight volume‑level arrays (every 50 ms) and control commands cross IPC.
- **Call Topology & Host Migration**: Full‑mesh P2P audio via QUIC Unreliable Datagrams. If two peers have strict Symmetric NAT, the current Room Host acts as a lightweight UDP packet relay between them. Deterministic Host Migration: peers are ordered by `(join_timestamp, node_id)`. If the Host heartbeat times out (2000 ms), all peers deterministically promote the next peer in order. Manual host transfer via signed `TransferHost { target_node_id }` gossip message.
- **Storage**: `rusqlite` (with `bundled` feature so SQLite compiles on Windows/Android without system libs) for contacts, offline message queue, and chat history. Local `themes.json` for custom UI palettes.

## Android & Mobile Build Configuration

### Prerequisites
- **Android Studio** & **Android SDK** (API Level 24+ / Android 7.0+ minimum, API Level 34 recommended)
- **Android NDK** (version 26+ or r25c+)
- **Java Development Kit (JDK 17+)**
- Set environment variables:
  ```bash
  export ANDROID_HOME="$HOME/Android/Sdk" # Linux/macOS
  export NDK_HOME="$ANDROID_HOME/ndk/<ndk_version>"
  # Windows PowerShell:
  # $env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
  # $env:NDK_HOME = "$env:ANDROID_HOME\ndk\<ndk_version>"
  ```
- Install Rust target architectures:
  ```bash
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
  ```

### Initialization & Build Commands
1. **Initialize Android Project**:
   ```bash
   npx tauri android init
   ```
   Generates the Gradle wrapper and native Android container in `src-tauri/gen/android`.

2. **Run in Development / Debug on Device or Emulator**:
   ```bash
   npx tauri android dev
   ```

3. **Build Release APKs and Android App Bundles (AAB)**:
   ```bash
   npx tauri android build
   ```
   To build specifically for 64-bit ARM devices (most modern phones):
   ```bash
   npx tauri android build --target aarch64-linux-android
   ```
   The built APK is saved to:
   `src-tauri/gen/android/app/build/outputs/apk/release/app-release-unsigned.apk`

### Android Permissions & Mobile Capabilities
Configured in `src-tauri/gen/android/app/src/main/AndroidManifest.xml` and `src-tauri/capabilities/mobile.json`:
- **`RECORD_AUDIO` & `MODIFY_AUDIO_SETTINGS`**: Enables low-latency Opus microphone capture via `cpal` (AAudio / OpenSL ES backend via `ndk`).
- **`INTERNET` & `ACCESS_NETWORK_STATE` / `ACCESS_WIFI_STATE`**: Serverless P2P QUIC hole-punching, gossip broadcast, and blob synchronization.
- **`CAMERA`**: End-to-end QR code account and database cloning via `@tauri-apps/plugin-barcode-scanner` (with fallback to camera stream).
- **Conditional Compilation**: In `src-tauri/src/network/vpn_bypass.rs`, `#[cfg(target_os = "android")]` cleanly bypasses raw desktop socket binding and setsockopt to prevent permission crashes in the Android sandbox.
