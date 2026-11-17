<script lang="ts">
  import { onMount } from "svelte";
  import type { UserProfile, Contact, RoomState, Message, DirectCallRingPayload } from "$lib/types";
  import { initTheme, type ThemeConfig } from "$lib/theme";
  import { api } from "$lib/api";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import CallView from "$lib/components/CallView.svelte";
  import ChatView from "$lib/components/ChatView.svelte";
  import SettingsModal from "$lib/components/SettingsModal.svelte";

  let currentTheme = $state<ThemeConfig>(initTheme());
  let profile = $state<UserProfile | null>(null);
  let contacts = $state<Contact[]>([]);
  let roomState = $state<RoomState | null>(null);
  let activeChatId = $state<string>("lobby");
  let messages = $state<Message[]>([]);
  let audioLevels = $state<Record<string, number>>({});
  let isMuted = $state<boolean>(false);
  let isDeafened = $state<boolean>(false);
  let showSettingsModal = $state<boolean>(false);
  let isMobileDrawerOpen = $state<boolean>(false);
  let incomingCall = $state<DirectCallRingPayload | null>(null);

  // Swipe gesture detection
  let touchStartX = 0;
  let touchStartY = 0;

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length === 1) {
      touchStartX = e.touches[0].clientX;
      touchStartY = e.touches[0].clientY;
    }
  }

  function handleTouchEnd(e: TouchEvent) {
    if (e.changedTouches.length === 1) {
      const touchEndX = e.changedTouches[0].clientX;
      const touchEndY = e.changedTouches[0].clientY;
      const deltaX = touchEndX - touchStartX;
      const deltaY = touchEndY - touchStartY;

      // Swipe right from left edge to open
      if (deltaX > 70 && Math.abs(deltaY) < 60 && touchStartX < 50) {
        isMobileDrawerOpen = true;
      }
      // Swipe left to close drawer
      else if (deltaX < -70 && Math.abs(deltaY) < 60 && isMobileDrawerOpen) {
        isMobileDrawerOpen = false;
      }
    }
  }

  onMount(() => {
    // Disable default WebView right-click context menu
    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();
    };
    window.addEventListener("contextmenu", handleContextMenu);

    // 1. Initialize Theme
    currentTheme = initTheme();

    // 2. Fetch Initial User Profile
    api.getMyProfile().then((p) => {
      profile = p;
    }).catch(console.error);

    // 3. Fetch Contacts
    loadContacts();

    // 4. Fetch Room State
    api.getRoomState().then((r) => {
      roomState = r;
      if (r) {
        activeChatId = r.room_id;
      }
    }).catch(console.error);

    // 5. Register Event Listeners
    let unlistenLevels: (() => void) | undefined;
    let unlistenHost: (() => void) | undefined;
    let unlistenPeers: (() => void) | undefined;
    let unlistenChat: (() => void) | undefined;
    let unlistenChatReceived: (() => void) | undefined;
    let unlistenChatDelivered: (() => void) | undefined;
    let unlistenIncomingCall: (() => void) | undefined;

    api.onAudioLevels((levels) => {
      audioLevels = levels;
    }).then((u) => (unlistenLevels = u));

    api.onHostChanged((payload) => {
      if (roomState && roomState.room_id === payload.room_id) {
        roomState.host_node_id = payload.new_host_id;
        for (const peer of roomState.peers) {
          peer.is_host = peer.node_id === payload.new_host_id;
        }
      }
    }).then((u) => (unlistenHost = u));

    api.onPeersUpdated((newState) => {
      roomState = newState;
    }).then((u) => (unlistenPeers = u));

    api.onChatMessage((msg) => {
      handleIncomingMessage(msg);
    }).then((u) => (unlistenChat = u));

    api.onChatMessageReceived((msg) => {
      handleIncomingMessage(msg);
    }).then((u) => (unlistenChatReceived = u));

    api.onChatMessageDelivered((payload) => {
      handleMessageDelivered(payload.message_id);
    }).then((u) => (unlistenChatDelivered = u));

    api.onIncomingCall((ring) => {
      incomingCall = ring;
    }).then((u) => (unlistenIncomingCall = u));

    return () => {
      window.removeEventListener("contextmenu", handleContextMenu);
      unlistenLevels?.();
      unlistenHost?.();
      unlistenPeers?.();
      unlistenChat?.();
      unlistenChatReceived?.();
      unlistenChatDelivered?.();
      unlistenIncomingCall?.();
    };
  });

  $effect(() => {
    // Whenever activeChatId changes, fetch latest messages
    if (activeChatId) {
      api.getMessages(activeChatId, 50).then((res) => {
        messages = res;
      }).catch(console.error);
    }
  });

  async function loadContacts() {
    try {
      contacts = await api.getContacts();
    } catch (err) {
      console.error("Failed to load contacts:", err);
    }
  }

  function handleIncomingMessage(msg: Message) {
    const idx = messages.findIndex((m) => m.id === msg.id);
    if (idx !== -1) {
      messages[idx] = msg;
      messages = [...messages];
    } else if (msg.chat_id === activeChatId || msg.sender_id === activeChatId) {
      messages = [...messages, msg];
    }
  }

  function handleMessageDelivered(messageId: string) {
    const idx = messages.findIndex((m) => m.id === messageId);
    if (idx !== -1) {
      messages[idx].delivered = 1;
      messages = [...messages];
    }
  }

  function getContactName(nodeId: string): string {
    const found = contacts.find((c) => c.node_id === nodeId);
    if (found && found.nickname) return found.nickname;
    return nodeId.length > 12 ? `${nodeId.slice(0, 8)}...` : nodeId;
  }

  function handleAcceptCall() {
    if (!incomingCall) return;
    const ticket = incomingCall.room_ticket;
    const caller = incomingCall.caller_id;
    incomingCall = null;
    handleJoinRoom(ticket);
    activeChatId = caller;
  }

  function handleDeclineCall() {
    incomingCall = null;
  }

  async function handleJoinRoom(roomId: string) {
    try {
      const state = await api.joinRoom(roomId);
      roomState = state;
      activeChatId = roomId;
    } catch (err) {
      console.error("Failed to join room:", err);
    }
  }

  async function handleLeaveRoom() {
    try {
      await api.leaveRoom();
      roomState = null;
    } catch (err) {
      console.error("Failed to leave room:", err);
    }
  }

  async function handleDirectCall(contactNodeId: string) {
    try {
      const state = await api.startDirectCall(contactNodeId);
      roomState = state;
      activeChatId = contactNodeId;
    } catch (err) {
      console.error("Failed to start direct call:", err);
    }
  }

  async function handleAddContact(nodeId: string) {
    try {
      await api.addContact(nodeId);
      await loadContacts();
    } catch (err) {
      console.error("Failed to add contact:", err);
    }
  }

  async function handleSendMessage(content: string) {
    try {
      const sent = await api.sendMessage(activeChatId, content);
      messages = [...messages, sent];
    } catch (err) {
      console.error("Failed to send message:", err);
    }
  }

  function handleThemeApplied(theme: ThemeConfig) {
    currentTheme = theme;
  }
