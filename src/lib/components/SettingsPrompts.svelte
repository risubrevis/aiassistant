<script lang="ts">
  import { onMount } from "svelte";
  import { m } from "$lib/i18n";
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

  let systemPrompt = $state("");
  let collapsePct = $state(90);
  let autoPull = $state(true);
  let promptSaved = $state(false);
  let promptError = $state("");
  let addEnvInfo = $state(true);
  let envInfo = $state("");
  let envInfoSaved = $state("");
  let envInfoSaving = $state(false);
  let envInfoDetecting = $state(false);
  let envInfoSavedFlash = $state(false);
  let envInfoError = $state("");

  // Mirrors `DEFAULT_SYSTEM_PROMPT` in src-tauri/src/config/model.rs. Used to
  // pre-fill the field for configs that predate the default (system_prompt == "").
  const DEFAULT_SYSTEM_PROMPT = `You are a helpful, knowledgeable assistant in a desktop app for chatting and working on code projects.

Guidelines:
- Be clear and concise; avoid filler and unnecessary restatement.
- If a request is ambiguous or lacks details, ask a short clarifying question instead of guessing.
- For code, prefer correct, minimal solutions and explain non-obvious decisions briefly.
- Use the available tools when they help, and briefly state what you are doing.
- When unsure about facts, say so rather than fabricating.`;

  let savedTimer: ReturnType<typeof setTimeout> | undefined;
  let envSavedTimer: ReturnType<typeof setTimeout> | undefined;

  let envDirty = $derived(envInfo !== envInfoSaved);

  onMount(() => {
    void load();
    return () => {
      clearTimeout(savedTimer);
      clearTimeout(envSavedTimer);
    };
  });

  async function load() {
    try {
      const config = await configGet();
      systemPrompt = config.defaults.system_prompt || DEFAULT_SYSTEM_PROMPT;
      collapsePct = config.defaults.auto_collapse_context_pct ?? 90;
      autoPull = config.defaults.auto_pull_changes ?? true;
      addEnvInfo = config.defaults.add_environment_info ?? true;
      envInfo = envInfoSaved = await environmentGet();
    } catch (e) {
      promptError = String(e);
    }
  }

  async function savePrompt() {
    try {
      await setSystemPrompt(systemPrompt);
      promptError = "";
      promptSaved = true;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (promptSaved = false), 1500);
    } catch (e) {
      promptSaved = false;
      promptError = String(e);
    }
  }

  async function saveCollapse(value: number) {
    try {
      await setAutoCollapseContextPct(value);
      promptError = "";
    } catch (e) {
      promptError = String(e);
    }
  }

  async function saveAutoPull(value: boolean) {
    try {
      await setAutoPullChanges(value);
      promptError = "";
    } catch (e) {
      promptError = String(e);
    }
  }

  async function saveEnvInfo() {
    envInfoSaving = true;
    try {
      await environmentSave(envInfo);
      envInfoSaved = envInfo;
      envInfoError = "";
      envInfoSavedFlash = true;
      clearTimeout(envSavedTimer);
      envSavedTimer = setTimeout(() => (envInfoSavedFlash = false), 1500);
    } catch (e) {
      envInfoError = String(e);
    } finally {
      envInfoSaving = false;
    }
  }

  function cancelEnvInfo() {
    envInfo = envInfoSaved;
    envInfoError = "";
  }

  async function detectEnvInfo() {
    envInfoDetecting = true;
    try {
      envInfo = await environmentDetect();
      envInfoError = "";
    } catch (e) {
      envInfoError = String(e);
    } finally {
      envInfoDetecting = false;
    }
  }

  async function saveAddEnvInfo(value: boolean) {
    addEnvInfo = value;
    try {
      await setAddEnvironmentInfo(value);
      envInfoError = "";
    } catch (e) {
      envInfoError = String(e);
    }
  }

  function onPromptKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void savePrompt();
    }
  }

</script>

