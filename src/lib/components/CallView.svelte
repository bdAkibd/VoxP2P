<script lang="ts">
  import type { RoomState, PeerInfo, UserProfile } from "../types";
  import { api } from "../api";

  let {
    roomState,
    profile,
    audioLevels = {},
  }: {
    roomState: RoomState | null;
    profile: UserProfile | null;
    audioLevels: Record<string, number>;
  } = $props();

  let contextMenuPeer = $state<PeerInfo | null>(null);
  let contextMenuPos = $state<{ x: number; y: number }>({ x: 0, y: 0 });
  let peerVolumeMap = $state<Record<string, number>>({});

  function isSpeaking(nodeId: string): boolean {
    const level = audioLevels[nodeId] || 0;
    return level > 0.08;
  }

  function handlePeerContextMenu(e: MouseEvent, peer: PeerInfo) {
    e.preventDefault();
    contextMenuPeer = peer;
    contextMenuPos = { x: e.clientX, y: e.clientY };
    if (peerVolumeMap[peer.node_id] === undefined) {
      peerVolumeMap[peer.node_id] = 1.0;
    }
  }

  function closeContextMenu() {
    contextMenuPeer = null;
  }

  async function updatePeerVolume(nodeId: string, volume: number) {
    peerVolumeMap[nodeId] = volume;
    await api.setPeerVolume(nodeId, volume);
  }

  async function handleTransferHost(targetNodeId: string) {
    try {
      await api.transferHost(targetNodeId);
      closeContextMenu();
    } catch (err) {
      console.error("Failed to transfer host:", err);
    }
  }

  let isCurrentHost = $derived(
    !!(roomState && profile && roomState.host_node_id === profile.node_id)
  );
</script>

<svelte:window onclick={closeContextMenu} />

