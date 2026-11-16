<script lang="ts">
  import {
    type ThemeConfig,
    PRESET_THEMES,
    applyTheme,
    exportThemeToJson,
    importThemeFromJson,
    persistTheme,
    loadSavedThemes,
  } from "../theme";

  let { currentTheme = $bindable(), onClose }: { currentTheme: ThemeConfig; onClose: () => void } = $props();

  let activeTheme = $state<ThemeConfig>({ ...currentTheme });
  let savedThemes = $state<Record<string, ThemeConfig>>({ ...PRESET_THEMES });
  let importJsonText = $state("");
  let copyFeedback = $state(false);
  let saveFeedback = $state(false);
  let importError = $state<string | null>(null);

  $effect(() => {
    loadSavedThemes().then((themes) => {
      savedThemes = themes;
    });
  });

  function updateColor(key: keyof ThemeConfig, val: string | number) {
    (activeTheme as any)[key] = val;
    currentTheme = { ...activeTheme };
    applyTheme(activeTheme);
  }

  function selectPreset(presetName: string) {
    if (savedThemes[presetName]) {
      activeTheme = { ...savedThemes[presetName] };
      currentTheme = { ...activeTheme };
      applyTheme(activeTheme);
    }
  }

  async function handleSaveTheme() {
    await persistTheme(activeTheme);
    savedThemes[activeTheme.name] = { ...activeTheme };
    saveFeedback = true;
    setTimeout(() => {
      saveFeedback = false;
    }, 2000);
  }

  function handleExport() {
    const jsonStr = exportThemeToJson(activeTheme);
    navigator.clipboard.writeText(jsonStr);
    copyFeedback = true;
    setTimeout(() => {
      copyFeedback = false;
    }, 2000);
  }

  function handleImport() {
    importError = null;
    const parsed = importThemeFromJson(importJsonText);
    if (!parsed) {
      importError = "Invalid VoxP2P theme JSON structure.";
      return;
    }
    activeTheme = parsed;
    currentTheme = { ...parsed };
    applyTheme(parsed);
    importJsonText = "";
  }
</script>

