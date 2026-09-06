<script lang="ts">
  import { m } from "$lib/i18n";

  let {
    value,
    onchange,
    disabled = false,
    align = "left",
  }: {
    value: string;
    onchange: (hex: string) => void;
    disabled?: boolean;
    align?: "left" | "right";
  } = $props();

  const HEX_RE = /^#?[0-9a-fA-F]{6}$/;
  const FALLBACK = "#6366f1";
  const PRESETS = [
    "#6366f1",
    "#8b5cf6",
    "#ec4899",
    "#ef4444",
    "#f59e0b",
    "#10b981",
    "#3b82f6",
    "#64748b",
    "#111827",
    "#f5f5f5",
  ];

  let svEl = $state<HTMLDivElement | null>(null);
  let hueEl = $state<HTMLDivElement | null>(null);
  let open = $state(false);
  let hexFocused = $state(false);
  let drag: "sv" | "hue" | null = null;

  let hex = $state(FALLBACK);
  let hexText = $state(FALLBACK);

  const hsv = $derived(hexToHsv(hex));
  const hexInvalid = $derived(!HEX_RE.test(hexText.trim()));
  const swatchHex = $derived(parseHex(value) ?? FALLBACK);

  $effect(() => {
    if (!hexFocused) hexText = hex;
  });

  function clamp01(n: number) {
    return Math.min(1, Math.max(0, n));
  }

  function hsvToRgb({ h, s, v }: { h: number; s: number; v: number }) {
    const f = (n: number) => {
      const k = (n + h / 60) % 6;
      return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
    };
    return {
      r: Math.round(f(5) * 255),
      g: Math.round(f(3) * 255),
      b: Math.round(f(1) * 255),
    };
  }

  function rgbToHsv(r: number, g: number, b: number) {
    const rr = r / 255;
    const gg = g / 255;
    const bb = b / 255;
    const max = Math.max(rr, gg, bb);
    const d = max - Math.min(rr, gg, bb);
    let h = 0;
    if (d !== 0) {
      if (max === rr) h = 60 * (((gg - bb) / d) % 6);
      else if (max === gg) h = 60 * ((bb - rr) / d + 2);
      else h = 60 * ((rr - gg) / d + 4);
      if (h < 0) h += 360;
    }
    return { h, s: max === 0 ? 0 : d / max, v: max };
  }

  function hsvToHex({ h, s, v }: { h: number; s: number; v: number }) {
    const { r, g, b } = hsvToRgb({ h, s, v });
    return `#${[r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
  }

  function hexToHsv(hexStr: string) {
    const n = parseInt(hexStr.slice(1), 16);
    return rgbToHsv((n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff);
  }

  function parseHex(v: string) {
    const t = v.trim();
    return HEX_RE.test(t) ? `#${t.replace(/^#/, "").toLowerCase()}` : null;
  }

  function applyHex(next: string) {
    hex = next;
    if (!hexFocused) hexText = next;
  }

  function applyHsv(h: number, s: number, v: number) {
    applyHex(hsvToHex({ h, s, v }));
  }

  function moveSv(e: PointerEvent) {
    if (!svEl) return;
    const r = svEl.getBoundingClientRect();
    applyHsv(hsv.h, clamp01((e.clientX - r.left) / r.width), 1 - clamp01((e.clientY - r.top) / r.height));
  }

  function moveHue(e: PointerEvent) {
    if (!hueEl) return;
    const r = hueEl.getBoundingClientRect();
    applyHsv(clamp01((e.clientX - r.left) / r.width) * 360, hsv.s, hsv.v);
  }

  function onSvDown(e: PointerEvent) {
    if (disabled || !e.isPrimary || e.button !== 0) return;
    drag = "sv";
    svEl?.setPointerCapture(e.pointerId);
    moveSv(e);
  }

  function onHueDown(e: PointerEvent) {
    if (disabled || !e.isPrimary || e.button !== 0) return;
    drag = "hue";
    hueEl?.setPointerCapture(e.pointerId);
    moveHue(e);
  }

  function onSvMove(e: PointerEvent) {
    if (drag === "sv") moveSv(e);
  }

  function onHueMove(e: PointerEvent) {
    if (drag === "hue") moveHue(e);
  }

  function onDragEnd() {
    drag = null;
  }

  function onHexInput() {
    const parsed = parseHex(hexText);
    if (parsed) applyHex(parsed);
  }

  function onHexKeydown(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    e.preventDefault();
    const parsed = parseHex(hexText);
    if (parsed) applyHex(parsed);
    else hexText = hex;
  }

  function onHexBlur() {
    hexFocused = false;
    if (!parseHex(hexText)) hexText = hex;
  }

  function openPicker() {
    hex = swatchHex;
    hexText = swatchHex;
    open = true;
  }

  function close() {
    open = false;
    hexFocused = false;
  }

  function save() {
    onchange(hex);
    close();
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (!open || e.key !== "Escape") return;
    // Capture phase: close only the picker, not enclosing dialogs.
    e.stopPropagation();
    e.preventDefault();
    close();
  }