<div class="call-view-container">
  {#if !roomState}
    <div class="idle-call-state">
      <div class="idle-icon">🎙️</div>
      <h3>No Active Voice Call</h3>
      <p>Join or create a room from the sidebar to start speaking with end-to-end P2P mesh audio.</p>
    </div>
  {:else}
    <div class="call-view-header">
      <div class="call-title-wrap">
        <span class="live-dot"></span>
        <span class="room-title">Room: {roomState.room_id}</span>
        <span class="peer-count">({roomState.peers.length} active peers)</span>
      </div>
      <div class="call-meta-badges">
        <span class="meta-badge">QUIC Mesh Relay Fallback</span>
        <span class="meta-badge">48kHz RNNoise Denoised</span>
      </div>
    </div>

    <div class="peers-grid">
      {#each roomState.peers as peer}
        <div
          class="peer-card"
          class:speaking={isSpeaking(peer.node_id)}
          oncontextmenu={(e) => handlePeerContextMenu(e, peer)}
          role="button"
          tabindex="0"
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              handlePeerContextMenu(e as unknown as MouseEvent, peer);
            }
          }}
        >
          <div class="avatar-ring-container">
            <div
              class="peer-avatar"
              class:is-speaking={isSpeaking(peer.node_id)}
            >
              {peer.nickname ? peer.nickname.slice(0, 1).toUpperCase() : "P"}
            </div>
            {#if peer.is_host}
              <div class="host-crown-badge" title="Room Host">👑</div>
            {/if}
            {#if peer.is_muted}
              <div class="muted-badge" title="Muted">🔇</div>
            {/if}
          </div>

          <div class="peer-name-row">
            <span class="peer-nickname">{peer.nickname}</span>
            {#if peer.node_id === profile?.node_id}
              <span class="you-tag">(You)</span>
            {/if}
          </div>

          <div class="volume-bar-wrap">
            <div
              class="volume-bar-fill"
              style="width: {Math.min(100, (audioLevels[peer.node_id] || 0) * 200)}%;"
            ></div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<!-- Right-Click Context Menu -->
{#if contextMenuPeer}
  <div
    class="peer-context-menu"
    style="top: {contextMenuPos.y}px; left: {contextMenuPos.x}px;"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="menu"
    tabindex="-1"
  >
    <div class="menu-header">
      <span>{contextMenuPeer.nickname}</span>
      {#if contextMenuPeer.is_host}
        <span class="crown-mini">👑 Host</span>
      {/if}
    </div>

    <!-- Peer Volume Slider (0 - 200%) -->
    <div class="menu-item-slider">
      <div class="slider-labels">
        <span>Volume</span>
        <span>{Math.round((peerVolumeMap[contextMenuPeer.node_id] ?? 1.0) * 100)}%</span>
      </div>
      <input
        type="range"
        min="0"
        max="2"
        step="0.05"
        value={peerVolumeMap[contextMenuPeer.node_id] ?? 1.0}
        oninput={(e) =>
          updatePeerVolume(
            contextMenuPeer!.node_id,
            parseFloat((e.target as HTMLInputElement).value)
          )}
      />
    </div>

    <!-- Make Host Button (visible only if current user is Host) -->
    {#if isCurrentHost && contextMenuPeer.node_id !== profile?.node_id}
      <button
        class="menu-btn-action"
        onclick={() => handleTransferHost(contextMenuPeer!.node_id)}
      >
        👑 Transfer Host Role
      </button>
    {/if}
  </div>
{/if}

<style>
  .call-view-container {
    height: 48%;
    min-height: 240px;
    background-color: var(--bg-primary);
    border-bottom: var(--border-width) solid var(--border-color);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    position: relative;
  }

  .idle-call-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    padding: 24px;
    text-align: center;
  }

  .idle-icon {
    font-size: 2.8rem;
    margin-bottom: 12px;
    opacity: 0.8;
  }

  .idle-call-state h3 {
    margin: 0 0 6px 0;
    color: var(--text-primary);
    font-size: 1.15rem;
  }

  .idle-call-state p {
    margin: 0;
    font-size: 0.88rem;
    max-width: 440px;
  }

  .call-view-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 18px;
    background: var(--bg-secondary);
    border-bottom: var(--border-width) solid var(--border-color);
  }

  .call-title-wrap {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .live-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--voice-speaking-ring);
    box-shadow: 0 0 8px var(--voice-speaking-ring);
  }

  .room-title {
    font-weight: 700;
    font-size: 0.95rem;
    color: var(--text-primary);
  }

  .peer-count {
    font-size: 0.82rem;
    color: var(--text-muted);
  }

  .call-meta-badges {
    display: flex;
    gap: 8px;
  }

  .meta-badge {
    font-size: 0.72rem;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    padding: 4px 8px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
  }

  .peers-grid {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 16px;
    padding: 18px;
    overflow-y: auto;
    align-content: start;
  }

  .peer-card {
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 14px 10px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    cursor: context-menu;
    transition: all 0.2s ease;
    user-select: none;
  }

  .peer-card:hover {
    border-color: var(--accent-primary);
    transform: translateY(-2px);
  }

  .peer-card.speaking {
    border-color: var(--voice-speaking-ring);
    box-shadow: 0 0 12px rgba(16, 185, 129, 0.2);
  }

  .avatar-ring-container {
    position: relative;
    width: 60px;
    height: 60px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .peer-avatar {
    width: 54px;
    height: 54px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 1.3rem;
    font-weight: 700;
    transition: all 0.15s ease;
  }

  .peer-avatar.is-speaking {
    outline: 3px solid var(--voice-speaking-ring);
    outline-offset: 3px;
  }

  .host-crown-badge {
    position: absolute;
    top: -6px;
    right: -4px;
    font-size: 1rem;
    filter: drop-shadow(0 2px 4px rgba(0,0,0,0.4));
  }

  .muted-badge {
    position: absolute;
    bottom: -2px;
    right: -2px;
    font-size: 0.9rem;
    background: var(--bg-primary);
    border-radius: 50%;
    padding: 2px;
  }

  .peer-name-row {
    display: flex;
    align-items: center;
    gap: 4px;
    max-width: 100%;
    overflow: hidden;
  }

  .peer-nickname {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .you-tag {
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  .volume-bar-wrap {
    width: 80%;
    height: 4px;
    background: var(--bg-secondary);
    border-radius: 2px;
    overflow: hidden;
  }

  .volume-bar-fill {
    height: 100%;
    background: var(--voice-speaking-ring);
    transition: width 0.05s ease-out;
  }

  .peer-context-menu {
    position: fixed;
    z-index: 1000;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    min-width: 200px;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .menu-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-weight: 600;
    font-size: 0.85rem;
    color: var(--text-primary);
    padding: 4px 6px;
    border-bottom: 1px solid var(--border-color);
  }

  .crown-mini {
    font-size: 0.75rem;
    color: #eab308;
  }

  .menu-item-slider {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px 6px;
  }

  .slider-labels {
    display: flex;
    justify-content: space-between;
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .menu-item-slider input[type="range"] {
    accent-color: var(--accent-primary);
  }

  .menu-btn-action {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 6px 10px;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s;
  }

  .menu-btn-action:hover {
    background: var(--accent-primary);
    color: #ffffff;
  }
</style>
