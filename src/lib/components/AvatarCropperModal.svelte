<script lang="ts">
  import { onMount } from "svelte";

  let {
    imageSrc,
    onApply,
    onCancel,
  }: {
    imageSrc: string;
    onApply: (bytes: Uint8Array) => Promise<void> | void;
    onCancel: () => void;
  } = $props();

  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let zoom = $state<number>(1.0);
  let panX = $state<number>(0);
  let panY = $state<number>(0);
  let isDragging = $state<boolean>(false);
  let isApplying = $state<boolean>(false);
  let errorMessage = $state<string | null>(null);

  const CANVAS_SIZE = 320;
  const CROP_RADIUS = 120; // 240px diameter crop area
  const CENTER = CANVAS_SIZE / 2;

  let img: HTMLImageElement | null = null;
  let baseScale = 1.0;
  let dragStart = { x: 0, y: 0 };
  let initialPan = { x: 0, y: 0 };

  onMount(() => {
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => {
      img = image;
      // Calculate base scale so image fully covers the crop circle diameter (2 * CROP_RADIUS)
      const minDim = Math.min(image.naturalWidth, image.naturalHeight);
      baseScale = (CROP_RADIUS * 2) / minDim;
      panX = 0;
      panY = 0;
      zoom = 1.0;
      draw();
    };
    image.onerror = () => {
      errorMessage = "Не удалось загрузить выбранное изображение";
    };
    image.src = imageSrc;

    const handleWheel = (e: WheelEvent) => {
      if (!canvasEl) return;
      e.preventDefault();
      const delta = -e.deltaY * 0.002;
      const nextZoom = Math.max(1.0, Math.min(4.0, zoom + delta));
      if (nextZoom !== zoom) {
        zoom = nextZoom;
        clampPan();
        draw();
      }
    };

    const canvas = canvasEl;
    if (canvas) {
      canvas.addEventListener("wheel", handleWheel, { passive: false });
    }

    return () => {
      if (canvas) {
        canvas.removeEventListener("wheel", handleWheel);
      }
    };
  });

  function getScale(): number {
    return baseScale * zoom;
  }

  function clampPan() {
    if (!img) return;
    const currentScale = getScale();
    const scaledWidth = img.naturalWidth * currentScale;
    const scaledHeight = img.naturalHeight * currentScale;

    const maxPanX = Math.max(0, scaledWidth / 2 - CROP_RADIUS);
    const maxPanY = Math.max(0, scaledHeight / 2 - CROP_RADIUS);

    panX = Math.max(-maxPanX, Math.min(maxPanX, panX));
    panY = Math.max(-maxPanY, Math.min(maxPanY, panY));
  }

  function draw() {
    if (!canvasEl || !img) return;
    const ctx = canvasEl.getContext("2d");
    if (!ctx) return;

    const currentScale = getScale();
    const scaledWidth = img.naturalWidth * currentScale;
    const scaledHeight = img.naturalHeight * currentScale;

    // 1. Clear background
    ctx.clearRect(0, 0, CANVAS_SIZE, CANVAS_SIZE);

    // 2. Draw scaled & panned image
    const drawX = CENTER + panX - scaledWidth / 2;
    const drawY = CENTER + panY - scaledHeight / 2;
    ctx.drawImage(img, drawX, drawY, scaledWidth, scaledHeight);

    // 3. Draw dimmed backdrop with circular cutout
    ctx.save();
    ctx.fillStyle = "rgba(12, 14, 20, 0.72)";
    ctx.beginPath();
    ctx.rect(0, 0, CANVAS_SIZE, CANVAS_SIZE);
    // Draw cutout circle counter-clockwise
    ctx.arc(CENTER, CENTER, CROP_RADIUS, 0, Math.PI * 2, true);
    ctx.fill();

    // 4. Draw circular crop boundary ring
    ctx.beginPath();
    ctx.arc(CENTER, CENTER, CROP_RADIUS, 0, Math.PI * 2);
    ctx.strokeStyle = "#6366f1";
    ctx.lineWidth = 2.5;
    ctx.stroke();

    // 5. Subtle rule-of-thirds grid inside the crop circle
    ctx.save();
    ctx.beginPath();
    ctx.arc(CENTER, CENTER, CROP_RADIUS, 0, Math.PI * 2);
    ctx.clip();
    ctx.strokeStyle = "rgba(255, 255, 255, 0.16)";
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 4]);

    ctx.beginPath();
    const offset = (CROP_RADIUS * 2) / 3;
    const left = CENTER - CROP_RADIUS;
    const top = CENTER - CROP_RADIUS;

    // Vertical grid lines
    ctx.moveTo(left + offset, top);
    ctx.lineTo(left + offset, top + CROP_RADIUS * 2);
    ctx.moveTo(left + offset * 2, top);
    ctx.lineTo(left + offset * 2, top + CROP_RADIUS * 2);

    // Horizontal grid lines
    ctx.moveTo(left, top + offset);
    ctx.lineTo(left + CROP_RADIUS * 2, top + offset);
    ctx.moveTo(left, top + offset * 2);
    ctx.lineTo(left + CROP_RADIUS * 2, top + offset * 2);

    ctx.stroke();
    ctx.restore();

    ctx.restore();
  }

  function handlePointerDown(e: PointerEvent) {
    if (!canvasEl) return;
    canvasEl.setPointerCapture(e.pointerId);
    isDragging = true;
    dragStart = { x: e.clientX, y: e.clientY };
    initialPan = { x: panX, y: panY };
  }

  function handlePointerMove(e: PointerEvent) {
    if (!isDragging || !img) return;
    const dx = e.clientX - dragStart.x;
    const dy = e.clientY - dragStart.y;
    panX = initialPan.x + dx;
    panY = initialPan.y + dy;
    clampPan();
    draw();
  }

  function handlePointerUp(e: PointerEvent) {
    if (!canvasEl) return;
    try {
      canvasEl.releasePointerCapture(e.pointerId);
    } catch (_) {}
    isDragging = false;
  }

  function handleZoomInput(e: Event) {
    const val = parseFloat((e.target as HTMLInputElement).value);
    zoom = val;
    clampPan();
    draw();
  }

  function handleReset() {
    zoom = 1.0;
    panX = 0;
    panY = 0;
    draw();
  }

  async function handleApply() {
    if (!img) return;
    isApplying = true;
    errorMessage = null;

    try {
      const currentScale = getScale();
      const srcCropDiameter = (CROP_RADIUS * 2) / currentScale;
      const srcCenterX = img.naturalWidth / 2 - panX / currentScale;
      const srcCenterY = img.naturalHeight / 2 - panY / currentScale;
      const srcCropX = Math.max(0, srcCenterX - srcCropDiameter / 2);
      const srcCropY = Math.max(0, srcCenterY - srcCropDiameter / 2);

      const offscreen = document.createElement("canvas");
      offscreen.width = 256;
      offscreen.height = 256;
      const offCtx = offscreen.getContext("2d");
      if (!offCtx) throw new Error("Canvas 2D context not available");

      // High quality scaling
      offCtx.imageSmoothingEnabled = true;
      offCtx.imageSmoothingQuality = "high";
      offCtx.drawImage(
        img,
        srcCropX,
        srcCropY,
        srcCropDiameter,
        srcCropDiameter,
        0,
        0,
        256,
        256
      );

      const blob = await new Promise<Blob | null>((resolve) => {
        offscreen.toBlob((b) => resolve(b), "image/png");
      });

      if (!blob) throw new Error("Failed to encode cropped avatar");

      const arrayBuf = await blob.arrayBuffer();
      const bytes = new Uint8Array(arrayBuf);
      await onApply(bytes);
    } catch (err: any) {
      errorMessage = err?.message || String(err);
    } finally {
      isApplying = false;
    }
  }
