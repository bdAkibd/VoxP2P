# Architecture Specification

## Framework
- **Tauri v2** – supports Desktop (Windows, Linux, macOS) and Mobile (Android, iOS).

## Frontend
- **Svelte 5** + **TypeScript** + **Vite**
- **CSS Custom Properties** for dynamic theming.

## Backend (Rust Core)
- **Networking (100% Serverless)**: `iroh` (Ed25519 `NodeId` identity, Mainline DHT & Pkarr discovery, QUIC NAT hole‑punching, public relay fallback), `iroh-gossip` (room state & text broadcast), `iroh-blobs` (chunked BLAKE3 file/media/avatar transfers).
- **VPN Bypass**: `vpn_bypass.rs` provides `socket2` / `netdev` physical interface detection and socket-option helpers (`IP_UNICAST_IF` on Windows, `SO_BINDTODEVICE` on Linux, `IP_BOUND_IF` on macOS). However, `iroh 1.4.1` manages its own internal `IpTransport` (`netwatch::UdpSocket`) and does not expose a stable public API to inject a pre-bound `std::net::UdpSocket` / `socket2::Socket` (`Endpoint::builder(...).add_custom_transport(...)` is gated behind `unstable-custom-transports` and only handles `CustomAddr`). Accordingly, `bypass_vpn` is not wired into `iroh`'s runtime UDP socket and is not simulated with stub calls. LAN discovery uses `mdns-sd` (`MdnsAddressLookup`).
- **Audio Pipeline**: `cpal` (cross‑platform audio I/O), `nnnoiseless` (RNNoise noise suppression), `audiopus` (Opus 24‑64 kbps encoding/decoding), custom lock‑free ring buffer (`ringbuf`) for jitter buffer and PCM audio mixing. Audio never passes through Tauri IPC — only lightweight volume‑level arrays (every 50 ms) and control commands cross IPC.
- **Call Topology & Host Migration**: Full‑mesh P2P audio via QUIC Unreliable Datagrams. If two peers have strict Symmetric NAT, the current Room Host acts as a lightweight UDP packet relay between them. Deterministic Host Migration: peers are ordered by `(join_timestamp, node_id)`. If the Host heartbeat times out (2000 ms), all peers deterministically promote the next peer in order. Manual host transfer via signed `TransferHost { target_node_id }` gossip message.
- **Storage**: `rusqlite` (with `bundled` feature so SQLite compiles on Windows/Android without system libs) for contacts, offline message queue, and chat history. Local `themes.json` for custom UI palettes.

## P2P Connectivity, DM Outbox & Synchronization Contracts
To avoid connection failures, race conditions, and message loss across desktop and mobile nodes, all networking and storage changes must adhere to the following invariants:

1. **Identity & Peer Authentication**:
   - Remote peer identity on incoming QUIC connections (`DM_ALPN`, `AUDIO_ALPN`, `SYNC_ALPN`) MUST be derived from and verified against the TLS-authenticated `connection.remote_id()` rather than trusting self-reported JSON fields (`node_id`, `sender_id`, `caller_id`).
   - On `MessageEdited` and `MessageDeleted`, verify that the remote peer matches the original `sender_id` of the target message in SQLite before mutating local state.

