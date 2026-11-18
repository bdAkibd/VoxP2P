<script module lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { api } from "../api";

  // Module-level reactive cache shared across all Avatar components
  const avatarCache = new Map<string, string>();
  let cacheEpoch = $state(0);

  export function cleanHash(val: string): string {
    return val.replace(/.*[/\\]/, "").replace(/\.png$/, "").trim();
  }

  // Global listener for background avatar downloads from P2P peers
  if (typeof window !== "undefined") {
    listen<string>("avatar://downloaded", async (event) => {
      const h = cleanHash(event.payload || "");
      if (h) {
        try {
          const url = await api.getAvatar(h);
          if (url) {
            avatarCache.set(h, url);
            cacheEpoch += 1;
          }
        } catch {}
      }
    }).catch(() => {});
  }
</script>

<script lang="ts">
  let {
    hash = null,
    src = null,
    nickname = null,
    size = 36,
    isSpeaking = false,
  }: {
    hash?: string | null;
    src?: string | null;
    nickname?: string | null;
    size?: number;
    isSpeaking?: boolean;
  } = $props();

  let hasImageError = $state(false);

  $effect(() => {
    if (hash) {
      const c = cleanHash(hash);
      if (c && !avatarCache.has(c)) {
        let attempts = 0;
        let timer: any = null;
        let cancelled = false;

        const tryFetch = () => {
          if (cancelled || avatarCache.has(c)) return;
          api
            .getAvatar(c)
            .then((url) => {
              if (cancelled) return;
              if (url) {
                avatarCache.set(c, url);
                cacheEpoch += 1;
              } else if (attempts < 5) {
                attempts += 1;
                const delay = Math.min(1000 * Math.pow(1.5, attempts), 5000);
                timer = setTimeout(tryFetch, delay);
              }
            })
            .catch(() => {});
        };

        tryFetch();

        return () => {
          cancelled = true;
          if (timer) clearTimeout(timer);
        };
      }
    }
  });

  $effect(() => {
    void src;
    void hash;
    hasImageError = false;
  });

  let resolvedUrl = $derived.by(() => {
    void cacheEpoch;
    if (src) return src;
    if (hash) {
      const c = cleanHash(hash);
      return avatarCache.get(c) || null;
    }
    return null;
  });

  let initialChar = $derived(
    nickname && nickname.trim().length > 0
      ? nickname.trim().slice(0, 1).toUpperCase()
      : "?"
  );
</script>

<div
  class="unified-avatar"
  class:speaking={isSpeaking}
  style="width: {size}px; height: {size}px; min-width: {size}px; min-height: {size}px;"
>
  {#if resolvedUrl && !hasImageError}
    <img
      class="avatar-image"
      src={resolvedUrl}
      alt={nickname ? `${nickname}'s avatar` : "Avatar"}
      onerror={() => (hasImageError = true)}
    />
  {:else}
    <div
      class="avatar-fallback"
      style="font-size: {Math.max(12, Math.round(size * 0.42))}px;"
    >
      {initialChar}
    </div>
  {/if}
</div>

<style>
  .unified-avatar {
    position: relative;
    border-radius: 50%;
    overflow: hidden;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary, #1f232b);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.08));
    user-select: none;
    flex-shrink: 0;
    transition: box-shadow 0.15s ease-out, border-color 0.15s ease-out;
  }

  .unified-avatar.speaking {
    border-color: var(--voice-speaking-ring, #10b981);
    box-shadow: 0 0 0 2px var(--voice-speaking-ring, #10b981),
      0 0 12px rgba(16, 185, 129, 0.45);
  }

  .avatar-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    border-radius: 50%;
  }

  .avatar-fallback {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    color: var(--text-primary, #f3f4f6);
    background: linear-gradient(
      135deg,
      var(--bg-tertiary, #2a2e39) 0%,
      var(--bg-secondary, #1a1d24) 100%
    );
    text-transform: uppercase;
  }
</style>
