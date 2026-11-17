<script lang="ts">
  import { onMount } from "svelte";
  import type { UserProfile, Contact, RoomState, Message, DirectCallRingPayload } from "$lib/types";
  import { initTheme, type ThemeConfig } from "$lib/theme";
  import { api } from "$lib/api";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import CallView from "$lib/components/CallView.svelte";
  import ChatView from "$lib/components/ChatView.svelte";
  import SettingsModal from "$lib/components/SettingsModal.svelte";
  import Avatar from "$lib/components/Avatar.svelte";

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
  let incomingCall = $state<DirectCallRingPayload | null>(null);
  let isMobile = $state<boolean>(false);
  let mobileTab = $state<"chats" | "call" | "contacts" | "settings">("chats");
  let settingsInitialTab = $state<"audio" | "network" | "theme" | "sync">("audio");

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
    if (e.changedTouches.length === 1 && isMobile) {
      const touchEndX = e.changedTouches[0].clientX;
      const touchEndY = e.changedTouches[0].clientY;
      const deltaX = touchEndX - touchStartX;
      const deltaY = touchEndY - touchStartY;

      // Horizontal swipe between mobile tabs
      if (Math.abs(deltaX) > 60 && Math.abs(deltaY) < 60) {
        const tabs: Array<"chats" | "call" | "contacts" | "settings"> = ["chats", "call", "contacts", "settings"];
        const currentIndex = tabs.indexOf(mobileTab);
        if (deltaX < 0 && currentIndex < tabs.length - 1) {
          mobileTab = tabs[currentIndex + 1];
        } else if (deltaX > 0 && currentIndex > 0) {
          mobileTab = tabs[currentIndex - 1];
        }
      }
    }
  }

  function openSettings(tab: "audio" | "network" | "theme" | "sync" = "audio") {
    settingsInitialTab = tab;
    showSettingsModal = true;
  }

  onMount(() => {
    // Disable default WebView right-click context menu
    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();
    };
    window.addEventListener("contextmenu", handleContextMenu);

    // Responsive viewport check
    const checkViewport = () => {
      isMobile = window.innerWidth < 768;
    };
    checkViewport();
    window.addEventListener("resize", checkViewport);

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
    let unlistenSyncImport: (() => void) | undefined;
    let unlistenProfileUpdated: (() => void) | undefined;
    let unlistenContactsUpdated: (() => void) | undefined;
    let unlistenChatEdited: (() => void) | undefined;
    let unlistenChatDeleted: (() => void) | undefined;

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

    api.onSyncImportCompleted((payload) => {
      api.getMyProfile().then((p) => {
        profile = p;
      }).catch(console.error);
      loadContacts();
      if (activeChatId) {
        api.getMessages(activeChatId, 50).then((res) => (messages = res)).catch(console.error);
      }
    }).then((u) => (unlistenSyncImport = u));

    api.onProfileUpdated((p) => {
      profile = p;
    }).then((u) => (unlistenProfileUpdated = u));

    api.onContactsUpdated(() => {
      loadContacts();
    }).then((u) => (unlistenContactsUpdated = u));

    api.onChatMessageEdited((payload) => {
      const idx = messages.findIndex((m) => m.id === payload.message_id);
      if (idx !== -1) {
        messages[idx] = { ...messages[idx], content: payload.new_content, is_edited: 1 };
        messages = [...messages];
      }
    }).then((u) => (unlistenChatEdited = u));

    api.onChatMessageDeleted((payload) => {
      messages = messages.filter((m) => m.id !== payload.message_id);
    }).then((u) => (unlistenChatDeleted = u));

    return () => {
      window.removeEventListener("contextmenu", handleContextMenu);
      window.removeEventListener("resize", checkViewport);
      unlistenLevels?.();
      unlistenHost?.();
      unlistenPeers?.();
      unlistenChat?.();
      unlistenChatReceived?.();
      unlistenChatDelivered?.();
      unlistenIncomingCall?.();
      unlistenSyncImport?.();
      unlistenProfileUpdated?.();
      unlistenContactsUpdated?.();
      unlistenChatEdited?.();
      unlistenChatDeleted?.();
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
      if (!messages.some((m) => m.id === msg.id)) {
        messages = [...messages, msg];
      }
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
      if (isMobile) {
        mobileTab = "call";
      }
    } catch (err) {
      console.error("Failed to join room:", err);
    }
  }

  async function handleLeaveRoom() {
    try {
      await api.leaveRoom();
      roomState = null;
      if (isMobile && mobileTab === "call") {
        mobileTab = "chats";
      }
    } catch (err) {
      console.error("Failed to leave room:", err);
    }
  }

  async function handleDirectCall(contactNodeId: string) {
    try {
      const state = await api.startDirectCall(contactNodeId);
      roomState = state;
      activeChatId = contactNodeId;
      if (isMobile) {
        mobileTab = "call";
      }
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
      if (!messages.some((m) => m.id === sent.id)) {
        messages = [...messages, sent];
      }
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

<main class="app-layout" class:mobile-mode={isMobile}>
  {#if !isMobile}
    <!-- DESKTOP 2-COLUMN VIEW -->
    <div class="sidebar-wrapper">
      <Sidebar
        {profile}
        {contacts}
        {roomState}
        bind:activeChatId
        bind:isMuted
        bind:isDeafened
        onOpenSettings={() => openSettings("audio")}
        onJoinRoom={handleJoinRoom}
        onLeaveRoom={handleLeaveRoom}
        onAddContact={handleAddContact}
        onDirectCall={handleDirectCall}
      />
    </div>

    <!-- Right: Voice Call Top Panel + Chat Bottom Panel -->
    <section class="main-content-panel">
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
        {contacts}
        {messages}
        onSendMessage={handleSendMessage}
        onThemeApplied={handleThemeApplied}
        onJoinRoom={handleJoinRoom}
        onDirectCall={handleDirectCall}
      />
    </section>

  {:else}
    <!-- MOBILE VIEWPORT WITH BOTTOM NAVIGATION TABS -->
    <div class="mobile-main-container">
      {#if mobileTab === "chats"}
        <div class="mobile-tab-content">
          <div class="mobile-header-bar">
            <div class="mobile-header-left">
              {#if activeChatId !== "lobby" && !activeChatId.startsWith("room_")}
                {@const currentContact = contacts.find((c) => c.node_id === activeChatId)}
                <Avatar hash={currentContact?.avatar_path} nickname={currentContact?.nickname || activeChatId} size={32} />
              {/if}
              <div class="mobile-header-title-box">
                <span class="mobile-chat-title">{getContactName(activeChatId)}</span>
                {#if activeChatId !== "lobby"}
                  <span class="mobile-chat-sub">
                    {activeChatId.startsWith("room_") ? "Mesh Room" : "Direct Message"}
                  </span>
                {/if}
              </div>
            </div>
            <div class="mobile-header-actions">
              {#if activeChatId !== "lobby" && !activeChatId.startsWith("room_")}
                <button
                  class="mobile-touch-btn"
                  title="Call Contact"
                  onclick={() => handleDirectCall(activeChatId)}
                  aria-label="Call Contact"
                >
                  📞
                </button>
              {/if}
              <button
                class="mobile-touch-btn"
                title="Scan QR Code"
                onclick={() => openSettings("sync")}
                aria-label="Scan QR Code"
              >
                📷
              </button>
            </div>
          </div>

          {#if roomState}
            <div
              class="active-call-compact-banner mobile-clickable"
              onclick={() => (mobileTab = "call")}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === "Enter" && (mobileTab = "call")}
            >
              <div class="banner-left">
                <span class="live-dot pulse"></span>
                <span class="banner-title">
                  🔊 Active Call: <strong>{roomState.room_id}</strong>
                </span>
              </div>
              <button class="banner-btn return-btn" onclick={() => (mobileTab = "call")}>
                View Call ›
              </button>
            </div>
          {/if}

          <div class="mobile-screen-scrollable">
            <ChatView
              {activeChatId}
              {profile}
              {contacts}
              {messages}
              onSendMessage={handleSendMessage}
              onThemeApplied={handleThemeApplied}
              onJoinRoom={(rid) => {
                handleJoinRoom(rid);
                mobileTab = "call";
              }}
              onDirectCall={(nid) => {
                handleDirectCall(nid);
                mobileTab = "call";
              }}
            />
          </div>
        </div>

      {:else if mobileTab === "call"}
        <div class="mobile-tab-content">
          <div class="mobile-header-bar">
            <span class="mobile-chat-title">🔊 Active Voice Call</span>
            {#if roomState}
              <button class="banner-btn leave-btn" onclick={handleLeaveRoom}>
                Leave Call
              </button>
            {/if}
          </div>

          <div class="mobile-screen-scrollable">
            {#if roomState}
              <CallView {roomState} {profile} {audioLevels} />
            {:else}
              <div class="mobile-idle-screen">
                <div class="idle-card-box">
                  <div class="idle-big-icon">🎙️</div>
                  <h3>No Active Call</h3>
                  <p>You are not currently in a voice call. Join the general lobby or call a contact directly.</p>
                  <div class="idle-actions-col">
                    <button class="btn-primary hero-btn" onclick={() => handleJoinRoom("lobby")}>
                      🏠 Join Public Lobby
                    </button>
                    <button class="btn-secondary hero-btn" onclick={() => (mobileTab = "contacts")}>
                      👥 Choose a Contact
                    </button>
                  </div>
                </div>
              </div>
            {/if}
          </div>
        </div>

      {:else if mobileTab === "contacts"}
        <div class="mobile-tab-content">
          <div class="mobile-screen-scrollable full-height">
            <Sidebar
              {profile}
              {contacts}
              {roomState}
              bind:activeChatId
              bind:isMuted
              bind:isDeafened
              onOpenSettings={() => openSettings("audio")}
              onJoinRoom={(roomId) => {
                handleJoinRoom(roomId);
                mobileTab = "call";
              }}
              onLeaveRoom={handleLeaveRoom}
              onAddContact={handleAddContact}
              onDirectCall={(nid) => {
                handleDirectCall(nid);
                mobileTab = "call";
              }}
            />
          </div>
        </div>

      {:else if mobileTab === "settings"}
        <div class="mobile-tab-content">
          <div class="mobile-header-bar">
            <span class="mobile-chat-title">⚙️ Settings & Device Sync</span>
          </div>

          <div class="mobile-screen-scrollable mobile-settings-padding">
            <!-- QR Sync Hero Card -->
            <div class="mobile-hero-qr-card">
              <div class="hero-qr-badge">📷 QR SYNC</div>
              <h4>Link Mobile Device</h4>
              <p>Scan a QR code from your desktop PC to instantly clone your account, contacts, and chat history.</p>
              <button class="btn-primary hero-qr-action-btn" onclick={() => openSettings("sync")}>
                📷 Scan QR or Generate Ticket
              </button>
            </div>

            <div class="mobile-settings-nav-group">
              <button class="mobile-settings-row-btn" onclick={() => openSettings("audio")}>
                <span class="settings-row-icon">🎙️</span>
                <div class="settings-row-text">
                  <span class="settings-row-title">Voice & Audio</span>
                  <span class="settings-row-desc">Microphone gain, RNNoise AI suppression & sensitivity</span>
                </div>
                <span class="settings-row-chevron">›</span>
              </button>

              <button class="mobile-settings-row-btn" onclick={() => openSettings("theme")}>
                <span class="settings-row-icon">🎨</span>
                <div class="settings-row-text">
                  <span class="settings-row-title">Theme Studio</span>
                  <span class="settings-row-desc">Dark Minimal, Cyberpunk, AMOLED, custom palette</span>
                </div>
                <span class="settings-row-chevron">›</span>
              </button>

              <button class="mobile-settings-row-btn" onclick={() => openSettings("network")}>
                <span class="settings-row-icon">🌐</span>
                <div class="settings-row-text">
                  <span class="settings-row-title">Network & VPN Bypass</span>
                  <span class="settings-row-desc">Direct binding, P2P NAT relay, serverless status</span>
                </div>
                <span class="settings-row-chevron">›</span>
              </button>
            </div>

            <!-- Profile Overview Card -->
            <div class="mobile-account-summary">
              <div class="account-summary-top">
                <span class="account-badge">My Account</span>
                <span class="online-tag">● Connected</span>
              </div>
              <div class="account-summary-profile">
                <Avatar hash={profile?.avatar_hash} nickname={profile?.nickname} size={48} />
                <div class="account-summary-info">
                  <div class="account-nickname">{profile?.nickname || "VoxP2P Peer"}</div>
                  <div class="account-node-id">{profile?.node_id || ""}</div>
                </div>
              </div>
            </div>
          </div>
        </div>
      {/if}

      <!-- Bottom Navigation Bar (4 tabs) -->
      <nav class="mobile-bottom-nav" aria-label="Mobile Navigation">
        <button
          class="nav-tab-btn"
          class:active={mobileTab === "chats"}
          onclick={() => (mobileTab = "chats")}
          aria-label="Chats"
        >
          <span class="nav-tab-icon">💬</span>
          <span class="nav-tab-label">Chats</span>
        </button>

        <button
          class="nav-tab-btn"
          class:active={mobileTab === "call"}
          onclick={() => (mobileTab = "call")}
          aria-label="Active Call"
        >
          <div class="nav-icon-container">
            <span class="nav-tab-icon">🔊</span>
            {#if roomState}
              <span class="live-call-dot pulse"></span>
            {/if}
          </div>
          <span class="nav-tab-label">Call</span>
        </button>

        <button
          class="nav-tab-btn"
          class:active={mobileTab === "contacts"}
          onclick={() => (mobileTab = "contacts")}
          aria-label="Contacts"
        >
          <div class="nav-icon-container">
            <span class="nav-tab-icon">👥</span>
            {#if contacts.length > 0}
              <span class="contacts-count-bubble">{contacts.length}</span>
            {/if}
          </div>
          <span class="nav-tab-label">Contacts</span>
        </button>

        <button
          class="nav-tab-btn"
          class:active={mobileTab === "settings"}
          onclick={() => (mobileTab = "settings")}
          aria-label="Settings"
        >
          <span class="nav-tab-icon">⚙️</span>
          <span class="nav-tab-label">Settings</span>
        </button>
      </nav>
    </div>
  {/if}

  <!-- Settings Modal (Voice, Network, Theme Studio, Device Linking) -->
  {#if showSettingsModal}
    <SettingsModal
      bind:currentTheme
      initialTab={settingsInitialTab}
      onClose={() => (showSettingsModal = false)}
      onSyncCompleted={() => {
        api.getMyProfile().then((p) => {
          profile = p;
        }).catch(console.error);
        loadContacts();
        if (activeChatId) {
          api.getMessages(activeChatId, 50).then((res) => (messages = res));
        }
      }}
    />
  {/if}
  <!-- Incoming 1-on-1 Call Overlay Modal -->
  {#if incomingCall}
    {@const callerContact = contacts.find((c) => c.node_id === incomingCall?.caller_id)}
    <div class="incoming-call-overlay" role="dialog" aria-modal="true" tabindex="-1">
      <div class="incoming-call-card">
        <div class="call-pulse-animation">
          <Avatar
            hash={callerContact?.avatar_path}
            nickname={callerContact?.nickname || incomingCall?.caller_id || "Peer"}
            size={68}
          />
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

  /* Mobile Navigation & Screen Styles */
  .mobile-main-container {
    display: flex;
    flex-direction: column;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    position: relative;
    background: var(--bg-primary);
  }

  .mobile-tab-content {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    overflow: hidden;
    padding-bottom: 60px;
    box-sizing: border-box;
  }

  .mobile-screen-scrollable {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    overflow-x: hidden;
    -webkit-overflow-scrolling: touch;
  }

  .mobile-screen-scrollable.full-height {
    height: 100%;
  }

  .mobile-header-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    background: var(--bg-secondary);
    border-bottom: var(--border-width) solid var(--border-color);
    flex-shrink: 0;
    min-height: 54px;
    box-sizing: border-box;
  }

  .mobile-header-left {
    display: flex;
    align-items: center;
    gap: 10px;
    overflow: hidden;
  }

  .mobile-header-title-box {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .mobile-chat-title {
    font-weight: 700;
    font-size: 0.95rem;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mobile-chat-sub {
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  .mobile-header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .mobile-touch-btn {
    background: var(--bg-tertiary);
    border: var(--border-width) solid var(--border-color);
    color: var(--text-primary);
    font-size: 1.15rem;
    min-width: 44px;
    min-height: 44px;
    width: 44px;
    height: 44px;
    border-radius: var(--border-radius);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .mobile-touch-btn:active {
    background: var(--accent-primary);
    color: #ffffff;
  }

  .mobile-clickable {
    cursor: pointer;
  }

  /* Bottom Navigation Bar (4 tabs) */
  .mobile-bottom-nav {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    height: 60px;
    background: var(--bg-secondary);
    border-top: var(--border-width) solid var(--border-color);
    display: flex;
    justify-content: space-around;
    align-items: center;
    z-index: 100;
    box-shadow: 0 -2px 14px rgba(0, 0, 0, 0.45);
  }

  .nav-tab-btn {
    flex: 1;
    height: 100%;
    min-height: 44px;
    min-width: 44px;
    background: transparent;
    border: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
    user-select: none;
    -webkit-tap-highlight-color: transparent;
  }

  .nav-tab-btn.active {
    color: var(--accent-primary);
  }

  .nav-tab-btn.active .nav-tab-icon {
    transform: scale(1.1);
  }

  .nav-tab-icon {
    font-size: 1.25rem;
    line-height: 1;
    transition: transform 0.15s ease;
  }

  .nav-tab-label {
    font-size: 0.72rem;
    font-weight: 600;
    line-height: 1;
  }

  .nav-icon-container {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .live-call-dot {
    position: absolute;
    top: -2px;
    right: -6px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--voice-speaking-ring);
    box-shadow: 0 0 6px var(--voice-speaking-ring);
  }

  .contacts-count-bubble {
    position: absolute;
    top: -4px;
    right: -10px;
    background: var(--accent-primary);
    color: #ffffff;
    font-size: 0.65rem;
    font-weight: 700;
    padding: 1px 5px;
    border-radius: 8px;
    line-height: 1.2;
  }

  /* Mobile Idle Call Screen */
  .mobile-idle-screen {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    padding: 24px;
  }

  .idle-card-box {
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 28px 20px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 12px;
    max-width: 340px;
    width: 100%;
  }

  .idle-big-icon {
    font-size: 3rem;
  }

  .idle-card-box h3 {
    margin: 0;
    font-size: 1.15rem;
    color: var(--text-primary);
  }

  .idle-card-box p {
    margin: 0;
    font-size: 0.85rem;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .idle-actions-col {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
    margin-top: 8px;
  }

  .hero-btn {
    min-height: 46px;
    width: 100%;
    font-size: 0.9rem;
    font-weight: 600;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--border-radius);
    cursor: pointer;
  }

  /* Mobile Settings Hub */
  .mobile-settings-padding {
    padding: 16px;
    gap: 16px;
    display: flex;
    flex-direction: column;
  }

  .mobile-hero-qr-card {
    background: var(--surface-card);
    border: var(--border-width) solid var(--accent-primary);
    border-radius: var(--border-radius);
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
  }

  .hero-qr-badge {
    font-size: 0.72rem;
    font-weight: 700;
    color: var(--accent-primary);
    letter-spacing: 0.05em;
  }

  .mobile-hero-qr-card h4 {
    margin: 0;
    font-size: 1.05rem;
    color: var(--text-primary);
  }

  .mobile-hero-qr-card p {
    margin: 0;
    font-size: 0.82rem;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .hero-qr-action-btn {
    min-height: 46px;
    width: 100%;
    font-size: 0.92rem;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--border-radius);
    cursor: pointer;
    border: none;
    background: var(--accent-primary);
    color: #ffffff;
  }

  .hero-qr-action-btn:hover {
    background: var(--accent-hover);
  }

  .mobile-settings-nav-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .mobile-settings-row-btn {
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 12px 14px;
    min-height: 52px;
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
    text-align: left;
    transition: background 0.15s;
    color: var(--text-primary);
  }

  .mobile-settings-row-btn:active {
    background: var(--bg-tertiary);
  }

  .settings-row-icon {
    font-size: 1.35rem;
    flex-shrink: 0;
  }

  .settings-row-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .settings-row-title {
    font-weight: 600;
    font-size: 0.88rem;
    color: var(--text-primary);
  }

  .settings-row-desc {
    font-size: 0.74rem;
    color: var(--text-muted);
  }

  .settings-row-chevron {
    font-size: 1.25rem;
    color: var(--text-muted);
  }

  .mobile-account-summary {
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .account-summary-profile {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 2px;
  }

  .account-summary-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
  }

  .account-summary-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.75rem;
  }

  .account-badge {
    font-weight: 700;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .online-tag {
    color: var(--online-status-color);
    font-weight: 600;
  }

  .account-nickname {
    font-weight: 700;
    font-size: 1rem;
    color: var(--text-primary);
  }

  .account-node-id {
    font-size: 0.72rem;
    color: var(--text-muted);
    font-family: monospace;
    word-break: break-all;
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
    min-height: 48px;
    min-width: 48px;
    padding: 12px 18px;
    border-radius: var(--border-radius);
    font-size: 1rem;
    font-weight: 600;
    cursor: pointer;
    border: none;
    transition: all 0.15s ease;
    display: flex;
    align-items: center;
    justify-content: center;
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
