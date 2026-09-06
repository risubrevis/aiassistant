<script lang="ts">
  import { onMount } from "svelte";
  import { Plus, Trash2, Pencil, X } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import {
    configGet,
    setSystemPrompt,
    setAutoCollapseContextPct,
    setAutoPullChanges,
    globalRulesList,
    globalRulesSave,
    environmentGet,
    environmentSave,
    environmentDetect,
    setAddEnvironmentInfo,
    type GlobalRule,
  } from "$lib/tauri";

  let systemPrompt = $state("");
  let collapsePct = $state(90);
  let autoPull = $state(true);
  let rulesList = $state<GlobalRule[]>([]);
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

  let modalOpen = $state(false);
  let editingIndex = $state<number | null>(null);
  let deleteIndex = $state<number | null>(null);
  let modalError = $state("");
  let formTitle = $state("");
  let formText = $state("");

  let savedTimer: ReturnType<typeof setTimeout> | undefined;
  let envSavedTimer: ReturnType<typeof setTimeout> | undefined;

  let canSaveRule = $derived(formText.trim().length > 0);
  let envDirty = $derived(envInfo !== envInfoSaved);
  let modalTitle = $derived(
    editingIndex === null
      ? m.settings_prompts_add_rule()
      : m.settings_prompts_edit_rule(),
  );

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
      rulesList = await globalRulesList();
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

  function openAdd() {
    editingIndex = null;
    formTitle = "";
    formText = "";
    modalError = "";
    modalOpen = true;
  }

  function openEdit(index: number) {
    editingIndex = index;
    formTitle = rulesList[index].title;
    formText = rulesList[index].text;
    modalError = "";
    modalOpen = true;
  }

  function openDelete(index: number) {
    deleteIndex = index;
    modalError = "";
  }

  function closeModals() {
    modalOpen = false;
    deleteIndex = null;
  }

  async function saveRule() {
    const text = formText.trim();
    if (!text) return;
    const title = formTitle.trim();
    const updated = [...rulesList];
    if (editingIndex === null) {
      updated.push({ title, text, enabled: true });
    } else {
      updated[editingIndex] = { ...rulesList[editingIndex], title, text };
    }
    try {
      await globalRulesSave(updated);
      rulesList = updated;
      modalOpen = false;
    } catch (e) {
      modalError = String(e);
    }
  }

  async function confirmDelete() {
    if (deleteIndex === null) return;
    const updated = rulesList.filter((_, i) => i !== deleteIndex);
    try {
      await globalRulesSave(updated);
      rulesList = updated;
      deleteIndex = null;
    } catch (e) {
      modalError = String(e);
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    closeModals();
  }
</script>

<svelte:window onkeydown={onKeydown} />

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

  <section class="section">
    <div class="sec-head">
      <span class="sec-title">{m.settings_prompts_rules()}</span>
      <button class="btn" onclick={openAdd}>
        <Plus size={13} /> {m.settings_prompts_add_rule()}
      </button>
    </div>
    {#if rulesList.length === 0}
      <div class="empty">{m.settings_prompts_no_rules()}</div>
    {:else}
      <div class="rules">
        {#each rulesList as rule, i (i)}
          <div class="rule-row">
            <div class="rule-main">
              {#if rule.title}<span class="rule-title">{rule.title}</span>{/if}
              <span class="rule-preview" title={rule.text}>{rule.text}</span>
            </div>
            <button class="icon-btn" title={m.common_edit()} onclick={() => openEdit(i)}>
              <Pencil size={13} />
            </button>
            <button class="icon-btn danger" title={m.common_delete()} onclick={() => openDelete(i)}>
              <Trash2 size={13} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </section>
</div>

{#if modalOpen}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">
        <span class="head-title">{modalTitle}</span>
        <button class="x" title={m.common_close()} onclick={closeModals}><X size={14} /></button>
      </header>
      <div class="body">
        <label class="field">
          <span class="lbl">{m.settings_prompts_rule_title()}</span>
          <input bind:value={formTitle} />
        </label>
        <label class="field">
          <span class="lbl">{m.settings_prompts_rule_text()}</span>
          <textarea bind:value={formText} rows="5" spellcheck="false"></textarea>
        </label>
        {#if modalError}<div class="err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={closeModals}>{m.common_cancel()}</button>
        <button class="btn primary" disabled={!canSaveRule} onclick={() => void saveRule()}>
          {m.common_save()}
        </button>
      </footer>
    </div>
  </div>
{/if}

{#if deleteIndex !== null}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog small"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <div class="body">
        <div class="confirm">{m.settings_prompts_delete_confirm()}</div>
        {#if modalError}<div class="err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={closeModals}>{m.common_cancel()}</button>
        <button class="btn danger" onclick={() => void confirmDelete()}>
          {m.common_delete()}
        </button>
      </footer>
    </div>
  </div>
{/if}

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
  .rules {
    display: grid;
    grid-template-columns: 1fr;
    gap: 0.4rem;
  }
  .rule-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .rule-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .rule-title {
    font-size: 0.8125rem;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rule-preview {
    font-size: 0.72rem;
    color: var(--muted-foreground);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty {
    color: var(--muted-foreground);
    font-size: 0.78rem;
    padding: 0.25rem 0;
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
  .btn.primary {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover {
    filter: brightness(1.08);
  }
  .btn.primary:disabled,
  .btn.primary:disabled:hover {
    opacity: 0.5;
    filter: none;
    background: var(--primary);
  }
  .btn.danger {
    background: var(--destructive);
    border-color: var(--destructive);
    color: var(--destructive-foreground);
  }
  .btn.danger:hover {
    filter: brightness(0.92);
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 1.5rem;
    width: 1.5rem;
    flex-shrink: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted-foreground);
    cursor: default;
  }
  .icon-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .icon-btn.danger:hover {
    color: var(--destructive);
  }
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .dialog {
    width: 480px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
  }
  .dialog.small {
    width: 360px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
  }
  .head-title {
    font-size: 0.875rem;
    font-weight: 500;
  }
  .x {
    display: inline-flex;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .x:hover {
    background: var(--accent);
  }
  .body {
    padding: 0.875rem 1rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .confirm {
    font-size: 0.8125rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
  }
  .lbl {
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.55rem 0.875rem;
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
</style>