2. **Message `chat_id` & Storage Invariants (`src-tauri/src/storage/db.rs`)**:
   - **Outgoing DM**: `chat_id = recipient_node_id`, `sender_id = local_node_id`, `delivered = 0`.
   - **Incoming DM** (`DirectMessageHandler`): MUST normalize `message.chat_id = peer_id` (the sender's authenticated `NodeId`) and `message.sender_id = peer_id` before inserting into SQLite and emitting UI events, so sender and receiver both index the 1-on-1 conversation under the peer's `NodeId`.
   - **Room chat**: `chat_id = room_id`.
   - Message IDs must be collision-free even under sub-millisecond burst sends (monotonic counter or random suffix in addition to timestamp).
   - `get_messages_for_chat(chat_id, limit)` must return the **latest** `limit` messages ordered chronologically (`ASC`) rather than the oldest `limit` messages.
   - Avatar paths must be consistent across modules (`<app_data_dir>/avatars/<hash>.png` in both `file_transfer.rs` and `device_sync.rs`).

3. **DM Connection Lifecycle & Outbox Worker (`src-tauri/src/network/file_transfer.rs`)**:
   - **Startup order (`src-tauri/src/lib.rs`)**: Register `DirectMessageHandler` and `AudioProtocolHandler` on `NodeManager` *before* `NodeManager::start()` spawns the `Router`, and initialize `FileTransferManager` with endpoint, store, and real `local_node_id` *before* starting the outbox watcher.
   - **No dropped notifications**: Outbox wakeups must use a permit-storing mechanism (`Notify::notify_one()` or per-peer command channel) instead of `notify_waiters()`, so notifications arriving mid-flush are never lost.
   - **No Head-of-Line blocking & per-peer serialization**: `sync_outbox` must fan out across contacts concurrently while guarding each `recipient_node_id` with a per-peer lock/worker so two flushes never race on `peer_backoff`. Explicitly queued user messages or incoming connections from a peer must cancel that peer's active backoff immediately.
   - **Connection timeouts & pooling**: Cold `endpoint.connect` over `presets::N0` (Pkarr + Relay + QUIC handshake) requires at least `10s–12s` timeout (not `4s`). Active `DM_ALPN` connections should be cached/reused per `NodeId` and incoming bi-streams handled concurrently (`tokio::spawn` per stream), avoiding repeated `ProfileHandshake` round-trips per message.
   - **Reliable one-shot control frames**: `send_dm_payload` (`DirectCallRing`, `MessageEdited`, `MessageDeleted`) must wait for an acknowledgement or stream completion (`send.stopped().await` / response) before dropping the `Connection`.

4. **Peer Discovery & Transport (`src-tauri/src/network/node.rs`)**:
   - `NodeManager::start` should monitor `endpoint.online()` asynchronously (without blocking app startup) and emit node/relay connectivity status to the UI.
   - Populate `MemoryLookup` whenever peer `EndpointAddr`s are learned (tickets, sync, live connections) and add LAN mDNS discovery (`mdns-sd` or `iroh` address lookup) so peers on the same network connect without relying on external `dns.iroh.link` / public relays.
   - On Android, initialize `iroh::dns::install_android_jni_context` before binding the `Endpoint`.
   - Either wire `vpn_bypass` into custom transport/socket configuration or clearly reflect its actual scope in settings; avoid dead stub calls. (In `iroh 1.4.1`, `IpTransport` is internal and `CustomTransport` requires `unstable-custom-transports` and only supports `CustomAddr`, so `vpn_bypass` is not wired into `iroh`'s UDP socket in runtime.)

5. **Room / Lobby & UI Consistency (`src-tauri/src/network/room.rs`, `src/routes/+page.svelte`)**:
   - Never route `"lobby"` (or non-`PublicKey` chat IDs) into the DM outbox when no room session is active.
   - Room `attach_file` must broadcast via `room.broadcast_gossip(...)`, not only the internal `broadcast_tx`.
   - In `RoomSession`, re-announce `PeerJoined` on `iroh_gossip::api::Event::NeighborUp` and defer host-timeout migration until gossip mesh connectivity is established.
   - Deduplicate backend-to-frontend chat events (`chat://message-received` vs `voxp2p://chat-message`) and expose real node/peer connectivity state in the UI.

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
- **`FOREGROUND_SERVICE` & `FOREGROUND_SERVICE_MICROPHONE` / `WAKE_LOCK`**: Keeps background voice calls alive with a persistent notification in the Android shade and prevents CPU sleep during calls.
- **`INTERNET` & `ACCESS_NETWORK_STATE` / `ACCESS_WIFI_STATE` & `CHANGE_WIFI_MULTICAST_STATE`**: Serverless P2P QUIC hole-punching, gossip broadcast, blob synchronization, and local Wi-Fi mDNS peer discovery.
- **`CAMERA`**: End-to-end QR code account and database cloning via `@tauri-apps/plugin-barcode-scanner` (with fallback to camera stream).
- **Conditional Compilation**: In `src-tauri/src/network/vpn_bypass.rs`, `#[cfg(target_os = "android")]` cleanly bypasses raw desktop socket binding and setsockopt to prevent permission crashes in the Android sandbox.
- **CI/CD Automation**: `.github/workflows/android.yml` compiles APK packages on `ubuntu-latest` with JDK 17, NDK 26.1, Rust targets (`aarch64`, `armv7`, `x86_64`), uploads artifacts and publishes GitHub Releases on `v*` tags.
