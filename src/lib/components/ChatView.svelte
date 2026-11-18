<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import type { Message, UserProfile, TransferProgressPayload, Contact, ContextMenuItem } from "../types";
  import { api } from "../api";
  import { detectThemeInText, applyTheme, type ThemeConfig } from "../theme";
  import Avatar from "./Avatar.svelte";
  import ContextMenu from "./ContextMenu.svelte";

  let {
    activeChatId,
    profile,
    contacts = [],
    messages,
    onSendMessage,
    onThemeApplied,
    onJoinRoom,
    onDirectCall,
  }: {
    activeChatId: string;
    profile: UserProfile | null;
    contacts?: Contact[];
    messages: Message[];
    onSendMessage: (text: string) => void;
    onThemeApplied: (theme: ThemeConfig) => void;
    onJoinRoom?: (roomIdOrTicket: string) => void;
    onDirectCall?: (nodeId: string) => void;
  } = $props();

  let messageInput = $state("");
  let messagesContainer = $state<HTMLDivElement | null>(null);
  let isDragging = $state(false);
  let transferProgressMap = $state<Record<string, TransferProgressPayload>>({});
  let downloadedHashes = $state<Record<string, boolean>>({});

  // Context Menu State
  let contextMenuState = $state<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);

  // Message Editing State
  let editingMessageId = $state<string | null>(null);
  let editingContent = $state<string>("");

  // Forward Modal State
  let forwardingMessage = $state<Message | null>(null);
  let isForwarding = $state<boolean>(false);
  let forwardFeedback = $state<string | null>(null);

  // Input Textarea Reference
  let textareaEl = $state<HTMLTextAreaElement | null>(null);

  function extractTicket(text: string): string | null {
    if (!text) return null;
    const match = text.match(/(ticket_[A-Za-z0-9_-]+)/);
    if (match) return match[1];
    return null;
  }

  $effect(() => {
    // Listen for file transfer progress
    let unlisten: (() => void) | undefined;
    api.onTransferProgress((payload) => {
      transferProgressMap[payload.hash] = payload;
      if (payload.is_complete) {
        downloadedHashes[payload.hash] = true;
      }
    }).then((u) => {
      unlisten = u;
    });

    // Native Tauri v2 window drag-and-drop listener
    let unlistenNativeDragDrop: (() => void) | undefined;
    try {
      getCurrentWebview().onDragDropEvent(async (event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          isDragging = true;
        } else if (event.payload.type === "leave") {
          isDragging = false;
        } else if (event.payload.type === "drop") {
          isDragging = false;
          const paths = event.payload.paths;
          if (paths && paths.length > 0) {
            for (const filePath of paths) {
              const filename = filePath.split(/[/\\]/).pop() || filePath;
              try {
                await api.attachFile(filePath, activeChatId, `Shared file: ${filename}`);
              } catch (err) {
                console.error("Failed to upload dropped file via native drag-drop:", err);
              }
            }
          }
        }
      }).then((u) => {
        unlistenNativeDragDrop = u;
      }).catch(console.warn);
    } catch (e) {
      console.warn("Native drag-drop event registration skipped:", e);
    }

    return () => {
      unlisten?.();
      unlistenNativeDragDrop?.();
    };
  });

  $effect(() => {
    // Scroll to bottom when new messages arrive
    if (messages.length && messagesContainer) {
      setTimeout(() => {
        if (messagesContainer) {
          messagesContainer.scrollTop = messagesContainer.scrollHeight;
        }
      }, 50);
    }
  });

  function handleSend() {
    if (!messageInput.trim()) return;
    onSendMessage(messageInput.trim());
    messageInput = "";
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  // Mobile touch long-press context menu
  let touchTimer: any = null;
  let touchStartPos = { x: 0, y: 0 };
  let touchMoved = false;

  function handleMessageTouchStart(e: TouchEvent, msg: Message) {
    if (e.touches.length !== 1) return;
    touchMoved = false;
    const touch = e.touches[0];
    touchStartPos = { x: touch.clientX, y: touch.clientY };

    if (touchTimer) clearTimeout(touchTimer);
    touchTimer = setTimeout(() => {
      touchTimer = null;
      if (!touchMoved) {
        const syntheticEvent = {
          clientX: touchStartPos.x,
          clientY: touchStartPos.y,
          preventDefault: () => {},
          stopPropagation: () => {},
        } as unknown as MouseEvent;
        handleMessageContextMenu(syntheticEvent, msg);
        if (typeof navigator !== "undefined" && navigator.vibrate) {
          navigator.vibrate(40);
        }
      }
    }, 450);
  }

  function handleMessageTouchMove(e: TouchEvent) {
    if (!touchTimer) return;
    const touch = e.touches[0];
    const dx = Math.abs(touch.clientX - touchStartPos.x);
    const dy = Math.abs(touch.clientY - touchStartPos.y);
    if (dx > 10 || dy > 10) {
      touchMoved = true;
      clearTimeout(touchTimer);
      touchTimer = null;
    }
  }

  function handleMessageTouchEnd() {
    if (touchTimer) {
      clearTimeout(touchTimer);
      touchTimer = null;
    }
  }

  function handleMessageContextMenu(e: MouseEvent, msg: Message) {
    e.preventDefault();
    e.stopPropagation();

    const isMe = msg.sender_id === profile?.node_id;
    const items: ContextMenuItem[] = [];

    // 1. Copy Text (if message has content)
    if (msg.content) {
      items.push({
        label: "Copy Text",
        icon: "📋",
        action: async () => {
          try {
            await navigator.clipboard.writeText(msg.content);
          } catch (err) {
            console.error("Failed to copy message text:", err);
          }
        },
      });
    }

    // 2. Forward to Contact...
    items.push({
      label: "Forward to Contact...",
      icon: "↗️",
      action: () => {
        forwardingMessage = msg;
        forwardFeedback = null;
      },
    });

    // 3. Edit Message (visible only if sender is me and not a file attachment)
    if (isMe && !msg.attachment_hash) {
      items.push({
        label: "Edit Message",
        icon: "✏️",
        action: () => {
          editingMessageId = msg.id;
          editingContent = msg.content;
        },
      });
    }

    items.push({
      separator: true,
      label: "",
      action: () => {},
    });

    // 4. Delete Message
    if (!isMe) {
      items.push({
        label: "Delete Message",
        icon: "🗑️",
        danger: true,
        action: async () => {
          try {
            await api.deleteMessage(msg.id, false);
          } catch (err) {
            console.error("Failed to delete message:", err);
          }
        },
      });
    } else {
      items.push({
        label: "Delete for me",
        icon: "🗑️",
        danger: true,
        action: async () => {
          try {
            await api.deleteMessage(msg.id, false);
          } catch (err) {
            console.error("Failed to delete message for me:", err);
          }
        },
      });
      items.push({
        label: "Delete for everyone",
        icon: "💥",
        danger: true,
        action: async () => {
          try {
            await api.deleteMessage(msg.id, true);
          } catch (err) {
            console.error("Failed to delete message for everyone:", err);
          }
        },
      });
    }

    contextMenuState = {
      x: e.clientX,
      y: e.clientY,
      items,
    };
  }

  function handleInputContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();

    const start = textareaEl?.selectionStart ?? 0;
    const end = textareaEl?.selectionEnd ?? 0;
    const hasSelection = start !== end;

    const items: ContextMenuItem[] = [
      {
        label: "Cut",
        icon: "✂️",
        disabled: !hasSelection,
        action: async () => {
          if (!hasSelection || !textareaEl) return;
          const sel = messageInput.slice(start, end);
          try {
            await navigator.clipboard.writeText(sel);
          } catch (err) {
            console.error("Failed to write to clipboard:", err);
          }
          messageInput = messageInput.slice(0, start) + messageInput.slice(end);
          setTimeout(() => {
            textareaEl?.setSelectionRange(start, start);
            textareaEl?.focus();
          }, 0);
        },
      },
      {
        label: "Copy",
        icon: "📋",
        disabled: !hasSelection,
        action: async () => {
          if (!hasSelection) return;
          const sel = messageInput.slice(start, end);
          try {
            await navigator.clipboard.writeText(sel);
          } catch (err) {
            console.error("Failed to copy to clipboard:", err);
          }
        },
      },
      {
        label: "Paste",
        icon: "📥",
        action: async () => {
          try {
            const clip = await navigator.clipboard.readText();
            if (!clip) return;
            messageInput = messageInput.slice(0, start) + clip + messageInput.slice(end);
            setTimeout(() => {
              const newPos = start + clip.length;
              textareaEl?.setSelectionRange(newPos, newPos);
              textareaEl?.focus();
            }, 0);
          } catch (err) {
            console.error("Failed to paste from clipboard:", err);
          }
        },
      },
    ];

    contextMenuState = {
      x: e.clientX,
      y: e.clientY,
      items,
    };
  }

  async function handleSaveEdit(msg: Message) {
    if (!editingContent.trim()) return;
    try {
      await api.editMessage(msg.id, editingContent.trim());
      editingMessageId = null;
    } catch (err) {
      console.error("Failed to save edited message:", err);
    }
  }

  function handleCancelEdit() {
    editingMessageId = null;
    editingContent = "";
  }

  async function handleForwardTo(contact: Contact) {
    if (!forwardingMessage) return;
    isForwarding = true;
    forwardFeedback = null;
    try {
      await api.sendMessage(
        contact.node_id,
        forwardingMessage.content,
        forwardingMessage.attachment_hash,
        forwardingMessage.attachment_name
      );
      forwardFeedback = `✓ Forwarded to ${contact.nickname}!`;
      setTimeout(() => {
        forwardingMessage = null;
        forwardFeedback = null;
        isForwarding = false;
      }, 900);
    } catch (err: any) {
      forwardFeedback = `Error: ${err?.toString() || "Failed to forward"}`;
      isForwarding = false;
    }
  }

  function renderMarkdown(raw: string): string {
    if (!raw) return "";
    let escaped = raw
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");

    // Code blocks
    escaped = escaped.replace(/```([\s\S]*?)```/g, "<pre><code>$1</code></pre>");
    // Inline code
    escaped = escaped.replace(/`([^`]+)`/g, "<code>$1</code>");
    // Bold
    escaped = escaped.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
    // Italic
    escaped = escaped.replace(/\*([^*]+)\*/g, "<em>$1</em>");
    // Links
    escaped = escaped.replace(
      /(https?:\/\/[^\s]+)/g,
      '<a href="$1" target="_blank" rel="noopener noreferrer">$1</a>'
    );
    // Line breaks
    escaped = escaped.replace(/\n/g, "<br/>");
    return escaped;
  }

  function isImageAttachment(name?: string | null): boolean {
    if (!name) return false;
    const ext = name.toLowerCase();
    return (
      ext.endsWith(".png") ||
      ext.endsWith(".jpg") ||
      ext.endsWith(".jpeg") ||
      ext.endsWith(".gif") ||
      ext.endsWith(".webp")
    );
  }

  async function handleDownload(msg: Message) {
    if (!msg.attachment_hash) return;
    const savePath = `./downloads/${msg.attachment_name || msg.attachment_hash}`;
    try {
      await api.downloadAttachment(msg.sender_id, msg.attachment_hash, savePath);
    } catch (err) {
      console.error("Failed to download attachment:", err);
    }
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragLeave() {
    isDragging = false;
  }

  async function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    if (e.dataTransfer?.files?.length) {
      const file = e.dataTransfer.files[0];
      // In Tauri, web File objects might have a .path property or we can attach via file path
      const filePath = (file as any).path || file.name;
      try {
        await api.attachFile(filePath, activeChatId, `Shared file: ${file.name}`);
      } catch (err) {
        console.error("Failed to upload dropped file:", err);
      }
    }
  }

  async function handleAttachButtonClick() {
    const input = document.createElement("input");
    input.type = "file";
    input.onchange = async () => {
      if (input.files?.length) {
        const file = input.files[0];
        const filePath = (file as any).path || file.name;
        try {
          await api.attachFile(filePath, activeChatId, `Shared file: ${file.name}`);
        } catch (err) {
          console.error("Failed to attach file:", err);
        }
      }
    };
    input.click();
  }

  function applyThemeFromMessage(theme: ThemeConfig) {
    applyTheme(theme);
    onThemeApplied(theme);
  }
</script>

<div
  class="chat-view-container"
  class:dragging={isDragging}
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
  role="region"
  aria-label="Chat messages feed"
>
  {#if isDragging}
    <div class="drag-overlay">
      <div class="drag-box">
        <span class="drag-icon">📁</span>
        <span class="drag-title">Drop files to send via P2P</span>
        <span class="drag-desc">Instant zero-server transfer via iroh-blobs</span>
      </div>
    </div>
  {/if}

  <!-- Header -->
  <div class="chat-header">
    <div class="chat-title-row">
      <span class="chat-hash-icon">💬</span>
      <span class="chat-title">
        {activeChatId.startsWith("room_") || activeChatId.length < 32
          ? `Room: ${activeChatId}`
          : `Direct: ${activeChatId.slice(0, 10)}...${activeChatId.slice(-6)}`}
      </span>
    </div>
    <div class="chat-header-actions">
      {#if activeChatId !== "lobby" && !activeChatId.startsWith("room_") && !activeChatId.startsWith("call_") && onDirectCall}
        <button
          class="direct-call-action-btn"
          title="Start 1-on-1 Voice Call"
          onclick={() => onDirectCall?.(activeChatId)}
        >
          📞 Call
        </button>
      {/if}
      <span class="chat-encryption-badge">P2P Encrypted</span>
    </div>
  </div>

  <!-- Messages Scroll Area -->
  <div class="messages-feed" bind:this={messagesContainer}>
    {#if messages.length === 0}
      <div class="empty-chat">
        <span class="empty-icon">🗨️</span>
        <p>No messages yet. Send a message, share a theme, or drop a file to transfer via P2P.</p>
      </div>
    {:else}
      {#each messages as msg}
        {@const detectedTheme = detectThemeInText(msg.content)}
        {@const callTicket = extractTicket(msg.content)}
        {@const senderContact = contacts.find((c) => c.node_id === msg.sender_id)}
        {@const isMe = msg.sender_id === profile?.node_id}
        {@const authorAvatarHash = isMe ? profile?.avatar_hash : senderContact?.avatar_path}
        {@const authorNickname = isMe ? (profile?.nickname || "You") : (senderContact?.nickname || msg.sender_id.slice(0, 4))}
        <div
          class="message-card"
          class:my-message={isMe}
          role="article"
          oncontextmenu={(e) => handleMessageContextMenu(e, msg)}
          ontouchstart={(e) => handleMessageTouchStart(e, msg)}
          ontouchmove={handleMessageTouchMove}
          ontouchend={handleMessageTouchEnd}
          ontouchcancel={handleMessageTouchEnd}
        >
          <div class="message-avatar-wrap">
            <Avatar
              hash={authorAvatarHash}
              nickname={authorNickname}
              size={36}
            />
          </div>

          <div class="message-body">
            <div class="message-meta">
              <span class="message-sender">
                {msg.sender_id === profile?.node_id
                  ? `${profile?.nickname || "You"}`
                  : `${msg.sender_id.slice(0, 8)}...`}
              </span>
              <span class="message-time">
                {new Date(msg.timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
              </span>
              {#if msg.sender_id === profile?.node_id}
                <span class="message-status" title={msg.delivered ? "Delivered" : "Queued offline"}>
                  {msg.delivered ? "✓✓" : "🕒"}
                </span>
              {/if}
            </div>

            <!-- Content -->
            {#if editingMessageId === msg.id}
              <div class="inline-edit-box">
                <textarea
                  class="edit-textarea"
                  bind:value={editingContent}
                  onkeydown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      handleSaveEdit(msg);
                    } else if (e.key === "Escape") {
                      e.preventDefault();
                      handleCancelEdit();
                    }
                  }}
                  rows="2"
                ></textarea>
                <div class="edit-btn-row">
                  <button class="btn-cancel-edit" onclick={handleCancelEdit}>
                    Cancel
                  </button>
                  <button class="btn-save-edit" onclick={() => handleSaveEdit(msg)}>
                    Save
                  </button>
                </div>
              </div>
            {:else if msg.content}
              <div class="message-text">
                <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                {@html renderMarkdown(msg.content)}
                {#if msg.is_edited}
                  <span class="edited-badge" title="Edited message">(edited)</span>
                {/if}
              </div>
            {/if}

            <!-- Apply Theme Banner if Theme JSON detected -->
            {#if detectedTheme}
              <div class="theme-detected-card">
                <div class="theme-preview-chip" style="background: {detectedTheme.bgPrimary}; border: 1px solid {detectedTheme.accentPrimary};">
                  <span style="color: {detectedTheme.textPrimary};">🎨 {detectedTheme.name}</span>
                </div>
                <button class="apply-theme-btn" onclick={() => applyThemeFromMessage(detectedTheme!)}>
                  Apply Theme
                </button>
              </div>
            {/if}

            <!-- Voice Call Invitation Card if ticket detected in message -->
            {#if callTicket}
              <div class="call-invite-banner">
                <div class="invite-info">
                  <span class="invite-icon">📞</span>
                  <div class="invite-text-wrap">
                    <span class="invite-title">Voice Call Invitation</span>
                    <span class="invite-sub">P2P Encrypted Mesh Room</span>
                  </div>
                </div>
                {#if onJoinRoom}
                  <button class="join-call-btn" onclick={() => onJoinRoom?.(callTicket)}>
                    Join Call
                  </button>
                {/if}
              </div>
            {/if}

            <!-- Attachment Card / Image Preview -->
            {#if msg.attachment_hash}
              <div class="attachment-box">
                {#if isImageAttachment(msg.attachment_name)}
                  <div class="image-preview-card">
                    <img
                      src={`./downloads/${msg.attachment_name}`}
                      alt={msg.attachment_name || "Attachment"}
                      onerror={(e) => {
                        // If local preview not yet downloaded, show placeholder card
                        (e.target as HTMLElement).style.display = "none";
                      }}
                    />
                  </div>
                {/if}

                <div class="file-card">
                  <div class="file-icon">📄</div>
                  <div class="file-info">
                    <span class="file-name">{msg.attachment_name || "attachment.bin"}</span>
                    <span class="file-hash">{msg.attachment_hash.slice(0, 16)}...</span>
                  </div>

                  {#if transferProgressMap[msg.attachment_hash] && !transferProgressMap[msg.attachment_hash].is_complete}
                    <div class="transfer-progress-wrap">
                      <div
                        class="transfer-bar"
                        style="width: {transferProgressMap[msg.attachment_hash].percent}%"
                      ></div>
                    </div>
                  {:else}
                    <button
                      class="download-btn"
                      class:downloaded={downloadedHashes[msg.attachment_hash]}
                      onclick={() => handleDownload(msg)}
                    >
                      {downloadedHashes[msg.attachment_hash] ? "✓ Saved" : "Download"}
                    </button>
                  {/if}
                </div>
              </div>
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>

  <!-- Input Bar -->
  <div class="chat-input-area">
    <button class="attach-btn" title="Attach File (iroh-blobs)" onclick={handleAttachButtonClick}>
      📎
    </button>
    <textarea
      bind:this={textareaEl}
      class="chat-textarea"
      placeholder={`Message ${activeChatId.startsWith("room_") ? activeChatId : "peer"}...`}
      bind:value={messageInput}
      onkeydown={handleKeydown}
      oncontextmenu={handleInputContextMenu}
      rows="1"
    ></textarea>
    <button class="send-btn" onclick={handleSend} disabled={!messageInput.trim()}>
      ➤
    </button>
  </div>
</div>

{#if contextMenuState}
  <ContextMenu
    x={contextMenuState.x}
    y={contextMenuState.y}
    items={contextMenuState.items}
    onClose={() => (contextMenuState = null)}
  />
{/if}

{#if forwardingMessage}
  <div
    class="forward-modal-backdrop"
    onclick={() => (forwardingMessage = null)}
    onkeydown={(e) => e.key === "Escape" && (forwardingMessage = null)}
    role="presentation"
  >
    <div
      class="forward-modal-card"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <div class="forward-modal-header">
        <h4>Forward Message</h4>
        <button class="forward-close-btn" onclick={() => (forwardingMessage = null)} aria-label="Close">&times;</button>
      </div>

      <div class="forward-message-preview">
        <span class="preview-label">Message:</span>
        <p class="preview-text">{forwardingMessage.content || forwardingMessage.attachment_name || "Attachment"}</p>
      </div>

      {#if forwardFeedback}
        <div class="forward-feedback-msg">{forwardFeedback}</div>
      {/if}

      <div class="forward-contacts-list">
        <span class="contacts-heading">Select Contact:</span>
        {#if contacts.length === 0}
          <div class="no-contacts-msg">No contacts available to forward to.</div>
        {:else}
          {#each contacts as contact}
            <button
              class="forward-contact-row"
              onclick={() => handleForwardTo(contact)}
              disabled={isForwarding}
            >
              <Avatar hash={contact.avatar_path} nickname={contact.nickname} size={32} />
              <div class="contact-info-col">
                <span class="contact-name">{contact.nickname}</span>
                <span class="contact-id">{contact.node_id.slice(0, 12)}...</span>
              </div>
              <span class="forward-action-label">Send ›</span>
            </button>
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .chat-view-container {
    flex: 1;
    display: flex;
    flex-direction: column;
    background-color: var(--bg-primary);
    overflow: hidden;
    position: relative;
  }

  .chat-view-container.dragging {
    outline: 2px dashed var(--accent-primary);
  }

  .drag-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.78);
    backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
    pointer-events: none;
  }

  .drag-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 32px 48px;
    background: var(--surface-card);
    border: 2px dashed var(--accent-primary);
    border-radius: var(--border-radius);
    box-shadow: 0 12px 36px rgba(0, 0, 0, 0.6);
  }

  .drag-icon {
    font-size: 3.2rem;
  }

  .drag-title {
    font-size: 1.15rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .drag-desc {
    font-size: 0.82rem;
    color: var(--text-muted);
  }

  .chat-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 18px;
    background: var(--bg-secondary);
    border-bottom: var(--border-width) solid var(--border-color);
  }

  .chat-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .chat-hash-icon {
    font-size: 1rem;
  }

  .chat-title {
    font-weight: 700;
    font-size: 0.92rem;
    color: var(--text-primary);
  }

  .chat-header-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .direct-call-action-btn {
    background: var(--bg-tertiary);
    border: var(--border-width) solid var(--border-color);
    color: var(--text-primary);
    padding: 4px 10px;
    border-radius: var(--border-radius);
    font-size: 0.78rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .direct-call-action-btn:hover {
    background: var(--accent-primary);
    color: #ffffff;
    border-color: var(--accent-primary);
  }

  .chat-encryption-badge {
    font-size: 0.72rem;
    color: var(--online-status-color);
    background: var(--bg-tertiary);
    padding: 3px 8px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
  }

  .messages-feed {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .empty-chat {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    text-align: center;
    gap: 10px;
  }

  .empty-icon {
    font-size: 2.2rem;
    opacity: 0.7;
  }

  .message-card {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 6px 10px;
    border-radius: var(--border-radius);
    transition: background-color 0.12s ease;
    cursor: default;
  }

  .message-card:hover {
    background-color: rgba(255, 255, 255, 0.03);
  }

  .edited-badge {
    font-size: 0.72rem;
    color: var(--text-muted);
    font-style: italic;
    margin-left: 6px;
    user-select: none;
  }

  .inline-edit-box {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 4px;
    width: 100%;
    max-width: 520px;
  }

  .edit-textarea {
    width: 100%;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--accent-primary);
    border-radius: var(--border-radius);
    color: var(--text-primary);
    font-size: 0.9rem;
    font-family: inherit;
    padding: 8px 10px;
    resize: vertical;
    min-height: 48px;
    box-sizing: border-box;
  }

  .edit-textarea:focus {
    outline: none;
  }

  .edit-btn-row {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-end;
  }

  .btn-save-edit {
    background: var(--accent-primary);
    border: none;
    border-radius: 4px;
    color: #ffffff;
    font-size: 0.78rem;
    font-weight: 600;
    padding: 5px 14px;
    cursor: pointer;
  }

  .btn-save-edit:hover {
    background: var(--accent-hover);
  }

  .btn-cancel-edit {
    background: transparent;
    border: var(--border-width) solid var(--border-color);
    border-radius: 4px;
    color: var(--text-muted);
    font-size: 0.78rem;
    padding: 5px 12px;
    cursor: pointer;
  }

  .btn-cancel-edit:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  /* Forward Message Modal */
  .forward-modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.65);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
    backdrop-filter: blur(2px);
  }

  .forward-modal-card {
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 20px;
    width: 90vw;
    max-width: 400px;
    box-shadow: 0 12px 36px rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .forward-modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .forward-modal-header h4 {
    margin: 0;
    font-size: 1.1rem;
    color: var(--text-primary);
  }

  .forward-close-btn {
    background: transparent;
    border: none;
    font-size: 1.3rem;
    color: var(--text-muted);
    cursor: pointer;
    line-height: 1;
  }

  .forward-close-btn:hover {
    color: var(--text-primary);
  }

  .forward-message-preview {
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: 6px;
    padding: 8px 12px;
  }

  .preview-label {
    font-size: 0.72rem;
    color: var(--text-muted);
    font-weight: 600;
    text-transform: uppercase;
  }

  .preview-text {
    margin: 4px 0 0 0;
    font-size: 0.85rem;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .forward-contacts-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 240px;
    overflow-y: auto;
  }

  .contacts-heading {
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--text-muted);
  }

  .no-contacts-msg {
    font-size: 0.85rem;
    color: var(--text-muted);
    padding: 12px;
    text-align: center;
  }

  .forward-contact-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    background: var(--bg-secondary);
    border: var(--border-width) solid transparent;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: all 0.12s ease;
  }

  .forward-contact-row:hover:not(:disabled) {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
  }

  .contact-info-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .contact-name {
    font-size: 0.88rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .contact-id {
    font-size: 0.72rem;
    color: var(--text-muted);
    font-family: monospace;
  }

  .forward-action-label {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--accent-primary);
  }

  .forward-feedback-msg {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--online-status-color, #10b981);
    text-align: center;
  }

  .message-avatar-wrap {
    flex-shrink: 0;
  }

  .message-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow: hidden;
  }

  .message-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.78rem;
  }

  .message-sender {
    font-weight: 600;
    color: var(--text-primary);
  }

  .message-time {
    color: var(--text-muted);
  }

  .message-status {
    color: var(--accent-primary);
    font-size: 0.72rem;
  }

  .message-text {
    color: var(--text-primary);
    font-size: 0.9rem;
    line-height: 1.45;
    word-break: break-word;
  }

  :global(.message-text pre) {
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 10px;
    overflow-x: auto;
    font-family: monospace;
    font-size: 0.82rem;
  }

  :global(.message-text code) {
    background: var(--bg-secondary);
    padding: 2px 6px;
    border-radius: 4px;
    font-family: monospace;
    font-size: 0.82rem;
  }

  :global(.message-text a) {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .theme-detected-card {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 6px;
    padding: 8px 12px;
    background: var(--surface-card);
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
    width: fit-content;
  }

  .theme-preview-chip {
    padding: 4px 10px;
    border-radius: 4px;
    font-size: 0.8rem;
    font-weight: 600;
  }

  .apply-theme-btn {
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    border-radius: var(--border-radius);
    padding: 5px 12px;
    font-size: 0.78rem;
    font-weight: 600;
    cursor: pointer;
  }

  .apply-theme-btn:hover {
    background: var(--accent-hover);
  }

  .call-invite-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-top: 8px;
    padding: 10px 14px;
    background: var(--surface-card);
    border: var(--border-width) solid var(--accent-primary);
    border-radius: var(--border-radius);
    width: fit-content;
    max-width: 100%;
  }

  .invite-info {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .invite-icon {
    font-size: 1.4rem;
  }

  .invite-text-wrap {
    display: flex;
    flex-direction: column;
  }

  .invite-title {
    font-weight: 600;
    font-size: 0.88rem;
    color: var(--text-primary);
  }

  .invite-sub {
    font-size: 0.74rem;
    color: var(--text-muted);
  }

  .join-call-btn {
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    border-radius: var(--border-radius);
    padding: 6px 14px;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .join-call-btn:hover {
    background: var(--accent-hover);
  }

  .attachment-box {
    margin-top: 6px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 400px;
  }

  .image-preview-card img {
    max-width: 100%;
    max-height: 240px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
    object-fit: cover;
  }

  .file-card {
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--surface-card);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 8px 12px;
  }

  .file-icon {
    font-size: 1.3rem;
  }

  .file-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .file-name {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .file-hash {
    font-size: 0.7rem;
    color: var(--text-muted);
    font-family: monospace;
  }

  .download-btn {
    background: var(--bg-tertiary);
    border: var(--border-width) solid var(--border-color);
    color: var(--text-primary);
    padding: 5px 12px;
    border-radius: var(--border-radius);
    font-size: 0.78rem;
    font-weight: 600;
    cursor: pointer;
  }

  .download-btn:hover {
    background: var(--accent-primary);
    color: #ffffff;
  }

  .download-btn.downloaded {
    background: var(--online-status-color);
    color: #ffffff;
  }

  .transfer-progress-wrap {
    width: 80px;
    height: 6px;
    background: var(--bg-primary);
    border-radius: 3px;
    overflow: hidden;
  }

  .transfer-bar {
    height: 100%;
    background: var(--accent-primary);
  }

  .chat-input-area {
    padding: 12px 18px;
    background: var(--bg-secondary);
    border-top: var(--border-width) solid var(--border-color);
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .attach-btn {
    background: transparent;
    border: none;
    font-size: 1.35rem;
    color: var(--text-muted);
    cursor: pointer;
    min-width: 44px;
    min-height: 44px;
    padding: 8px;
    border-radius: var(--border-radius);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.15s;
    flex-shrink: 0;
  }

  .attach-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .chat-textarea {
    flex: 1;
    background: var(--bg-primary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 10px 14px;
    color: var(--text-primary);
    font-size: 0.9rem;
    resize: none;
    font-family: inherit;
    line-height: 1.3;
    min-height: 44px;
  }

  .chat-textarea:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .send-btn {
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    width: 44px;
    height: 44px;
    min-width: 44px;
    min-height: 44px;
    border-radius: var(--border-radius);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    font-size: 1.1rem;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .send-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
