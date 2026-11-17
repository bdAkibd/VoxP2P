<script lang="ts">
  import type { ContextMenuItem } from "../types";

  let {
    x,
    y,
    items,
    onClose,
  }: {
    x: number;
    y: number;
    items: ContextMenuItem[];
    onClose: () => void;
  } = $props();

  let menuEl = $state<HTMLDivElement | null>(null);
  let clampedPos = $state<{ x: number; y: number }>({ x: 0, y: 0 });

  $effect(() => {
    let finalX = x;
    let finalY = y;
    if (menuEl) {
      const rect = menuEl.getBoundingClientRect();
      const padding = 8;

      if (finalX + rect.width > window.innerWidth - padding) {
        finalX = Math.max(padding, window.innerWidth - rect.width - padding);
      }
      if (finalY + rect.height > window.innerHeight - padding) {
        finalY = Math.max(padding, window.innerHeight - rect.height - padding);
      }
    }
    clampedPos = { x: finalX, y: finalY };
  });

  $effect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };

    const handleScroll = () => {
      onClose();
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("scroll", handleScroll, true);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("scroll", handleScroll, true);
    };
  });

  function handleItemClick(item: ContextMenuItem, e: MouseEvent) {
    e.stopPropagation();
    if (item.disabled) return;
    item.action();
    onClose();
  }
</script>

<!-- Backdrop overlay to catch outside clicks and right-clicks -->
<div
  class="context-menu-backdrop"
  onclick={onClose}
  oncontextmenu={(e) => {
    e.preventDefault();
    onClose();
  }}
  role="presentation"
>
  <div
    bind:this={menuEl}
    class="context-menu"
    style="left: {clampedPos.x}px; top: {clampedPos.y}px;"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    oncontextmenu={(e) => e.preventDefault()}
    role="menu"
    tabindex="-1"
  >
    {#each items as item}
      {#if item.separator}
        <div class="menu-separator" role="separator"></div>
      {:else}
        <button
          class="menu-item"
          class:danger={item.danger}
          class:disabled={item.disabled}
          disabled={item.disabled}
          onclick={(e) => handleItemClick(item, e)}
          role="menuitem"
        >
          {#if item.icon}
            <span class="item-icon">{item.icon}</span>
          {/if}
          <span class="item-label">{item.label}</span>
        </button>
      {/if}
    {/each}
  </div>
</div>

<style>
  .context-menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 9999;
    background: transparent;
    user-select: none;
  }

  .context-menu {
    position: fixed;
    min-width: 175px;
    background: var(--surface-card, #252a36);
    border: var(--border-width, 1px) solid var(--border-color, #2d3443);
    border-radius: var(--border-radius, 8px);
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.45);
    animation: menu-pop 0.12s cubic-bezier(0.16, 1, 0.3, 1);
    transform-origin: top left;
  }

  @keyframes menu-pop {
    0% {
      opacity: 0;
      transform: scale(0.95);
    }
    100% {
      opacity: 1;
      transform: scale(1);
    }
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border: none;
    background: transparent;
    border-radius: 6px;
    color: var(--text-primary, #f1f3f7);
    font-size: 0.85rem;
    font-weight: 500;
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: background 0.1s ease, color 0.1s ease;
  }

  .menu-item:hover:not(:disabled) {
    background: var(--accent-primary, #4f46e5);
    color: #ffffff;
  }

  .menu-item.danger {
    color: var(--danger-color, #ef4444);
  }

  .menu-item.danger:hover:not(:disabled) {
    background: var(--danger-color, #ef4444);
    color: #ffffff;
  }

  .menu-item.disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .item-icon {
    font-size: 1rem;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    flex-shrink: 0;
  }

  .item-label {
    flex: 1;
    white-space: nowrap;
  }

  .menu-separator {
    height: 1px;
    background: var(--border-color, #2d3443);
    margin: 4px 6px;
  }
</style>