</script>

<!-- Backdrop Modal -->
<div class="cropper-backdrop" role="dialog" aria-modal="true" aria-labelledby="cropper-title">
  <div class="cropper-card">
    <div class="cropper-header">
      <h3 id="cropper-title">✂️ Редактирование аватара</h3>
      <button class="close-btn" onclick={onCancel} aria-label="Закрыть">✕</button>
    </div>

    {#if errorMessage}
      <div class="cropper-error">
        ⚠️ {errorMessage}
      </div>
    {/if}

    <div class="canvas-wrapper">
      <canvas
        bind:this={canvasEl}
        width={CANVAS_SIZE}
        height={CANVAS_SIZE}
        class="cropper-canvas"
        class:grabbing={isDragging}
        onpointerdown={handlePointerDown}
        onpointermove={handlePointerMove}
        onpointerup={handlePointerUp}
        onpointercancel={handlePointerUp}
      ></canvas>
      <div class="canvas-hint">
        Перетаскивайте для смещения • Колесо мыши для масштаба
      </div>
    </div>

    <!-- Zoom Control Slider -->
    <div class="zoom-controls">
      <span class="zoom-icon">🔍</span>
      <input
        type="range"
        min="1.0"
        max="4.0"
        step="0.01"
        value={zoom}
        oninput={handleZoomInput}
        class="zoom-slider"
        aria-label="Масштаб"
      />
      <span class="zoom-val">{zoom.toFixed(2)}x</span>
      <button class="reset-btn" onclick={handleReset} title="Сбросить масштаб и положение">
        ↺
      </button>
    </div>

    <!-- Actions -->
    <div class="cropper-actions">
      <button class="btn-cancel" onclick={onCancel} disabled={isApplying}>
        Отмена
      </button>
      <button class="btn-apply" onclick={handleApply} disabled={isApplying}>
        {#if isApplying}
          Сохранение...
        {:else}
          Применить
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .cropper-backdrop {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.8);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 9999;
    backdrop-filter: blur(6px);
    padding: 16px;
    animation: fadeIn 0.15s ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .cropper-card {
    background: var(--surface-card, #1e222d);
    border: var(--border-width, 1px) solid var(--border-color, #2a2e3d);
    border-radius: var(--border-radius, 12px);
    width: 100%;
    max-width: 400px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .cropper-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border-color, #2a2e3d);
  }

  .cropper-header h3 {
    margin: 0;
    font-size: 1.05rem;
    font-weight: 600;
    color: var(--text-primary, #ffffff);
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted, #94a3b8);
    font-size: 1.1rem;
    cursor: pointer;
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
    transition: background 0.15s, color 0.15s;
  }

  .close-btn:hover {
    background: var(--bg-tertiary, #2e3346);
    color: var(--text-primary, #ffffff);
  }

  .cropper-error {
    background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.4);
    color: #fca5a5;
    padding: 10px 14px;
    margin: 12px 16px 0;
    border-radius: 6px;
    font-size: 0.85rem;
  }

  .canvas-wrapper {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 16px 16px 8px;
  }

  .cropper-canvas {
    width: 320px;
    height: 320px;
    border-radius: 8px;
    background: #0f1117;
    cursor: grab;
    touch-action: none;
    user-select: none;
    box-shadow: inset 0 0 20px rgba(0, 0, 0, 0.8);
  }

  .cropper-canvas.grabbing {
    cursor: grabbing;
  }

  .canvas-hint {
    margin-top: 8px;
    font-size: 0.75rem;
    color: var(--text-muted, #94a3b8);
    text-align: center;
  }

  .zoom-controls {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 20px 12px;
  }

  .zoom-icon {
    font-size: 1rem;
    user-select: none;
  }

  .zoom-slider {
    flex: 1;
    accent-color: var(--accent-primary, #6366f1);
    height: 6px;
    cursor: pointer;
  }

  .zoom-val {
    font-size: 0.85rem;
    font-family: monospace;
    color: var(--text-primary, #ffffff);
    min-width: 44px;
    text-align: right;
  }

  .reset-btn {
    background: var(--bg-secondary, #252a38);
    border: 1px solid var(--border-color, #2a2e3d);
    color: var(--text-muted, #94a3b8);
    border-radius: 6px;
    width: 28px;
    height: 28px;
    font-size: 1rem;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.15s;
  }

  .reset-btn:hover {
    background: var(--bg-tertiary, #2e3346);
    color: var(--text-primary, #ffffff);
  }

  .cropper-actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 14px 18px;
    background: var(--bg-primary, #151821);
    border-top: 1px solid var(--border-color, #2a2e3d);
  }

  .btn-cancel,
  .btn-apply {
    padding: 8px 18px;
    border-radius: 6px;
    font-size: 0.9rem;
    font-weight: 500;
    cursor: pointer;
    min-height: 40px;
    min-width: 90px;
    transition: all 0.15s;
  }

  .btn-cancel {
    background: transparent;
    border: 1px solid var(--border-color, #2a2e3d);
    color: var(--text-primary, #ffffff);
  }

  .btn-cancel:hover:not(:disabled) {
    background: var(--bg-tertiary, #2e3346);
  }

  .btn-apply {
    background: var(--accent-primary, #6366f1);
    border: none;
    color: #ffffff;
  }

  .btn-apply:hover:not(:disabled) {
    background: var(--accent-hover, #4f46e5);
  }

  .btn-cancel:disabled,
  .btn-apply:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
