import { invoke } from "@tauri-apps/api/core";

export interface ThemeConfig {
  name: string;
  bgPrimary: string;
  bgSecondary: string;
  bgTertiary: string;
  surfaceCard: string;
  borderColor: string;
  borderWidth: number; // in px
  borderRadius: number; // in px
  textPrimary: string;
  textMuted: string;
  accentPrimary: string;
  accentHover: string;
  voiceSpeakingRing: string;
  dangerColor: string;
  onlineStatusColor: string;
}

export const PRESET_THEMES: Record<string, ThemeConfig> = {
  "Dark Minimal": {
    name: "Dark Minimal",
    bgPrimary: "#0f1115",
    bgSecondary: "#161920",
    bgTertiary: "#1f232d",
    surfaceCard: "#252a36",
    borderColor: "#2d3443",
    borderWidth: 1,
    borderRadius: 8,
    textPrimary: "#f1f3f7",
    textMuted: "#8b94a5",
    accentPrimary: "#4f46e5",
    accentHover: "#6366f1",
    voiceSpeakingRing: "#10b981",
    dangerColor: "#ef4444",
    onlineStatusColor: "#10b981",
  },
  "Cyberpunk Neon": {
    name: "Cyberpunk Neon",
    bgPrimary: "#070714",
    bgSecondary: "#0e0e24",
    bgTertiary: "#18183c",
    surfaceCard: "#1f1d47",
    borderColor: "#ff007f",
    borderWidth: 2,
    borderRadius: 4,
    textPrimary: "#00fff2",
    textMuted: "#a090cf",
    accentPrimary: "#ff007f",
    accentHover: "#ff3399",
    voiceSpeakingRing: "#00fff2",
    dangerColor: "#ff003c",
    onlineStatusColor: "#00ff73",
  },
  "Nord": {
    name: "Nord",
    bgPrimary: "#242933",
    bgSecondary: "#2e3440",
    bgTertiary: "#3b4252",
    surfaceCard: "#434c5e",
    borderColor: "#4c566a",
    borderWidth: 1,
    borderRadius: 10,
    textPrimary: "#eceff4",
    textMuted: "#d8dee9",
    accentPrimary: "#88c0d0",
    accentHover: "#81a1c1",
    voiceSpeakingRing: "#a3be8c",
    dangerColor: "#bf616a",
    onlineStatusColor: "#a3be8c",
  },
  "AMOLED Black": {
    name: "AMOLED Black",
    bgPrimary: "#000000",
    bgSecondary: "#080808",
    bgTertiary: "#121212",
    surfaceCard: "#181818",
    borderColor: "#282828",
    borderWidth: 1,
    borderRadius: 12,
    textPrimary: "#ffffff",
    textMuted: "#888888",
    accentPrimary: "#10b981",
    accentHover: "#34d399",
    voiceSpeakingRing: "#10b981",
    dangerColor: "#f43f5e",
    onlineStatusColor: "#10b981",
  },
};

export const DEFAULT_THEME: ThemeConfig = PRESET_THEMES["Dark Minimal"];

/**
 * Applies a ThemeConfig to document.documentElement CSS variables in real time.
 */
export function applyTheme(theme: ThemeConfig): void {
  if (typeof document === "undefined") return;

  const root = document.documentElement;
  root.style.setProperty("--bg-primary", theme.bgPrimary);
  root.style.setProperty("--bg-secondary", theme.bgSecondary);
  root.style.setProperty("--bg-tertiary", theme.bgTertiary);
  root.style.setProperty("--surface-card", theme.surfaceCard);
  root.style.setProperty("--border-color", theme.borderColor);
  root.style.setProperty("--border-width", `${theme.borderWidth}px`);
  root.style.setProperty("--border-radius", `${theme.borderRadius}px`);
  root.style.setProperty("--text-primary", theme.textPrimary);
  root.style.setProperty("--text-muted", theme.textMuted);
  root.style.setProperty("--accent-primary", theme.accentPrimary);
  root.style.setProperty("--accent-hover", theme.accentHover);
  root.style.setProperty("--voice-speaking-ring", theme.voiceSpeakingRing);
  root.style.setProperty("--danger-color", theme.dangerColor);
  root.style.setProperty("--online-status-color", theme.onlineStatusColor);

  try {
    localStorage.setItem("voxp2p_active_theme", JSON.stringify(theme));
  } catch {}
}