<div class="prompts">
  <section class="section">
    <div class="sec-head">
      <span class="sec-title">{m.settings_prompts_env_title()}</span>
      <input
        class="toggle"
        type="checkbox"
        checked={addEnvInfo}
        onchange={(e) => void saveAddEnvInfo(e.currentTarget.checked)}
      />
    </div>
    <div class="hint">{m.settings_prompts_env_hint()}</div>
    <div class="env-actions">
      <button class="btn" disabled={envInfoDetecting} onclick={() => void detectEnvInfo()}>
        {envInfoDetecting ? m.settings_prompts_env_detecting() : m.settings_prompts_env_detect()}
      </button>
      <span class="spacer"></span>
      <button class="btn" disabled={!envDirty || envInfoSaving} onclick={() => void saveEnvInfo()}>
        {m.common_save()}
      </button>
      <button class="btn" disabled={!envDirty || envInfoSaving} onclick={cancelEnvInfo}>
        {m.common_cancel()}
      </button>
    </div>
    <textarea
      class="prompt-area"
      bind:value={envInfo}
      rows="8"
      spellcheck="false"
      placeholder={m.settings_prompts_env_placeholder()}
    ></textarea>
    <div class="status">
      {#if envInfoSavedFlash}<span class="saved">{m.settings_prompts_saved()}</span>
      {:else if envInfoError}<span class="err">{envInfoError}</span>{/if}
    </div>
  </section>

  <section class="section">
    <label class="field">
      <span class="sec-title">{m.settings_prompts_system_prompt()}</span>
      <textarea
        class="prompt-area"
        bind:value={systemPrompt}
        rows="6"
        spellcheck="false"
        onblur={() => void savePrompt()}
        onkeydown={onPromptKeydown}
      ></textarea>
    </label>
    <div class="hint">{m.settings_prompts_system_prompt_hint()}</div>
    <div class="status">
      {#if promptSaved}<span class="saved">{m.settings_prompts_saved()}</span>
      {:else if promptError}<span class="err">{promptError}</span>{/if}
    </div>
  </section>

  <section class="section">
    <div class="sec-head">
      <span class="sec-title">{m.settings_prompts_compaction()}</span>
      <span class="collapse-val">{collapsePct === 0 ? m.settings_prompts_compaction_off() : `${collapsePct}%`}</span>
    </div>
    <input
      class="slider"
      type="range"
      min="0"
      max="100"
      step="5"
      value={collapsePct}
      oninput={(e) => (collapsePct = Number((e.currentTarget as HTMLInputElement).value))}
      onchange={(e) => void saveCollapse(Number((e.currentTarget as HTMLInputElement).value))}
    />
    <div class="hint">{m.settings_prompts_compaction_hint()}</div>
  </section>

  <section class="section">
    <div class="sec-head">
      <span class="sec-title">{m.settings_prompts_auto_pull()}</span>
      <input
        class="toggle"
        type="checkbox"
        checked={autoPull}
        onchange={(e) => {
          autoPull = e.currentTarget.checked;
          void saveAutoPull(autoPull);
        }}
      />
    </div>
    <div class="hint">{m.settings_prompts_auto_pull_hint()}</div>
  </section>
</div>

<style>
  .prompts {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }
  .sec-title {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .sec-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    column-gap: 0.75rem;
  }
  .collapse-val {
    font-size: 0.75rem;
    color: var(--muted-foreground);
    font-variant-numeric: tabular-nums;
  }
  .slider {
    width: 100%;
    accent-color: var(--primary);
    cursor: default;
  }
  .toggle {
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    accent-color: var(--primary);
    cursor: default;
  }
  .hint {
    font-size: 0.7rem;
    color: var(--muted-foreground);
  }
  .env-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .status {
    min-height: 0.95rem;
    font-size: 0.7rem;
  }
  .saved {
    color: hsl(140 60% 40%);
  }
  .err {
    font-size: 0.72rem;
    color: var(--destructive);
    overflow-wrap: anywhere;
  }
  input,
  textarea {
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-sans);
    outline: none;
  }
  input:focus,
  textarea:focus {
    border-color: var(--ring);
  }
  textarea {
    resize: vertical;
    font-family: var(--font-mono);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.28rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.78rem;
    cursor: default;
  }
  .btn:hover {
    background: var(--accent);
  }
  .btn:disabled,
  .btn:disabled:hover {
    opacity: 0.5;
    cursor: default;
    background: var(--background);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
  }
  .spacer {
    flex: 1;
  }
</style>