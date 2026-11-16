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

<main class="app-layout">
  <!-- Left: Sidebar -->
  <Sidebar
    {profile}
    {contacts}
    {roomState}
    bind:activeChatId
    bind:isMuted
    bind:isDeafened
    onOpenSettings={() => (showSettingsModal = true)}
    onJoinRoom={handleJoinRoom}
    onLeaveRoom={handleLeaveRoom}
    onAddContact={handleAddContact}
  />

  <!-- Right: Voice Call Top Panel + Chat Bottom Panel -->
  <section class="main-content-panel">
    <CallView {roomState} {profile} {audioLevels} />
    <ChatView
      {activeChatId}
      {profile}
      {messages}
      onSendMessage={handleSendMessage}
      onThemeApplied={handleThemeApplied}
    />
  </section>

  <!-- Settings Modal (Voice, Network, Theme Studio) -->
  {#if showSettingsModal}
    <SettingsModal
      bind:currentTheme
      onClose={() => (showSettingsModal = false)}
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
  }

  .main-content-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }
</style>
