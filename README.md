# VoxP2P 🎙️

<p align="center">
  <img src="static/app-icon.png" alt="VoxP2P Logo" width="128" height="128" />
</p>

<p align="center">
  <b>Decentralized, 100% Serverless Peer-to-Peer Voice & Text Messenger</b><br>
  <i>True Serverless Mesh Network across Desktop and Mobile</i>
</p>

---

## 📌 About the Project

**VoxP2P** is an open-source, cross-platform private voice and text messaging application built on pure **Peer-to-Peer (P2P)** architecture.

Unlike centralized services (Discord, Telegram, WhatsApp), VoxP2P operates **without any central servers, corporate databases, or cloud accounts**. All voice streams, text messages, file transfers, and identity verifications happen directly between user devices over encrypted QUIC connections.

* **No Sign-Up or Registration:** No phone numbers, emails, passwords, or personal data required.
* **Cryptographic Identity:** Your address is a unique Ed25519 public key (`Node ID`).
* **Zero Telemetry & Absolute Privacy:** Direct end-to-end encryption ensures only you and your peers can access calls and chat history.
* **Cross-Platform:** Available for **Windows**, **Linux**, **macOS**, and **Android**.

---

## ✨ Key Features

* 🔊 **Ultra-Low Latency Voice Calls:**
  * Direct 1-on-1 Calls and multi-peer group Voice Rooms.
  * Low-latency Opus codec with adaptive bitrates and 20 ms audio frames.
  * Neural network background noise suppression and voice activity detection (`nnnoiseless`).
  * Per-peer volume adjustment and live speech activity ring indicators.

* 💬 **Encrypted Direct & Room Messaging:**
  * Instant, authenticated message delivery directly over QUIC.
  * **Store-and-Forward Outbox:** If your contact is offline, messages are safely queued locally and delivered automatically as soon as the peer reconnects.
  * Delivery acknowledgments, message editing, and deletion.

* 📁 **P2P File & Media Transfer:**
  * Transfer files of any type and size directly without cloud file size limitations.
  * Cryptographic integrity verification via BLAKE3 content-addressed blobs (`iroh-blobs`).
  * Automatic profile picture / avatar synchronization and caching.

* 📱 **Android & Mobile Experience:**
  * Responsive layout with sliding drawer sidebar and bottom navigation (Chats, Calls, Contacts, Settings).
  * Safe area insets support for display notches, camera cutouts, and gesture navigation bars.
  * Android Foreground Service with ongoing notification and CPU WakeLock to keep voice calls active in the background when the screen is locked.
  * Touch-optimized controls with long-press context menus and haptic feedback.

* 🔄 **Device Sync & QR Pairing:**
  * Pair and clone your identity, contacts, and message history between desktop and mobile devices via camera QR scan or sync ticket.
  * Secure binary sync protocol over local Wi-Fi or Internet.

* 🔒 **Local Security & Independence:**
  * Encrypted local SQLite database tied to the user profile key.
  * No single point of failure and zero vulnerability to centralized service outages or censorship.

* 🖥️ **Desktop Integrations:**
  * Background system tray integration with quick mute/unmute and disconnect shortcuts.
  * Single instance lock to prevent duplicate background processes.

---

## 🛠️ How It Works (Architecture)

VoxP2P combines modern peer-to-peer networking, high-performance audio processing, and reactive UI frameworks:

```
+-------------------------------------------------------------+
|              Presentation & User Interface (UI)             |
|                Svelte 5 (Runes) + TypeScript + Vite         |
+-------------------------------------------------------------+
                               ↕ IPC (Tauri v2)
+-------------------------------------------------------------+
|                   Backend Core (Rust 2024)                  |
|  +--------------------+  +-------------------------------+  |
|  |   Audio Pipeline   |  |        Iroh P2P Network       |  |
|  | CPAL + Opus + RNNoise| |  QUIC + NAT Hole Punch + DERP |  |
|  +--------------------+  +-------------------------------+  |
|  +--------------------+  +-------------------------------+  |
|  |  Storage (SQLite)  |  |    Content Storage (Blobs)    |  |
|  | Encrypted Database |  |    BLAKE3 Verified Storage    |  |
|  +--------------------+  +-------------------------------+  |
+-------------------------------------------------------------+
```

