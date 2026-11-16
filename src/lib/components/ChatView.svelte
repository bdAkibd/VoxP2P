<script lang="ts">
  import type { Message, UserProfile, TransferProgressPayload } from "../types";
  import { api } from "../api";
  import { detectThemeInText, applyTheme, type ThemeConfig } from "../theme";

  let {
    activeChatId,
    profile,
    messages,
    onSendMessage,
    onThemeApplied,
  }: {
    activeChatId: string;
    profile: UserProfile | null;
    messages: Message[];
    onSendMessage: (text: string) => void;
    onThemeApplied: (theme: ThemeConfig) => void;
  } = $props();

  let messageInput = $state("");
  let messagesContainer = $state<HTMLDivElement | null>(null);
  let isDragging = $state(false);
  let transferProgressMap = $state<Record<string, TransferProgressPayload>>({});
  let downloadedHashes = $state<Record<string, boolean>>({});

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

    return () => {
      unlisten?.();
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
        <span>Drop file here to send via P2P iroh-blobs</span>
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
    <span class="chat-encryption-badge">P2P Encrypted</span>
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
        <div class="message-card" class:my-message={msg.sender_id === profile?.node_id}>
          <div class="message-avatar">
            {msg.sender_id === profile?.node_id
              ? profile?.nickname?.slice(0, 1).toUpperCase() || "Y"
              : msg.sender_id.slice(0, 1).toUpperCase()}
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
            {#if msg.content}
              <div class="message-text">
                <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                {@html renderMarkdown(msg.content)}
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
      class="chat-textarea"
      placeholder={`Message ${activeChatId.startsWith("room_") ? activeChatId : "peer"}...`}
      bind:value={messageInput}
      onkeydown={handleKeydown}
      rows="1"
    ></textarea>
    <button class="send-btn" onclick={handleSend} disabled={!messageInput.trim()}>
      ➤
    </button>
  </div>
</div>

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
    background: rgba(0, 0, 0, 0.75);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }

  .drag-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    color: #ffffff;
    font-size: 1.1rem;
    font-weight: 600;
  }

  .drag-icon {
    font-size: 3rem;
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
  }

  .message-avatar {
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    font-size: 0.85rem;
    flex-shrink: 0;
  }

  .message-card.my-message .message-avatar {
    background: var(--accent-primary);
    color: #ffffff;
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
    font-size: 1.25rem;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    transition: color 0.15s;
  }

  .attach-btn:hover {
    color: var(--text-primary);
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
  }

  .chat-textarea:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .send-btn {
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    width: 36px;
    height: 36px;
    border-radius: var(--border-radius);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    font-size: 0.95rem;
    transition: all 0.15s ease;
  }

  .send-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
