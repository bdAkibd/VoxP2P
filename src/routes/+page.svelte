<script lang="ts">
  import { onMount } from "svelte";
  import type { UserProfile, Contact, RoomState, Message } from "$lib/types";
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
    let unlistenChat: (() => void) | undefined;

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

    api.onChatMessage((msg) => {
      if (msg.chat_id === activeChatId || msg.sender_id === activeChatId) {
        messages = [...messages, msg];
      }
    }).then((u) => (unlistenChat = u));

    return () => {
      unlistenLevels?.();
      unlistenHost?.();
      unlistenChat?.();
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

    <CallView {roomState} {profile} {audioLevels} />
    <ChatView
      {activeChatId}
      {profile}
      {messages}
      onSendMessage={handleSendMessage}
      onThemeApplied={handleThemeApplied}
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
</style>