### 1. P2P Mesh Connectivity (Iroh & QUIC)
Each node generates a cryptographic Ed25519 keypair upon initial launch:
* Peers discover each other and punch through NAT/firewalls using STUN and UDP hole punching.
* If direct symmetric NAT connections cannot be established, traffic routes through encrypted DERP relays where the relay only sees encrypted packets with zero ability to inspect data.
* Full-mesh UDP datagrams are used for voice streams, ensuring minimal delay and instant packet delivery.

### 2. Audio Processing Pipeline
1. Audio input capture from microphone via `cpal` (AAudio/Oboe on Android; WASAPI on Windows; ALSA/Pulse on Linux; CoreAudio on macOS).
2. Real-time background noise filtering and echo cancellation using `nnnoiseless` (RNNoise neural network).
3. Frame-based encoding into Opus at 48 kHz using `audiopus` (20 ms chunks).
4. Direct network transmission to peers, jitter buffer reordering via lock-free ring buffer (`ringbuf`), volume scaling, and output mixing.

### 3. Background Services on Android
* While an active voice call or room session is in progress, VoxP2P binds a native Android `ForegroundService` with a persistent notification in the status bar.
* Acquires a `PARTIAL_WAKE_LOCK` to keep the CPU running without interruption when the screen is turned off.

---

## 💻 Tech Stack

* **Frontend:** [Svelte 5](https://svelte.dev/) (Runes reactivity), TypeScript, Vite.
* **App Framework:** [Tauri v2](https://v2.tauri.app/) (Desktop & Mobile).
* **Backend Core:** [Rust 2024](https://www.rust-lang.org/).
* **P2P Networking:** [Iroh](https://iroh.computer/) (`iroh`, `iroh-gossip`, `iroh-blobs`).
* **Audio Engine:** `cpal`, `audiopus`, `nnnoiseless`, `ringbuf`.
* **Database:** SQLite with encrypted storage (`rusqlite`).
* **Mobile / Android:** Android NDK (r26b / 26.1), Kotlin Foreground Service, `@tauri-apps/plugin-barcode-scanner`.

---

## 🚀 Building and Development

### Prerequisites

1. **Node.js** (v18 or v20+) and `npm`.
2. **Rust** stable toolchain (`rustup`).
3. **Platform Build Tools:**
   * **Windows:** Visual Studio C++ Build Tools.
   * **Linux:** `sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libasound2-dev cmake build-essential`.
   * **macOS:** Xcode Command Line Tools.
   * **Android:** Android SDK (API 24+), Android NDK (26.1+), JDK 17.

### Step-by-Step Setup

1. **Clone the repository:**
   ```bash
   git clone https://github.com/your-username/VoxP2P.git
   cd VoxP2P
   ```

2. **Install frontend dependencies:**
   ```bash
   npm install
   ```

3. **Run in development mode (Desktop):**
   ```bash
   npm run tauri dev
   ```

4. **Build release desktop binary / installer:**
   ```bash
   npm run tauri build
   ```
   Installers and executables will be output to `src-tauri/target/release/bundle/`.

5. **Build Android APK (Mobile):**
   Ensure Android targets are installed:
   ```bash
   rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
   ```
   Build universal or multi-target APK:
   ```bash
   npm run tauri android build -- --apk --target aarch64 armv7 x86_64
   ```
   *(Optional: append `--split-per-abi` to generate separate slim APKs for each architecture)*.

   The resulting APK files will be located at:
   `src-tauri/gen/android/app/build/outputs/apk/`

---

## 🤖 Continuous Integration & Automated Builds

VoxP2P features automated cloud CI/CD workflows powered by **GitHub Actions**:
* `.github/workflows/release.yml`: Automated cross-platform desktop builds (Windows, Linux, macOS) upon tag releases (`v*`).
* `.github/workflows/android.yml`: Automated multi-ABI Android APK generation (`aarch64`, `armv7`, `x86_64`) for both commit artifacts and tagged GitHub Releases.

---

## ⚖️ Disclaimer

This software is provided **"AS IS"**, strictly for research, educational, and privacy-evaluation purposes, without warranties of any kind, either express or implied, including but not limited to the implied warranties of merchantability, fitness for a particular purpose, and non-infringement.

The authors and contributors assume no liability for any direct, indirect, incidental, special, exemplary, or consequential damages (including loss of data, communications interruptions, or operational issues) arising from the use or inability to use this software. Users are solely responsible for ensuring compliance with applicable local telecommunication and privacy laws.