export function exportThemeToJson(theme: ThemeConfig): string {
  return JSON.stringify(
    {
      _voxp2p_theme: true,
      ...theme,
    },
    null,
    2
  );
}

export function importThemeFromJson(jsonStr: string): ThemeConfig | null {
  try {
    const parsed = JSON.parse(jsonStr);
    if (!parsed || typeof parsed !== "object") return null;

    // Validate key fields
    if (
      typeof parsed.bgPrimary !== "string" ||
      typeof parsed.bgSecondary !== "string" ||
      typeof parsed.textPrimary !== "string" ||
      typeof parsed.accentPrimary !== "string"
    ) {
      return null;
    }

    return {
      name: parsed.name || "Imported Theme",
      bgPrimary: parsed.bgPrimary,
      bgSecondary: parsed.bgSecondary,
      bgTertiary: parsed.bgTertiary || parsed.bgSecondary,
      surfaceCard: parsed.surfaceCard || parsed.bgSecondary,
      borderColor: parsed.borderColor || "#333333",
      borderWidth: typeof parsed.borderWidth === "number" ? parsed.borderWidth : 1,
      borderRadius: typeof parsed.borderRadius === "number" ? parsed.borderRadius : 8,
      textPrimary: parsed.textPrimary,
      textMuted: parsed.textMuted || "#888888",
      accentPrimary: parsed.accentPrimary,
      accentHover: parsed.accentHover || parsed.accentPrimary,
      voiceSpeakingRing: parsed.voiceSpeakingRing || "#10b981",
      dangerColor: parsed.dangerColor || "#ef4444",
      onlineStatusColor: parsed.onlineStatusColor || "#10b981",
    };
  } catch {
    return null;
  }
}

/**
 * Checks if a message body contains a shared VoxP2P ThemeConfig JSON payload.
 */
export function detectThemeInText(text: string): ThemeConfig | null {
  if (!text) return null;

  // Check code blocks or bare JSON containing _voxp2p_theme or theme keys
  const codeBlockMatch = text.match(/```(?:json)?\s*(\{[\s\S]*?\})\s*```/);
  if (codeBlockMatch && codeBlockMatch[1]) {
    const theme = importThemeFromJson(codeBlockMatch[1]);
    if (theme) return theme;
  }

  // Bare JSON check
  const jsonStart = text.indexOf("{");
  const jsonEnd = text.lastIndexOf("}");
  if (jsonStart !== -1 && jsonEnd > jsonStart) {
    const slice = text.substring(jsonStart, jsonEnd + 1);
    const theme = importThemeFromJson(slice);
    if (theme && (slice.includes("_voxp2p_theme") || slice.includes("voiceSpeakingRing"))) {
      return theme;
    }
  }

  return null;
}

/**
 * Persists a custom theme to `themes.json` using the backend Tauri command.
 */
export async function persistTheme(theme: ThemeConfig): Promise<void> {
  try {
    const themeJson = exportThemeToJson(theme);
    await invoke("save_theme", {
      themeName: theme.name,
      themeJson,
    });
  } catch (err) {
    console.warn("Failed to persist theme to themes.json:", err);
  }
}

/**
 * Loads all saved themes from backend `themes.json`.
 */
export async function loadSavedThemes(): Promise<Record<string, ThemeConfig>> {
  const result: Record<string, ThemeConfig> = { ...PRESET_THEMES };
  try {
    const raw = await invoke<string>("get_saved_themes");
    if (raw && raw !== "{}") {
      const parsed = JSON.parse(raw);
      for (const [name, val] of Object.entries(parsed)) {
        const theme = importThemeFromJson(JSON.stringify(val));
        if (theme) {
          result[name] = theme;
        }
      }
    }
  } catch (err) {
    console.warn("Failed to load themes from themes.json, using defaults:", err);
  }
  return result;
}

/**
 * Initializes the theme on application load.
 */
export function initTheme(): ThemeConfig {
  if (typeof window === "undefined") return DEFAULT_THEME;

  try {
    const saved = localStorage.getItem("voxp2p_active_theme");
    if (saved) {
      const parsed = importThemeFromJson(saved);
      if (parsed) {
        applyTheme(parsed);
        return parsed;
      }
    }
  } catch {}

  applyTheme(DEFAULT_THEME);
  return DEFAULT_THEME;
}
