<script lang="ts">
  import { onMount } from "svelte";
  import { m } from "$lib/i18n";
  import { toast } from "$lib/stores/toasts";
  import { Check } from "@lucide/svelte";
  import {
    configGet,
    setTheme,
    setFontFamily,
    setFontSize,
    type AppConfig,
  } from "$lib/tauri";
  import { theme, applyTheme, type Theme } from "$lib/stores/theme";

  const themes: Theme[] = ["system", "light", "dark"];

  // Font family suggestions (common cross-platform fonts). The field is a free
  // <input> with a <datalist>, so users can also type any family installed on
  // their OS that isn't listed.
  const FONT_SUGGESTIONS = [
    "Inter", "Roboto", "Segoe UI", "SF Pro Text", "San Francisco", "Helvetica Neue",
    "Helvetica", "Arial", "Cantarell", "Ubuntu", "Noto Sans", "DejaVu Sans",
    "Liberation Sans", "Verdana", "Tahoma", "Trebuchet MS", "Calibri", "Open Sans",
    "Fira Sans", "Poppins", "Lato", "Montserrat", "Source Sans Pro", "system-ui",
    "JetBrains Mono", "Fira Code", "Cascadia Mono", "SF Mono", "Menlo", "Consolas",
    "Monaco", "DejaVu Sans Mono", "Liberation Mono", "Courier New",
  ];

  let fontFamily = $state("");
  let fontSize = $state(16);
  let lastFontFamily = $state("");
  let lastFontSize = $state(16);
  let saving = $state(false);
  let justSaved = $state(false);
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  const dirty = $derived(fontFamily.trim() !== lastFontFamily || fontSize !== lastFontSize);

  onMount(() => {
    void load();
    return () => {
      if (savedTimer) clearTimeout(savedTimer);
    };
  });

  async function load() {
    try {
      const cfg = await configGet();
      fontFamily = cfg.appearance.font_family ?? "";
      fontSize = cfg.appearance.font_size ?? 16;
      lastFontFamily = fontFamily;
      lastFontSize = fontSize;
    } catch {
      /* ignore */
    }
  }

  async function chooseTheme(t: Theme) {
    theme.set(t);
    applyTheme(t);
    try {
      await setTheme(t);
    } catch {
      /* ignore */
    }
  }

  async function save() {
    const family = fontFamily.trim();
    const size = Math.round(fontSize * 10) / 10;
    saving = true;
    try {
      await setFontFamily(family);
      await setFontSize(size);
      fontFamily = family;
      fontSize = size;
      lastFontFamily = family;
      lastFontSize = size;
      toast.success(m.settings_prompts_saved());
      justSaved = true;
      if (savedTimer) clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (justSaved = false), 1800);
    } catch (e) {
      toast.error(m.settings_prompts_save_failed(), String(e));
    } finally {
      saving = false;
    }
  }

  function cancel() {
    fontFamily = lastFontFamily;
    fontSize = lastFontSize;
  }

  function onSizeInput(e: Event) {
    const v = Number((e.currentTarget as HTMLInputElement).value);
    fontSize = Number.isFinite(v) ? v : lastFontSize;
  }
</script>

<div class="appearance">
  <section class="card">
    <div class="card-head">
      <span class="card-heading">{m.settings_appearance_theme()}</span>
    </div>
    <div class="theme-chips">
      {#each themes as t}
        <button
          class="theme-chip"
          class:active={$theme === t}
          onclick={() => chooseTheme(t)}
        >
          {t === "system"
            ? m.settings_appearance_theme_system()
            : t === "light"
              ? m.settings_appearance_theme_light()
              : m.settings_appearance_theme_dark()}
        </button>
      {/each}
    </div>
  </section>

  <section class="card">
    <div class="card-head">
      <span class="card-heading">{m.settings_appearance_font_family()}</span>
    </div>
    <p class="hint">{m.settings_appearance_font_hint()}</p>
    <input
      class="font-input"
      type="text"
      list="font-suggestions"
      placeholder={m.settings_appearance_font_family_system()}
      value={fontFamily}
      disabled={saving}
      oninput={(e) => (fontFamily = (e.currentTarget as HTMLInputElement).value)}
    />
    <datalist id="font-suggestions">
      {#each FONT_SUGGESTIONS as f (f)}
        <option value={f}></option>
      {/each}
    </datalist>

    <div class="field">
      <span class="label">{m.settings_appearance_font_size()}</span>
      <div class="size-row">
        <input
          class="size-input"
          type="number"
          min="10"
          max="28"
          step="0.5"
          value={fontSize}
          disabled={saving}
          oninput={onSizeInput}
        />
        <span class="size-unit">px</span>
      </div>
      <p class="hint">{m.settings_appearance_font_size_hint()}</p>
    </div>
  </section>

  <div class="footer">
    {#if justSaved}<span class="saved-check" title={m.settings_prompts_saved()}><Check size={12} /></span>{/if}
    <div class="spacer"></div>
    <button class="btn" onclick={cancel} disabled={!dirty || saving}>
      {m.common_cancel()}
    </button>
    <button class="btn primary" onclick={() => void save()} disabled={!dirty || saving}>
      {saving ? "…" : m.common_save()}
    </button>
  </div>
</div>

<style>
  .appearance {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    min-height: 100%;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 0.75rem 0.875rem;
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .card-heading {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .hint {
    margin: 0;
    font-size: 0.72rem;
    color: var(--muted-foreground);
    line-height: 1.35;
  }
  .theme-chips {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .theme-chip {
    padding: 0.375rem 0.75rem;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    cursor: default;
  }
  .theme-chip:hover {
    background: var(--accent);
  }
  .theme-chip.active {
    border-color: var(--primary);
    color: var(--primary);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .label {
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .font-input,
  .size-input {
    width: 100%;
    padding: 0.35rem 0.55rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-sans);
    outline: none;
  }
  .font-input:focus,
  .size-input:focus {
    border-color: var(--ring);
  }
  .font-input:disabled,
  .size-input:disabled {
    opacity: 0.5;
  }
  .size-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .size-input {
    width: 6rem;
    font-variant-numeric: tabular-nums;
  }
  .size-unit {
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .footer {
    position: sticky;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: auto;
    padding: 0.6rem 0 0.25rem;
    background: var(--background);
    border-top: 1px solid var(--border);
  }
  .saved-check {
    display: inline-flex;
    color: hsl(142 71% 45%);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.35rem 0.85rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    cursor: default;
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .btn:hover:not(:disabled) {
    background: var(--accent);
  }
  .btn.primary {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover:not(:disabled) {
    opacity: 0.9;
    background: var(--primary);
  }
</style>