<script lang="ts">
  import { onMount } from "svelte";
  import { Plus, Trash2, Pencil, X, ArrowUp, ArrowDown } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import {
    globalRulesList,
    globalRuleCreate,
    globalRuleUpdate,
    globalRuleDelete,
    globalRuleSetActive,
    globalRuleReorder,
    type Rule,
  } from "$lib/tauri";
  import RichTextEditor from "./RichTextEditor.svelte";

  let rules = $state<Rule[]>([]);
  let error = $state("");

  let modalOpen = $state(false);
  let editingId = $state<string | null>(null);
  let deleteId = $state<string | null>(null);
  let modalError = $state("");
  let formTitle = $state("");
  let formText = $state("");

  let canSave = $derived(formText.trim().length > 0);
  let modalTitle = $derived(
    editingId === null ? m.settings_rules_add() : m.settings_rules_edit(),
  );

  onMount(() => {
    void load();
  });

  async function load() {
    try {
      rules = await globalRulesList();
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleActive(rule: Rule, active: boolean) {
    try {
      await globalRuleSetActive(rule.id, active);
      error = "";
    } catch (e) {
      error = String(e);
    }
    await load();
  }

  async function move(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= rules.length) return;
    const next = [...rules];
    [next[i], next[j]] = [next[j], next[i]];
    rules = next;
    try {
      await globalRuleReorder(next.map((r) => r.id));
      error = "";
    } catch (e) {
      error = String(e);
    }
    await load();
  }

  function openAdd() {
    editingId = null;
    formTitle = "";
    formText = "";
    modalError = "";
    modalOpen = true;
  }

  function openEdit(rule: Rule) {
    editingId = rule.id;
    formTitle = rule.title;
    formText = rule.text;
    modalError = "";
    modalOpen = true;
  }

  function closeModals() {
    modalOpen = false;
    deleteId = null;
  }

  async function saveRule() {
    const text = formText.trim();
    if (!text) return;
    const title = formTitle.trim();
    try {
      if (editingId === null) {
        await globalRuleCreate(title, text);
      } else {
        await globalRuleUpdate(editingId, title, text);
      }
      modalOpen = false;
      error = "";
      await load();
    } catch (e) {
      modalError = String(e);
    }
  }

  async function confirmDelete() {
    if (deleteId === null) return;
    try {
      await globalRuleDelete(deleteId);
      deleteId = null;
      error = "";
      await load();
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
      <span class="sec-title">{m.settings_rules_title()}</span>
      <button class="btn" onclick={openAdd}>
        <Plus size={13} /> {m.settings_rules_add()}
      </button>
    </div>
    {#if rules.length === 0}
      <div class="empty">{m.settings_rules_empty()}</div>
    {:else}
      <div class="rules">
        {#each rules as rule, i (rule.id)}
          <div class="rule-row">
            <div class="rule-main">
              <span class="rule-title">{rule.title}</span>
              <span class="rule-preview" title={rule.text}>{rule.text}</span>
            </div>
            <input
              class="toggle"
              type="checkbox"
              title={m.settings_rules_active()}
              checked={rule.is_active === 1}
              onchange={(e) => void toggleActive(rule, e.currentTarget.checked)}
            />
            <button
              class="icon-btn"
              disabled={i === 0}
              title="Move up"
              onclick={() => void move(i, -1)}
            >
              <ArrowUp size={13} />
            </button>
            <button
              class="icon-btn"
              disabled={i === rules.length - 1}
              title="Move down"
              onclick={() => void move(i, 1)}
            >
              <ArrowDown size={13} />
            </button>
            <button class="icon-btn" title={m.common_edit()} onclick={() => openEdit(rule)}>
              <Pencil size={13} />
            </button>
            <button
              class="icon-btn danger"
              title={m.common_delete()}
              onclick={() => (deleteId = rule.id)}
            >
              <Trash2 size={13} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
    <div class="status">
      {#if error}<span class="err">{error}</span>{/if}
    </div>
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
          <span class="lbl">{m.settings_rules_field_title()}</span>
          <input bind:value={formTitle} />
        </label>
        <div class="field">
          <span class="lbl">{m.settings_rules_field_text()}</span>
          <RichTextEditor bind:value={formText} minHeight="120px" />
        </div>
        {#if modalError}<div class="err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={closeModals}>{m.common_cancel()}</button>
        <button class="btn primary" disabled={!canSave} onclick={() => void saveRule()}>
          {m.common_save()}
        </button>
      </footer>
    </div>
  </div>
{/if}

{#if deleteId !== null}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog small"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <div class="body">
        <div class="confirm">{m.settings_rules_delete_confirm()}</div>
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
  .err {
    font-size: 0.72rem;
    color: var(--destructive);
    overflow-wrap: anywhere;
  }
  input {
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
  input:focus {
    border-color: var(--ring);
  }
  .toggle {
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    accent-color: var(--primary);
    cursor: default;
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
  .status {
    min-height: 0.95rem;
    font-size: 0.7rem;
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
  .icon-btn:disabled {
    opacity: 0.4;
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