</script>

<svelte:window onkeydowncapture={onWindowKeydown} />

<div class="cp" class:disabled>
  <button
    class="swatch"
    style:background={swatchHex}
    title={swatchHex}
    aria-haspopup="dialog"
    aria-expanded={open}
    disabled={disabled}
    onclick={() => (open ? close() : openPicker())}
  ></button>

  {#if open}
    <button class="backdrop" aria-label={m.common_close()} onclick={close}></button>
    <div class="popover" class:right={align === "right"} role="dialog" aria-label={m.project_color()}>
      <div
        class="sv"
        role="presentation"
        bind:this={svEl}
        style:background="linear-gradient(to top, #000, rgb(0 0 0 / 0)), linear-gradient(to right, #fff, rgb(255 255 255 / 0)), hsl({hsv.h} 100% 50%)"
        onpointerdown={onSvDown}
        onpointermove={onSvMove}
        onpointerup={onDragEnd}
        onpointercancel={onDragEnd}
      >
        <div class="dot" style:left="{hsv.s * 100}%" style:top="{(1 - hsv.v) * 100}%" style:background={hex}></div>
      </div>

      <div
        class="hue"
        role="presentation"
        bind:this={hueEl}
        onpointerdown={onHueDown}
        onpointermove={onHueMove}
        onpointerup={onDragEnd}
        onpointercancel={onDragEnd}
      >
        <div class="dot" style:left="{(hsv.h / 360) * 100}%" style:background="hsl({hsv.h} 100% 50%)"></div>
      </div>

      <input
        class="hex-field"
        class:invalid={hexInvalid}
        bind:value={hexText}
        oninput={onHexInput}
        onkeydown={onHexKeydown}
        onfocus={() => (hexFocused = true)}
        onblur={onHexBlur}
        aria-label={m.project_color()}
        spellcheck="false"
        maxlength="7"
      />

      <div class="presets">
        {#each PRESETS as p (p)}
          <button
            class="preset"
            class:active={p === hex}
            style:background={p}
            title={p}
            onclick={() => applyHex(p)}
          ></button>
        {/each}
      </div>

      <div class="foot">
        <button class="primary" onclick={save}>{m.common_save()}</button>
        <button class="ghost" onclick={close}>{m.common_cancel()}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .cp {
    position: relative;
  }
  .cp.disabled {
    opacity: 0.5;
    pointer-events: none;
  }
  .swatch {
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .swatch:hover {
    border-color: var(--muted-foreground);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: rgb(0 0 0 / 0.4);
    border: none;
    padding: 0;
    cursor: default;
  }
  .popover {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    width: 17rem;
    max-width: calc(100vw - 1rem);
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.625rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--popover);
    color: var(--popover-foreground);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.18);
    font-family: var(--font-sans);
  }
  .popover.right {
    left: auto;
    right: 0;
  }
  .sv {
    position: relative;
    height: 8.75rem;
    border-radius: var(--radius-sm);
    cursor: crosshair;
    touch-action: none;
  }
  .hue {
    position: relative;
    height: 0.75rem;
    border-radius: var(--radius-sm);
    background: linear-gradient(to right, #f00, #ff0 17%, #0f0 33%, #0ff 50%, #00f 67%, #f0f 83%, #f00);
    cursor: crosshair;
    touch-action: none;
  }
  .dot {
    position: absolute;
    width: 0.875rem;
    height: 0.875rem;
    border: 2px solid #fff;
    border-radius: 9999px;
    box-shadow: 0 0 0 1px rgb(0 0 0 / 0.35);
    transform: translate(-50%, -50%);
    pointer-events: none;
  }
  .sv .dot {
    width: 0.75rem;
    height: 0.75rem;
  }
  .hue .dot {
    top: 50%;
  }
  .hex-field {
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-family: var(--font-mono);
    font-size: 0.8125rem;
    outline: none;
  }
  .hex-field:focus {
    border-color: var(--ring);
  }
  .hex-field.invalid {
    border-color: var(--destructive);
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(10, 1fr);
    gap: 0.25rem;
  }
  .preset {
    aspect-ratio: 1;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .preset:hover {
    border-color: var(--muted-foreground);
  }
  .preset.active {
    outline: 2px solid var(--ring);
    outline-offset: 1px;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  .foot button {
    padding: 0.3rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    font-size: 0.8125rem;
    cursor: default;
  }
  .foot .primary {
    background: var(--primary);
    color: var(--primary-foreground);
    border-color: var(--primary);
  }
  .foot .ghost {
    background: var(--background);
    color: var(--foreground);
  }
</style>