<div class="studio-container">
  <div class="studio-header">
    <h3>Theme Studio & Palette Engine</h3>
    <button class="close-btn" onclick={onClose}>&times;</button>
  </div>

  <div class="studio-content">
    <!-- Presets bar -->
    <div class="section">
      <span class="section-label">Presets</span>
      <div class="presets-row">
        {#each Object.keys(savedThemes) as presetName}
          <button
            class="preset-chip"
            class:active={activeTheme.name === presetName}
            onclick={() => selectPreset(presetName)}
          >
            {presetName}
          </button>
        {/each}
      </div>
    </div>

    <!-- Live Preview Card -->
    <div class="section">
      <span class="section-label">Live Preview</span>
      <div class="preview-box">
        <div class="preview-card">
          <div class="preview-header">
            <div class="preview-avatar">
              <span class="preview-speaking-ring"></span>
              A
            </div>
            <div>
              <div class="preview-title">Alice (Host) 👑</div>
              <div class="preview-sub"><span class="status-dot"></span> Online</div>
            </div>
          </div>
          <p class="preview-text">
            Hello! This is how your theme looks in real-time across VoxP2P.
          </p>
          <div class="preview-actions">
            <button class="btn-accent">Accent Button</button>
            <button class="btn-danger">Danger Button</button>
          </div>
        </div>
      </div>
    </div>

    <!-- Color Controls Grid -->
    <div class="section">
      <span class="section-label">Color Palettes</span>
      <div class="controls-grid">
        <label class="control-item">
          <span>Background Primary</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.bgPrimary}
              oninput={(e) => updateColor("bgPrimary", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.bgPrimary}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Background Secondary</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.bgSecondary}
              oninput={(e) => updateColor("bgSecondary", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.bgSecondary}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Background Tertiary</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.bgTertiary}
              oninput={(e) => updateColor("bgTertiary", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.bgTertiary}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Surface Card</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.surfaceCard}
              oninput={(e) => updateColor("surfaceCard", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.surfaceCard}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Border Color</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.borderColor}
              oninput={(e) => updateColor("borderColor", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.borderColor}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Primary Text</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.textPrimary}
              oninput={(e) => updateColor("textPrimary", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.textPrimary}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Muted Text</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.textMuted}
              oninput={(e) => updateColor("textMuted", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.textMuted}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Accent Primary</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.accentPrimary}
              oninput={(e) => updateColor("accentPrimary", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.accentPrimary}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Accent Hover</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.accentHover}
              oninput={(e) => updateColor("accentHover", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.accentHover}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Voice Speaking Ring</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.voiceSpeakingRing}
              oninput={(e) => updateColor("voiceSpeakingRing", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.voiceSpeakingRing}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Danger Color</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.dangerColor}
              oninput={(e) => updateColor("dangerColor", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.dangerColor}</span>
          </div>
        </label>

        <label class="control-item">
          <span>Online Status Color</span>
          <div class="color-picker-wrap">
            <input
              type="color"
              value={activeTheme.onlineStatusColor}
              oninput={(e) => updateColor("onlineStatusColor", (e.target as HTMLInputElement).value)}
            />
            <span>{activeTheme.onlineStatusColor}</span>
          </div>
        </label>
      </div>
    </div>

    <!-- Sliders for Geometry -->
    <div class="section">
      <span class="section-label">Geometry & Borders</span>
      <div class="sliders-row">
        <label class="slider-item">
          <div class="slider-header">
            <span>Border Width</span>
            <span>{activeTheme.borderWidth}px</span>
          </div>
          <input
            type="range"
            min="0"
            max="6"
            step="1"
            value={activeTheme.borderWidth}
            oninput={(e) => updateColor("borderWidth", parseInt((e.target as HTMLInputElement).value))}
          />
        </label>

        <label class="slider-item">
          <div class="slider-header">
            <span>Border Radius</span>
            <span>{activeTheme.borderRadius}px</span>
          </div>
          <input
            type="range"
            min="0"
            max="24"
            step="1"
            value={activeTheme.borderRadius}
            oninput={(e) => updateColor("borderRadius", parseInt((e.target as HTMLInputElement).value))}
          />
        </label>
      </div>
    </div>

    <!-- Theme Name & Save -->
    <div class="section">
      <label class="section-label" for="theme-name-input-id">Theme Name & Persistence</label>
      <div class="save-row">
        <input
          id="theme-name-input-id"
          type="text"
          class="theme-name-input"
          placeholder="Custom Theme Name..."
          bind:value={activeTheme.name}
        />
        <button class="btn-save" onclick={handleSaveTheme}>
          {saveFeedback ? "Saved to themes.json! ✓" : "Save Custom Theme"}
        </button>
      </div>
    </div>

    <!-- Export & Import Section -->
    <div class="section">
      <label class="section-label" for="theme-import-textarea-id">Export / Import Theme String</label>
      <div class="import-export-row">
        <button class="btn-secondary" onclick={handleExport}>
          {copyFeedback ? "Copied JSON to Clipboard! ✓" : "Export Theme JSON"}
        </button>
      </div>

      <div class="import-box">
        <textarea
          id="theme-import-textarea-id"
          class="import-textarea"
          placeholder="Paste Theme JSON string here to import..."
          bind:value={importJsonText}
        ></textarea>
        {#if importError}
          <div class="error-msg">{importError}</div>
        {/if}
        <button class="btn-accent" onclick={handleImport}>Import Theme</button>
      </div>
    </div>
  </div>
</div>

<style>
  .studio-container {
    display: flex;
    flex-direction: column;
    max-height: 85vh;
    color: var(--text-primary);
    background-color: var(--bg-primary);
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
  }

  .studio-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 20px;
    border-bottom: var(--border-width) solid var(--border-color);
  }

  .studio-header h3 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 600;
  }

  .close-btn {
    background: transparent;
    border: none;
    font-size: 1.5rem;
    color: var(--text-muted);
    cursor: pointer;
    line-height: 1;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  .studio-content {
    padding: 20px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .section-label {
    font-size: 0.82rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
    color: var(--text-muted);
  }

  .presets-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .preset-chip {
    padding: 6px 14px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 0.88rem;
    transition: all 0.15s ease;
  }

  .preset-chip:hover {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
  }

  .preset-chip.active {
    background: var(--accent-primary);
    color: #ffffff;
    border-color: var(--accent-hover);
  }

  .preview-box {
    padding: 16px;
    background: var(--bg-secondary);
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
  }

  .preview-card {
    background: var(--surface-card);
    padding: 16px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .preview-header {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .preview-avatar {
    width: 42px;
    height: 42px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: bold;
    position: relative;
  }

  .preview-speaking-ring {
    position: absolute;
    inset: -3px;
    border-radius: 50%;
    border: 2px solid var(--voice-speaking-ring);
    animation: pulse 1.5s infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(1.05); }
  }

  .preview-title {
    font-weight: 600;
    font-size: 0.95rem;
    color: var(--text-primary);
  }

  .preview-sub {
    font-size: 0.8rem;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--online-status-color);
  }

  .preview-text {
    margin: 0;
    font-size: 0.9rem;
    color: var(--text-primary);
  }

  .preview-actions {
    display: flex;
    gap: 10px;
  }

  .controls-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 12px;
  }

  .control-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--bg-secondary);
    padding: 10px 12px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
  }

  .control-item span {
    font-size: 0.78rem;
    color: var(--text-muted);
  }

  .color-picker-wrap {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .color-picker-wrap input[type="color"] {
    appearance: none;
    -webkit-appearance: none;
    border: none;
    width: 28px;
    height: 28px;
    border-radius: 4px;
    cursor: pointer;
    background: transparent;
  }

  .color-picker-wrap input[type="color"]::-webkit-color-swatch-wrapper {
    padding: 0;
  }

  .color-picker-wrap input[type="color"]::-webkit-color-swatch {
    border: 1px solid var(--border-color);
    border-radius: 4px;
  }

  .color-picker-wrap span {
    font-family: monospace;
    font-size: 0.85rem;
    color: var(--text-primary);
  }

  .sliders-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }

  .slider-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--bg-secondary);
    padding: 12px;
    border-radius: var(--border-radius);
    border: var(--border-width) solid var(--border-color);
  }

  .slider-header {
    display: flex;
    justify-content: space-between;
    font-size: 0.82rem;
    color: var(--text-muted);
  }

  .slider-item input[type="range"] {
    accent-color: var(--accent-primary);
  }

  .save-row {
    display: flex;
    gap: 10px;
  }

  .theme-name-input {
    flex: 1;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 8px 12px;
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  .theme-name-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .import-box {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 8px;
  }

  .import-textarea {
    width: 100%;
    min-height: 80px;
    background: var(--bg-secondary);
    border: var(--border-width) solid var(--border-color);
    border-radius: var(--border-radius);
    padding: 10px;
    color: var(--text-primary);
    font-family: monospace;
    font-size: 0.82rem;
    resize: vertical;
  }

  .import-textarea:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .btn-accent {
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    padding: 8px 16px;
    border-radius: var(--border-radius);
    cursor: pointer;
    font-size: 0.88rem;
    transition: background 0.15s ease;
  }

  .btn-accent:hover {
    background: var(--accent-hover);
  }

  .btn-danger {
    background: var(--danger-color);
    color: #ffffff;
    border: none;
    padding: 8px 16px;
    border-radius: var(--border-radius);
    cursor: pointer;
    font-size: 0.88rem;
  }

  .btn-secondary {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: var(--border-width) solid var(--border-color);
    padding: 8px 16px;
    border-radius: var(--border-radius);
    cursor: pointer;
    font-size: 0.88rem;
  }

  .btn-secondary:hover {
    background: var(--bg-tertiary);
  }

  .btn-save {
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    padding: 8px 18px;
    border-radius: var(--border-radius);
    cursor: pointer;
    font-size: 0.88rem;
  }

  .btn-save:hover {
    background: var(--accent-hover);
  }

  .error-msg {
    color: var(--danger-color);
    font-size: 0.82rem;
  }
</style>
