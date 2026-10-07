<script lang="ts">
  import { onMount } from "svelte";
  import { m } from "$lib/i18n";
  import { toast } from "$lib/stores/toasts";
  import { Check, Eye } from "@lucide/svelte";
  import SystemPromptPreviewModal from "./SystemPromptPreviewModal.svelte";
  import {
    configGet,
    setSystemPrompt,
    setAutoCollapseContextPct,
    setAutoPullChanges,
    environmentGet,
    environmentSave,
    environmentDetect,
    setAddEnvironmentInfo,
  } from "$lib/tauri";

  // Form state — local edits, committed to the backend by save().
  let systemPrompt = $state("");
  let collapsePct = $state(90);
  let autoPull = $state(true);
  let addEnvInfo = $state(true);
  let envInfo = $state("");

  // Pristine snapshots — last values confirmed by the backend.
  // Used by cancel() to revert local edits and by the derived `dirty` flag.
  let lastSystemPrompt = $state("");
  let lastCollapsePct = $state(90);
  let lastAutoPull = $state(true);
  let lastAddEnvInfo = $state(true);
  let lastEnvInfo = $state("");

  let saving = $state(false);
  let detecting = $state(false);
  let justSaved = $state(false);
  let previewOpen = $state(false);
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  // Mirrors `DEFAULT_SYSTEM_PROMPT` in src-tauri/src/config/model.rs. Used to
  // pre-fill the field for configs that predate the default (system_prompt == "").
  const DEFAULT_SYSTEM_PROMPT = `You are a helpful, knowledgeable assistant in a desktop app for chatting and working on code projects.

Guidelines:
- Be clear and concise; avoid filler and unnecessary restatement.
- If a request is ambiguous or lacks details, ask a short clarifying question instead of guessing.
- For code, prefer correct, minimal solutions and explain non-obvious decisions briefly.
- Use the available tools when they help, and briefly state what you are doing.
- When unsure about facts, say so rather than fabricating.`;

  // A single dirty flag for the whole page: any local field diverging from
  // its pristine snapshot makes the form editable.
  const dirty = $derived(
    systemPrompt !== lastSystemPrompt ||
      collapsePct !== lastCollapsePct ||
      autoPull !== lastAutoPull ||
      addEnvInfo !== lastAddEnvInfo ||
      envInfo !== lastEnvInfo,
  );

  onMount(() => {
    void load();
    return () => {
      if (savedTimer) clearTimeout(savedTimer);
    };
  });

  async function load() {
    try {
      const config = await configGet();
      systemPrompt = lastSystemPrompt =
        config.defaults.system_prompt || DEFAULT_SYSTEM_PROMPT;
      collapsePct = lastCollapsePct = config.defaults.auto_collapse_context_pct ?? 90;
      autoPull = lastAutoPull = config.defaults.auto_pull_changes ?? true;
      addEnvInfo = lastAddEnvInfo = config.defaults.add_environment_info ?? true;
      envInfo = lastEnvInfo = await environmentGet();
    } catch (e) {
      toast.error(m.settings_prompts_save_failed(), String(e));
    }
  }

  // Persist every field in one pass. Values are captured into consts before
  // any await: backend setters emit `config:reloaded` / reload events that can
  // overwrite local state mid-flight.
  async function save() {
    const sp = systemPrompt;
    const cp = collapsePct;
    const ap = autoPull;
    const aei = addEnvInfo;
    const ei = envInfo;
    saving = true;
    try {
      await setSystemPrompt(sp);
      await setAutoCollapseContextPct(cp);
      await setAutoPullChanges(ap);
      await setAddEnvironmentInfo(aei);
      await environmentSave(ei);
      lastSystemPrompt = sp;
      lastCollapsePct = cp;
      lastAutoPull = ap;
      lastAddEnvInfo = aei;
      lastEnvInfo = ei;
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

  // Revert all local edits back to the last confirmed snapshots.
  function cancel() {
    systemPrompt = lastSystemPrompt;
    collapsePct = lastCollapsePct;
    autoPull = lastAutoPull;
    addEnvInfo = lastAddEnvInfo;
    envInfo = lastEnvInfo;
  }

  // Auto-detect fills the env-info textarea; the result is a local edit that
  // the user confirms with the page-wide Save button.
  async function detectEnvInfo() {
    detecting = true;
    try {
      envInfo = await environmentDetect();
    } catch (e) {
      toast.error(m.settings_prompts_save_failed(), String(e));
    } finally {
      detecting = false;
    }
  }

  function onPromptKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      if (dirty && !saving) void save();
    }
  }
