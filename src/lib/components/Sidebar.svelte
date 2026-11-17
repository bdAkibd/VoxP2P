<script lang="ts">
  import type { UserProfile, Contact, RoomState } from "../types";
  import { api } from "../api";
  import Avatar from "./Avatar.svelte";

  let {
    profile,
    contacts,
    roomState,
    activeChatId = $bindable(),
    isMuted = $bindable(),
    isDeafened = $bindable(),
    onOpenSettings,
    onJoinRoom,
    onLeaveRoom,
    onAddContact,
    onDirectCall,
  }: {
    profile: UserProfile | null;
    contacts: Contact[];
    roomState: RoomState | null;
    activeChatId: string;
    isMuted: boolean;
    isDeafened: boolean;
    onOpenSettings: () => void;
    onJoinRoom: (roomId: string) => void;
    onLeaveRoom: () => void;
    onAddContact: (nodeId: string) => void;
    onDirectCall?: (nodeId: string) => void;
  } = $props();

  let targetRoomInput = $state("");
  let showAddContactModal = $state(false);
  let newContactNodeId = $state("");
  let copyFeedback = $state(false);
  let isEditingNickname = $state(false);
  let nicknameDraft = $state("");

  function formatShortId(id: string): string {
    if (!id) return "";
    return id.length > 20 ? `${id.slice(0, 10)}...${id.slice(-8)}` : id;
  }

  function copyNodeId() {
    if (!profile?.node_id) return;
    navigator.clipboard.writeText(profile.node_id);
    copyFeedback = true;
    setTimeout(() => {
      copyFeedback = false;
    }, 2000);
  }

  async function toggleMute() {
    const next = !isMuted;
    isMuted = next;
    await api.setMute(next);
  }

  async function toggleDeafen() {
    const next = !isDeafened;
    isDeafened = next;
    await api.setDeafen(next);
  }

  function handleJoinRoom() {
    if (!targetRoomInput.trim()) return;
    onJoinRoom(targetRoomInput.trim());
    targetRoomInput = "";
  }

  function handleAddContactSubmit() {
    if (!newContactNodeId.trim()) return;
    onAddContact(newContactNodeId.trim());
    newContactNodeId = "";
    showAddContactModal = false;
  }

  function startEditNickname() {
    nicknameDraft = profile?.nickname || "";
    isEditingNickname = true;
  }

  async function saveNickname() {
    if (nicknameDraft.trim() && profile) {
      await api.updateProfile(nicknameDraft.trim(), profile.avatar_hash);
      profile.nickname = nicknameDraft.trim();
    }
    isEditingNickname = false;
  }
</script>