</script>

<svelte:window
  ontouchstart={handleTouchStart}
  ontouchend={handleTouchEnd}
/>

<main class="app-layout">
  <!-- Mobile Backdrop Overlay -->
  {#if isMobileDrawerOpen}
    <div
      class="mobile-backdrop"
      onclick={() => (isMobileDrawerOpen = false)}
      onkeydown={(e) => e.key === "Escape" && (isMobileDrawerOpen = false)}
      role="button"
      tabindex="-1"
      aria-label="Close menu drawer"
    ></div>
  {/if}

  <!-- Left: Sidebar Drawer Wrapper -->
  <div class="sidebar-wrapper" class:open={isMobileDrawerOpen}>
    <Sidebar
      {profile}
      {contacts}
      {roomState}
      bind:activeChatId
      bind:isMuted
      bind:isDeafened
      onOpenSettings={() => (showSettingsModal = true)}
      onJoinRoom={(roomId) => {
        handleJoinRoom(roomId);
        isMobileDrawerOpen = false;
      }}
      onLeaveRoom={handleLeaveRoom}
      onAddContact={handleAddContact}
      onDirectCall={handleDirectCall}
    />
  </div>

  <!-- Right: Voice Call Top Panel + Chat Bottom Panel -->
  <section class="main-content-panel">
    <!-- Mobile Header Bar with Hamburger Button -->
    <div class="mobile-header-bar">
      <button
        class="hamburger-btn"
        aria-label="Toggle navigation drawer"
        onclick={() => (isMobileDrawerOpen = !isMobileDrawerOpen)}
      >
        ☰
      </button>
      <span class="mobile-chat-title">
        {activeChatId.startsWith("room_") || activeChatId.length < 32
          ? `Room: ${activeChatId}`
          : `Direct: ${activeChatId.slice(0, 10)}...`}
      </span>
      <button
        class="mobile-settings-btn"
        aria-label="Open settings"
        onclick={() => (showSettingsModal = true)}
      >
        ⚙
      </button>
    </div>

    <!-- Active Call Compact Banner (when navigating to direct contact DMs during a call) -->
    {#if roomState && activeChatId !== roomState.room_id}
      <div class="active-call-compact-banner">
        <div
          class="banner-left"
          onclick={() => (activeChatId = roomState!.room_id)}
          role="button"
          tabindex="0"
          onkeydown={(e) => e.key === "Enter" && (activeChatId = roomState!.room_id)}
        >
          <span class="live-dot pulse"></span>
          <span class="banner-title">
            🔊 {roomState.room_id.startsWith("call_") ? "1-on-1 Call" : "In Voice Room"}: <strong>{roomState.room_id}</strong>
          </span>
          <span class="banner-peer-count">
            ({roomState.peers.length} active {roomState.peers.length === 1 ? "peer" : "peers"})
          </span>
        </div>
        <div class="banner-right">
          <button
            class="banner-btn return-btn"
            onclick={() => (activeChatId = roomState!.room_id)}
          >
            Return to Call
          </button>
          <button class="banner-btn leave-btn" onclick={handleLeaveRoom}>
            Leave Call
          </button>
        </div>
      </div>
    {:else if !roomState && activeChatId !== "lobby" && !activeChatId.startsWith("room_") && !activeChatId.startsWith("call_")}
      <!-- Direct DM without active call: let ChatView take full height -->
    {:else}
      <CallView {roomState} {profile} {audioLevels} />
    {/if}

    <ChatView
      {activeChatId}
      {profile}
      {messages}
      onSendMessage={handleSendMessage}
      onThemeApplied={handleThemeApplied}
      onJoinRoom={handleJoinRoom}
      onDirectCall={handleDirectCall}
    />
  </section>

  <!-- Settings Modal (Voice, Network, Theme Studio, Device Linking) -->
  {#if showSettingsModal}
    <SettingsModal
      bind:currentTheme
      onClose={() => (showSettingsModal = false)}
      onSyncCompleted={() => {
        loadContacts();
        if (activeChatId) {
          api.getMessages(activeChatId, 50).then((res) => (messages = res));
        }
      }}
    />
  {/if}
  <!-- Incoming 1-on-1 Call Overlay Modal -->
  {#if incomingCall}
    <div class="incoming-call-overlay" role="dialog" aria-modal="true" tabindex="-1">
      <div class="incoming-call-card">
        <div class="call-pulse-animation">
          <div class="call-avatar-icon">📞</div>
        </div>
        <h3 class="incoming-title">Incoming 1-on-1 Call</h3>
        <p class="caller-name">From: <strong>{getContactName(incomingCall.caller_id)}</strong></p>
        <span class="call-badge">Direct Encrypted P2P Voice</span>
        <div class="incoming-call-actions">
          <button class="call-action-btn decline-btn" onclick={handleDeclineCall}>
            ✕ Decline
          </button>
          <button class="call-action-btn accept-btn" onclick={handleAcceptCall}>
            ✓ Accept
          </button>
        </div>
      </div>
    </div>
  {/if}
</main>

<style>
  :global(:root) {
    --bg-primary: #0f1115;
    --bg-secondary: #161920;
    --bg-tertiary: #1f232d;
    --surface-card: #252a36;
    --border-color: #2d3443;
    --border-width: 1px;
    --border-radius: 8px;
    --text-primary: #f1f3f7;
    --text-muted: #8b94a5;
    --accent-primary: #4f46e5;
    --accent-hover: #6366f1;
    --voice-speaking-ring: #10b981;
    --danger-color: #ef4444;
    --online-status-color: #10b981;
  }

  :global(body), :global(html) {
    margin: 0;
    padding: 0;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background-color: var(--bg-primary);
    color: var(--text-primary);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, Cantarell, "Open Sans", "Helvetica Neue", sans-serif;
    -webkit-font-smoothing: antialiased;
  }

  :global(*), :global(*::before), :global(*::after) {
    box-sizing: border-box;
  }

  .app-layout {
    display: flex;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    position: relative;
  }

  .sidebar-wrapper {
    display: contents;
  }

  .main-content-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }

  .active-call-compact-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 18px;
    background: var(--bg-secondary);
    border-bottom: var(--border-width) solid var(--accent-primary);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.25);
    z-index: 15;
  }

  .banner-left {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    flex: 1;
  }

  .banner-title {
    font-size: 0.9rem;
    color: var(--text-primary);
  }

  .banner-title strong {
    color: var(--accent-hover);
  }

  .banner-peer-count {
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .banner-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .banner-btn {
    border-radius: var(--border-radius);
    padding: 5px 12px;
    font-size: 0.78rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .return-btn {
    background: var(--accent-primary);
    border: none;
    color: #ffffff;
  }

  .return-btn:hover {
    background: var(--accent-hover);
  }

  .leave-btn {
    background: transparent;
    border: var(--border-width) solid var(--danger-color);
    color: var(--danger-color);
  }

  .leave-btn:hover {
    background: var(--danger-color);
    color: #ffffff;
  }

  .live-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--voice-speaking-ring);
    box-shadow: 0 0 8px var(--voice-speaking-ring);
    flex-shrink: 0;
  }

  .live-dot.pulse {
    animation: pulse-glow 2s infinite;
  }

  @keyframes pulse-glow {
    0% {
      transform: scale(0.95);
      box-shadow: 0 0 0 0 rgba(16, 185, 129, 0.7);
    }
    70% {
      transform: scale(1.1);
      box-shadow: 0 0 0 6px rgba(16, 185, 129, 0);
    }
    100% {
      transform: scale(0.95);
      box-shadow: 0 0 0 0 rgba(16, 185, 129, 0);
    }
  }

  .mobile-header-bar {
    display: none;
  }

  @media (max-width: 768px) {
    .sidebar-wrapper {
      display: block;
      position: fixed;
      top: 0;
      left: 0;
      bottom: 0;
      width: 280px;
      z-index: 150;
      transform: translateX(-100%);
      transition: transform 0.28s cubic-bezier(0.4, 0, 0.2, 1);
    }

    .sidebar-wrapper.open {
      transform: translateX(0);
      box-shadow: 6px 0 24px rgba(0, 0, 0, 0.6);
    }

    .mobile-backdrop {
      position: fixed;
      inset: 0;
      background: rgba(0, 0, 0, 0.65);
      z-index: 140;
    }

    .mobile-header-bar {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 10px 14px;
      background: var(--bg-secondary);
      border-bottom: var(--border-width) solid var(--border-color);
      z-index: 10;
    }

    .hamburger-btn, .mobile-settings-btn {
      background: transparent;
      border: none;
      color: var(--text-primary);
      font-size: 1.3rem;
      cursor: pointer;
      padding: 4px 8px;
      border-radius: var(--border-radius);
    }

    .hamburger-btn:hover, .mobile-settings-btn:hover {
      background: var(--bg-tertiary);
    }

    .mobile-chat-title {
      font-weight: 600;
      font-size: 0.9rem;
      color: var(--text-primary);
    }
  }

  .incoming-call-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 500;
    backdrop-filter: blur(4px);
  }

  .incoming-call-card {
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--accent-primary);
    border-radius: calc(var(--border-radius) * 1.5);
    padding: 32px 36px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6);
    max-width: 90vw;
    width: 360px;
    text-align: center;
    animation: scale-up 0.25s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes scale-up {
    0% {
      opacity: 0;
      transform: scale(0.9);
    }
    100% {
      opacity: 1;
      transform: scale(1);
    }
  }

  .call-pulse-animation {
    width: 76px;
    height: 76px;
    border-radius: 50%;
    background: rgba(79, 70, 229, 0.2);
    display: flex;
    align-items: center;
    justify-content: center;
    animation: ring-pulse 1.8s infinite;
  }

  @keyframes ring-pulse {
    0% {
      box-shadow: 0 0 0 0 rgba(79, 70, 229, 0.6);
    }
    70% {
      box-shadow: 0 0 0 18px rgba(79, 70, 229, 0);
    }
    100% {
      box-shadow: 0 0 0 0 rgba(79, 70, 229, 0);
    }
  }

  .call-avatar-icon {
    font-size: 2.2rem;
  }

  .incoming-title {
    margin: 4px 0 0 0;
    font-size: 1.25rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .caller-name {
    margin: 0;
    font-size: 0.95rem;
    color: var(--text-muted);
  }

  .caller-name strong {
    color: var(--text-primary);
  }

  .call-badge {
    background: var(--bg-tertiary);
    color: var(--online-status-color);
    font-size: 0.75rem;
    padding: 3px 10px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
  }

  .incoming-call-actions {
    display: flex;
    gap: 16px;
    margin-top: 14px;
    width: 100%;
  }

  .call-action-btn {
    flex: 1;
    padding: 10px 16px;
    border-radius: var(--border-radius);
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    border: none;
    transition: all 0.15s ease;
  }

  .decline-btn {
    background: var(--bg-tertiary);
    color: var(--danger-color);
    border: var(--border-width) solid var(--danger-color);
  }

  .decline-btn:hover {
    background: var(--danger-color);
    color: #ffffff;
  }

  .accept-btn {
    background: var(--voice-speaking-ring);
    color: #ffffff;
  }

  .accept-btn:hover {
    filter: brightness(1.1);
  }
</style>