</script>

<div class="prompts">
  <section class="card">
    <div class="card-head">
      <span class="card-heading">{m.settings_prompts_system_prompt()}</span>
    </div>
    <p class="hint">{m.settings_prompts_system_prompt_hint()}</p>
    <textarea
      class="prompt-area"
      bind:value={systemPrompt}
      rows="6"
      spellcheck="false"
      disabled={saving}
      onkeydown={onPromptKeydown}
    ></textarea>
  </section>

  <section class="card" class:disabled={!addEnvInfo}>
    <div class="card-head">
      <span class="card-heading">{m.settings_prompts_env_heading()}</span>
    </div>
    <p class="hint">{m.settings_prompts_env_hint()}</p>
    <label class="toggle-row">
      <input
        type="checkbox"
        checked={addEnvInfo}
        disabled={saving}
        onchange={(e) => (addEnvInfo = e.currentTarget.checked)}
      />
      <span class="toggle-label">{m.settings_prompts_env_title()}</span>
    </label>
    <div class="env-actions">
      <button class="btn" disabled={detecting || saving} onclick={() => void detectEnvInfo()}>
        {detecting ? m.settings_prompts_env_detecting() : m.settings_prompts_env_detect()}
      </button>
    </div>
    <textarea
      class="prompt-area"
      bind:value={envInfo}
      rows="8"
      spellcheck="false"
      disabled={saving}
      placeholder={m.settings_prompts_env_placeholder()}
    ></textarea>
  </section>

  <section class="card">
    <div class="card-head">
      <span class="card-heading">{m.settings_prompts_compaction()}</span>
      <span class="collapse-val">
        {collapsePct === 0 ? m.settings_prompts_compaction_off() : `${collapsePct}%`}
      </span>
    </div>
    <input
      class="slider"
      type="range"
      min="0"
      max="100"
      step="5"
      value={collapsePct}
      disabled={saving}
      oninput={(e) => (collapsePct = Number((e.currentTarget as HTMLInputElement).value))}
    />
    <p class="hint">{m.settings_prompts_compaction_hint()}</p>
  </section>

  <section class="card">
    <div class="card-head">
      <label class="toggle">
        <input
          type="checkbox"
          checked={autoPull}
          disabled={saving}
          onchange={(e) => (autoPull = e.currentTarget.checked)}
        />
        <span class="toggle-label">{m.settings_prompts_auto_pull()}</span>
      </label>
    </div>
    <p class="hint">{m.settings_prompts_auto_pull_hint()}</p>
  </section>

  <div class="footer">
    <button class="btn ghost" onclick={() => (previewOpen = true)} disabled={saving}>
      <Eye size={14} />
      {m.settings_prompts_preview()}
    </button>
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

<SystemPromptPreviewModal open={previewOpen} onclose={() => (previewOpen = false)} />

<style>
  .prompts {
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
  .card.disabled {
    opacity: 0.65;
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
  .collapse-val {
    font-size: 0.75rem;
    color: var(--muted-foreground);
    font-variant-numeric: tabular-nums;
  }
  .toggle-row {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8125rem;
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .toggle input,
  .toggle-row input {
    width: 0.95rem;
    height: 0.95rem;
    accent-color: var(--primary);
    cursor: default;
  }
  .env-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .slider {
    width: 100%;
    accent-color: var(--primary);
    cursor: default;
  }
  .prompt-area {
    width: 100%;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-mono);
    resize: vertical;
    outline: none;
  }
  .prompt-area:focus {
    border-color: var(--ring);
  }
  .prompt-area:disabled {
    opacity: 0.6;
  }
  .footer {
    position: sticky;
    bottom: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
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
  .btn.ghost {
    background: transparent;
  }
</style>