<aside class="sidebar">
  <!-- Top: App Branding & Status -->
  <div class="sidebar-header">
    <div class="brand">
      <div class="brand-logo">V</div>
      <div class="brand-text">
        <span class="brand-title">VoxP2P</span>
        <span class="brand-sub">Serverless Mesh</span>
      </div>
    </div>
    <button class="icon-btn" title="Settings" onclick={onOpenSettings}>
      ⚙
    </button>
  </div>

  <!-- Room Management Section -->
  <div class="sidebar-section">
    <div class="section-title">VOICE ROOM</div>
    {#if roomState}
      <div
        class="active-room-card"
        class:active={activeChatId === roomState.room_id}
        role="button"
        tabindex="0"
        onclick={() => (activeChatId = roomState.room_id)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            activeChatId = roomState.room_id;
          }
        }}
      >
        <div class="room-info">
          <span class="room-icon">🔊</span>
          <div class="room-details">
            <span class="room-name">{roomState.room_id.startsWith("call_") ? "1-on-1 Direct Call" : roomState.room_id}</span>
            <span class="room-meta">{roomState.room_id.startsWith("call_") ? "Private 2-Peer Call" : `${roomState.peers.length} connected`}</span>
          </div>
        </div>
        <button
          class="leave-btn"
          title="Leave Room"
          onclick={(e) => {
            e.stopPropagation();
            onLeaveRoom();
          }}
        >
          ✕
        </button>
      </div>
    {:else}
      <div class="join-room-row">
        <input
          type="text"
          class="sidebar-input"
          placeholder="Room ID or Ticket..."
          bind:value={targetRoomInput}
          onkeydown={(e) => e.key === "Enter" && handleJoinRoom()}
        />
        <button class="sidebar-action-btn" onclick={handleJoinRoom}>Join</button>
      </div>
    {/if}
  </div>

  <!-- Contacts List -->
  <div class="sidebar-section contacts-section">
    <div class="section-header-row">
      <span class="section-title">DIRECT CONTACTS ({contacts.length})</span>
      <button class="add-contact-btn" title="Add Contact" onclick={() => (showAddContactModal = true)}>
        + Add
      </button>
    </div>

    <div class="contacts-list">
      {#if contacts.length === 0}
        <div class="empty-state">No contacts added yet.</div>
      {:else}
        {#each contacts as contact}
          <div
            class="contact-item"
            class:active={activeChatId === contact.node_id}
            role="button"
            tabindex="0"
            onclick={() => (activeChatId = contact.node_id)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                activeChatId = contact.node_id;
              }
            }}
          >
            <div class="contact-avatar">
              <Avatar
                hash={contact.avatar_path}
                nickname={contact.nickname}
                size={32}
              />
              <span class="online-indicator"></span>
            </div>
            <div class="contact-info">
              <span class="contact-name">{contact.nickname}</span>
              <span class="contact-id">{formatShortId(contact.node_id)}</span>
            </div>
            <button
              class="quick-call-btn"
              title="Start 1-on-1 Call"
              onclick={(e) => {
                e.stopPropagation();
                if (onDirectCall) {
                  onDirectCall(contact.node_id);
                } else {
                  onJoinRoom(`room_${contact.node_id.slice(0, 8)}`);
                }
              }}
            >
              📞
            </button>
          </div>
        {/each}
      {/if}
    </div>
  </div>

  <!-- Bottom: User Profile Badge -->
  <div class="user-profile-badge">
    <div class="user-avatar-wrap">
      <div
        class="user-avatar"
        onclick={onOpenSettings}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === "Enter" && onOpenSettings()}
        title="Profile Settings"
      >
        <Avatar
          hash={profile?.avatar_hash}
          nickname={profile?.nickname}
          size={38}
        />
      </div>
      <span class="online-indicator"></span>
    </div>

    <div class="user-info">
      {#if isEditingNickname}
        <input
          class="nickname-edit-input"
          bind:value={nicknameDraft}
          onblur={saveNickname}
          onkeydown={(e) => e.key === "Enter" && saveNickname()}
        />
      {:else}
        <button
          type="button"
          class="user-nickname-btn"
          title="Click to edit nickname"
          onclick={startEditNickname}
        >
          {profile?.nickname || "Connecting..."}
        </button>
      {/if}
      <button
        type="button"
        class="user-id-btn"
        title="Click to copy full NodeId"
        onclick={copyNodeId}
      >
        <span class="user-id">{formatShortId(profile?.node_id || "")}</span>
        <span class="copy-icon">{copyFeedback ? "✓" : "📋"}</span>
      </button>
    </div>

    <div class="audio-toggles">
      <button
        class="toggle-btn"
        class:active={isMuted}
        title={isMuted ? "Unmute Mic" : "Mute Mic"}
        onclick={toggleMute}
      >
        {isMuted ? "🔇" : "🎤"}
      </button>
      <button
        class="toggle-btn"
        class:active={isDeafened}
        title={isDeafened ? "Undeafen Audio" : "Deafen Audio"}
        onclick={toggleDeafen}
      >
        {isDeafened ? "🔈" : "🎧"}
      </button>
    </div>
  </div>
</aside>

<!-- Add Contact Modal -->
{#if showAddContactModal}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) showAddContactModal = false; }}
    onkeydown={(e) => e.key === "Escape" && (showAddContactModal = false)}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal-card" role="document">
      <div class="modal-header">
        <h4>Add Peer Contact</h4>
        <button class="modal-close-btn" onclick={() => (showAddContactModal = false)}>&times;</button>
      </div>
      <div class="modal-body">
        <label for="contact-input-id">Node ID (Public Key)</label>
        <input
          id="contact-input-id"
          type="text"
          class="modal-input"
          placeholder="Paste 64-character Ed25519 node ID..."
          bind:value={newContactNodeId}
        />
      </div>
      <div class="modal-footer">
        <button class="btn-cancel" onclick={() => (showAddContactModal = false)}>Cancel</button>
        <button class="btn-primary" onclick={handleAddContactSubmit}>Add Contact</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .sidebar {
    width: 280px;
    min-width: 280px;
    height: 100vh;
    background-color: var(--bg-secondary);
    border-right: var(--border-width) solid var(--border-color);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    user-select: none;
  }

  @media (max-width: 768px) {
    .sidebar {
      width: 100%;
      min-width: 0;
      height: 100%;
      border-right: none;
    }
  }

  .sidebar-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px;
    border-bottom: var(--border-width) solid var(--border-color);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .brand-logo {
    width: 32px;
    height: 32px;
    border-radius: var(--border-radius);
    background: var(--accent-primary);
    color: #ffffff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 800;
    font-size: 1.1rem;
  }

  .brand-text {
    display: flex;
    flex-direction: column;
  }

  .brand-title {
    font-weight: 700;
    font-size: 1rem;
    color: var(--text-primary);
  }

  .brand-sub {
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  .icon-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 1.15rem;
    cursor: pointer;
    min-width: 44px;
    min-height: 44px;
    padding: 8px;
    border-radius: var(--border-radius);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s;
  }

  .icon-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .sidebar-section {
    padding: 14px 16px;
    border-bottom: var(--border-width) solid var(--border-color);
  }

  .section-title {
    font-size: 0.72rem;
    font-weight: 700;
    color: var(--text-muted);
    letter-spacing: 0.06em;
    margin-bottom: 8px;
    display: block;
  }

  .active-room-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--bg-tertiary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 10px 12px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .active-room-card:hover, .active-room-card.active {
    border-color: var(--accent-primary);
  }

  .room-info {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 1;
    overflow: hidden;
  }

  .room-icon {
    font-size: 1.1rem;
  }

  .room-details {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .room-name {
    font-weight: 600;
    font-size: 0.9rem;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .room-meta {
    font-size: 0.75rem;
    color: var(--online-status-color);
  }

  .leave-btn {
    background: transparent;
    border: none;
    color: var(--danger-color);
    font-size: 1.1rem;
    cursor: pointer;
    min-width: 44px;
    min-height: 44px;
    padding: 8px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .leave-btn:hover {
    background: var(--bg-primary);
  }

  .join-room-row {
    display: flex;
    gap: 8px;
  }

  .sidebar-input {
    flex: 1;
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 7px 10px;
    color: var(--text-primary);
    font-size: 0.82rem;
  }

  .sidebar-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .sidebar-action-btn {
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    border-radius: var(--border-radius);
    padding: 6px 12px;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
  }

  .sidebar-action-btn:hover {
    background: var(--accent-hover);
  }

  .contacts-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-bottom: none;
  }

  .section-header-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 8px;
  }

  .add-contact-btn {
    background: transparent;
    border: none;
    color: var(--accent-primary);
    font-size: 0.78rem;
    font-weight: 600;
    cursor: pointer;
  }

  .add-contact-btn:hover {
    color: var(--accent-hover);
  }

  .contacts-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .empty-state {
    font-size: 0.8rem;
    color: var(--text-muted);
    padding: 16px 0;
    text-align: center;
  }

  .contact-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--border-radius);
    cursor: pointer;
    transition: background 0.15s;
  }

  .contact-item:hover {
    background: var(--bg-tertiary);
  }

  .contact-item.active {
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
  }

  .contact-avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    font-size: 0.85rem;
    position: relative;
    flex-shrink: 0;
  }

  .online-indicator {
    position: absolute;
    bottom: -1px;
    right: -1px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--online-status-color);
    border: 2px solid var(--bg-secondary);
  }

  .contact-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .contact-name {
    font-size: 0.88rem;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .contact-id {
    font-size: 0.72rem;
    color: var(--text-muted);
    font-family: monospace;
  }

  .quick-call-btn {
    background: transparent;
    border: none;
    font-size: 1.1rem;
    opacity: 0.75;
    cursor: pointer;
    min-width: 44px;
    min-height: 44px;
    padding: 8px;
    border-radius: var(--border-radius);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s ease;
  }

  .quick-call-btn:hover {
    opacity: 1;
    background: var(--bg-tertiary);
  }

  .user-profile-badge {
    background: var(--bg-primary);
    border-top: var(--border-width) solid var(--border-color);
    padding: 12px 14px;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .user-avatar-wrap {
    position: relative;
    flex-shrink: 0;
  }

  .user-avatar {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    background: var(--accent-primary);
    color: #ffffff;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    font-size: 0.95rem;
    overflow: hidden;
    cursor: pointer;
    transition: transform 0.15s ease;
  }

  .user-avatar:hover {
    transform: scale(1.05);
  }

  .user-info {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .user-nickname-btn {
    background: transparent;
    border: none;
    padding: 0;
    text-align: left;
    font-weight: 600;
    font-size: 0.88rem;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }

  .user-id-btn {
    background: transparent;
    border: none;
    padding: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    cursor: pointer;
    text-align: left;
  }

  .user-id {
    font-size: 0.7rem;
    color: var(--text-muted);
    font-family: monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .copy-icon {
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .audio-toggles {
    display: flex;
    gap: 6px;
  }

  .toggle-btn {
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    min-width: 44px;
    min-height: 44px;
    padding: 8px;
    font-size: 1.15rem;
    cursor: pointer;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s ease;
  }

  .toggle-btn:hover {
    background: var(--bg-tertiary);
  }

  .toggle-btn.active {
    background: var(--danger-color);
    border-color: var(--danger-color);
    color: #ffffff;
  }

  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal-card {
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    width: 440px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 14px 18px;
    border-bottom: var(--border-width) solid var(--border-color);
  }

  .modal-header h4 {
    margin: 0;
    font-size: 1rem;
    color: var(--text-primary);
  }

  .modal-close-btn {
    background: transparent;
    border: none;
    font-size: 1.3rem;
    color: var(--text-muted);
    cursor: pointer;
  }

  .modal-body {
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .modal-body label {
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .modal-input {
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 9px 12px;
    color: var(--text-primary);
    font-size: 0.85rem;
    font-family: monospace;
  }

  .modal-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 12px 18px;
    border-top: var(--border-width) solid var(--border-color);
    background: var(--bg-secondary);
  }

  .btn-cancel {
    background: transparent;
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 6px 14px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .btn-primary {
    background: var(--accent-primary);
    border: none;
    border-radius: var(--border-radius);
    padding: 6px 16px;
    color: #ffffff;
    font-weight: 600;
    cursor: pointer;
  }
</style>
