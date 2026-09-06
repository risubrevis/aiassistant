<script lang="ts">
  import { onMount } from "svelte";
  import { Plus, Trash2, Pencil, X } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { skillsList, skillsSave, type Skill } from "$lib/tauri";

  let skills = $state<Skill[]>([]);
  let modalOpen = $state(false);
  let editingId = $state<string | null>(null);
  let deleteIndex = $state<number | null>(null);
  let modalError = $state("");
  let formTitle = $state("");
  let formBody = $state("");

  let canSave = $derived(formTitle.trim().length > 0 && formBody.trim().length > 0);
  let modalTitle = $derived(
    editingId === null ? m.settings_skills_add() : m.settings_skills_edit(),
  );

  onMount(() => {
    void load();
  });

  async function load() {
    try {
      skills = await skillsList();
    } catch (e) {
      console.error("skillsList failed", e);
    }
  }

  function openAdd() {
    editingId = null;
    formTitle = "";
    formBody = "";
    modalError = "";
    modalOpen = true;
  }

  function openEdit(index: number) {
    editingId = skills[index].id;
    formTitle = skills[index].title;
    formBody = skills[index].body;
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

  async function saveSkill() {
    const title = formTitle.trim();
    const body = formBody.trim();
    if (!title || !body) return;
    const updated = [...skills];
    if (editingId === null) {
      updated.push({ id: crypto.randomUUID(), title, body });
    } else {
      const idx = updated.findIndex((s) => s.id === editingId);
      if (idx >= 0) updated[idx] = { ...updated[idx], title, body };
    }
    try {
      await skillsSave(updated);
      skills = updated;
      modalOpen = false;
    } catch (e) {
      modalError = String(e);
    }
  }

  async function confirmDelete() {
    if (deleteIndex === null) return;
    const updated = skills.filter((_, i) => i !== deleteIndex);
    try {
      await skillsSave(updated);
      skills = updated;
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
      <span class="sec-title">{m.settings_skills_title()}</span>
      <button class="btn" onclick={openAdd}>
        <Plus size={13} /> {m.settings_skills_add()}
      </button>
    </div>
    {#if skills.length === 0}
      <div class="empty">{m.settings_skills_empty()}</div>
    {:else}
      <div class="rules">
        {#each skills as skill, i (skill.id)}
          <div class="rule-row">
            <div class="rule-main">
              <span class="rule-title">{skill.title}</span>
              <span class="rule-preview" title={skill.body}>{skill.body}</span>
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
          <span class="lbl">{m.settings_skills_field_title()}</span>
          <input bind:value={formTitle} />
        </label>
        <label class="field">
          <span class="lbl">{m.settings_skills_field_body()}</span>
          <textarea bind:value={formBody} rows="8" spellcheck="false"></textarea>
        </label>
        {#if modalError}<div class="err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={closeModals}>{m.common_cancel()}</button>
        <button class="btn primary" disabled={!canSave} onclick={() => void saveSkill()}>
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
        <div class="confirm">{m.settings_skills_delete_confirm()}</div>
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