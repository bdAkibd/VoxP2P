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
