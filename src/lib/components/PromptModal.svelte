<script lang="ts">
  import { MessageSquareText, X, Trash2, Plus, Paperclip, Star, Check } from "@lucide/svelte";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
  import { m } from "$lib/i18n";
  import Select from "./Select.svelte";
  import RichTextEditor from "./RichTextEditor.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { createPrompt, updatePrompt, deletePrompt } from "$lib/stores/prompts";
  import { projects } from "$lib/stores/project";
  import { skills as skillsStore } from "$lib/stores/skills";
  import type { Prompt } from "$lib/tauri";

  let {
    open,
    prompt = null,
    onclose,
  }: {
    open: boolean;
    prompt?: Prompt | null;
    onclose: () => void;
  } = $props();

  let title = $state("");
  let body = $state("");
  let projectId: string | null = $state(null);
  let attachFiles = $state<string[]>([]);
  let selectedSkillIds = $state<string[]>([]);
  let isFavorite = $state(false);
  let saving = $state(false);
  let confirmDeleteOpen = $state(false);

  let canSave = $derived(title.trim().length > 0 && !saving);

  const projectItems = $derived([
    { value: "", label: m.prompt_project_none() },
    ...$projects.map((p) => ({ value: p.id, label: p.name })),
  ]);

  function basename(path: string): string {
    return path.split(/[/\\]/).pop() ?? path;
  }

  // Reset form state whenever the modal opens or the edited prompt changes.
  $effect(() => {
    if (!open) return;
    title = prompt?.title ?? "";
    body = prompt?.body ?? "";
    projectId = prompt?.project_id ?? null;
    attachFiles = prompt?.attach_files ? [...prompt.attach_files] : [];
    selectedSkillIds = prompt?.skill_ids ? [...prompt.skill_ids] : [];
    isFavorite = prompt?.is_favorite ?? false;
  });

  function removeFile(index: number) {
    attachFiles = attachFiles.filter((_, i) => i !== index);
  }

  async function addFiles() {
    try {
      const res = await openFileDialog({ multiple: true });
      if (!res) return;
      const arr = Array.isArray(res) ? res : [res];
      const next = new Set(attachFiles);
      for (const p of arr) next.add(p);
      attachFiles = [...next];
    } catch (e) {
      console.error("file dialog failed", e);
    }
  }

  function toggleSkill(id: string) {
    if (selectedSkillIds.includes(id)) {
      selectedSkillIds = selectedSkillIds.filter((x) => x !== id);
    } else {
      selectedSkillIds = [...selectedSkillIds, id];
    }
  }

  async function save() {
    const trimmed = title.trim();
    if (!trimmed || saving) return;
    saving = true;
    try {
      if (prompt) {
        await updatePrompt(prompt.id, trimmed, body, projectId, attachFiles, selectedSkillIds, isFavorite);
      } else {
        await createPrompt(trimmed, body, projectId, attachFiles, selectedSkillIds, isFavorite);
      }
      onclose();
    } finally {
      saving = false;
    }
  }

  function remove() {
    if (!prompt) return;
    confirmDeleteOpen = true;
  }

  async function confirmDelete() {
    if (!prompt) return;
    confirmDeleteOpen = false;
    await deletePrompt(prompt.id);
    onclose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="overlay" role="presentation">
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">
        <MessageSquareText size={15} />
        <span class="name">{prompt ? m.prompt_edit() : m.prompt_create()}</span>
        <button class="x" title={m.common_close()} onclick={onclose}><X size={15} /></button>
      </header>
      <div class="body">
        <div class="field">
          <span class="lbl">{m.prompt_title()}</span>
          <input
            class="input"
            type="text"
            bind:value={title}
            placeholder={m.prompt_title_placeholder()}
            spellcheck="false"
          />
        </div>
        <div class="field">
          <span class="lbl">{m.prompt_body()}</span>
          <RichTextEditor
            bind:value={body}
            placeholder={m.prompt_body_placeholder()}
            minHeight="160px"
          />
        </div>
        <div class="row">
          <div class="field">
            <span class="lbl">{m.prompt_project()}</span>
            <Select
              value={projectId ?? ""}
              items={projectItems}
              onchange={(v) => (projectId = v === "" ? null : v)}
            />
          </div>
          <div class="field">
            <span class="lbl">{m.prompt_is_favorite()}</span>
            <button class="btn fav" class:on={isFavorite} onclick={() => (isFavorite = !isFavorite)}>
              <Star size={13} fill={isFavorite ? "currentColor" : "none"} />
              <span>{m.prompt_is_favorite()}</span>
            </button>
          </div>
        </div>
        <div class="field">
          <span class="lbl">{m.prompt_attach_files()}</span>
          {#if attachFiles.length > 0}
            <div class="files">
              {#each attachFiles as f, i (f)}
                <div class="file-row" title={f}>
                  <Paperclip size={12} />
                  <span class="file-name">{basename(f)}</span>
                  <button class="file-x" title={m.attachment_remove()} onclick={() => removeFile(i)}>
                    <X size={12} />
                  </button>
                </div>
              {/each}
            </div>
          {/if}
          <button class="btn" onclick={() => void addFiles()}>
            <Plus size={13} /> {m.prompt_attach_files_add()}
          </button>
        </div>
        <div class="field">
          <span class="lbl">{m.prompt_skills()}</span>
          <div class="skills-list">
            {#if $skillsStore.length === 0}
              <div class="skills-empty">{m.prompt_no_skills()}</div>
            {:else}
              {#each $skillsStore as skill (skill.id)}
                <button
                  class="skill-option"
                  class:checked={selectedSkillIds.includes(skill.id)}
                  onclick={() => toggleSkill(skill.id)}
                  role="menuitemcheckbox"
                  aria-checked={selectedSkillIds.includes(skill.id)}
                >
                  <span class="skill-check">
                    {#if selectedSkillIds.includes(skill.id)}<Check size={13} />{/if}
                  </span>
                  <span class="skill-title">{skill.title}</span>
                </button>
              {/each}
            {/if}
          </div>
        </div>
      </div>
      <footer class="foot">
        {#if prompt}
          <button class="btn danger" disabled={saving} onclick={() => void remove()}>
            <Trash2 size={13} /> {m.prompt_delete()}
          </button>
        {/if}
        <span class="spacer"></span>
        <button class="btn" onclick={onclose}>{m.prompt_cancel()}</button>
        <button class="btn primary" disabled={!canSave} onclick={() => void save()}>
          {prompt ? m.prompt_save() : m.prompt_create()}
        </button>
      </footer>
    </div>
  </div>
{/if}

<ConfirmDialog
  open={confirmDeleteOpen}
  message={m.prompt_delete_confirm()}
  onconfirm={() => void confirmDelete()}
  oncancel={() => (confirmDeleteOpen = false)}
/>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .dialog {
    width: 560px;
    max-width: 92vw;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.28);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
    min-width: 0;
  }
  .head :global(svg) {
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .name {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    margin-left: auto;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
    padding: 0.15rem;
    flex-shrink: 0;
  }
  .x:hover {
    background-color: var(--accent);
    color: var(--accent-foreground);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 0.875rem;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.75rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    min-width: 0;
  }
  .lbl {
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }
  .input {
    width: 100%;
    height: 2rem;
    padding: 0 0.625rem;
    border: 1px solid var(--input);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    outline: none;
  }
  .input:focus {
    border-color: var(--ring);
  }
  .files {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .file-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.2rem 0.4rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 0.78rem;
    min-width: 0;
  }
  .file-row > :global(svg) {
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .file-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .file-x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: transparent;
    color: var(--muted-foreground);
    border-radius: var(--radius-sm);
    padding: 0.1rem;
    cursor: pointer;
    flex-shrink: 0;
  }
  .file-x:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .skills-list {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    max-height: 8.5rem;
    overflow-y: auto;
    padding: 0.25rem;
    border: 1px solid var(--input);
    border-radius: var(--radius-md);
  }
  .skills-empty {
    padding: 0.25rem 0.35rem;
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .skill-option {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.25rem 0.35rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--foreground);
    font-size: 0.8125rem;
    text-align: left;
    cursor: default;
  }
  .skill-option:hover {
    background: var(--accent);
  }
  .skill-option.checked {
    background: var(--secondary);
  }
  .skill-check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    color: var(--primary);
  }
  .skill-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.78rem;
    cursor: default;
  }
  .btn:hover:not(:disabled) {
    background: var(--accent);
  }
  .btn.fav.on {
    color: #f59e0b;
  }
  .btn.primary {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover:not(:disabled) {
    background: var(--primary);
    opacity: 0.9;
  }
  .btn.danger {
    color: var(--destructive);
    border-color: var(--destructive);
  }
  .btn.danger:hover:not(:disabled) {
    background: var(--destructive);
    color: var(--destructive-foreground);
  }
  .btn:disabled {
    opacity: 0.5;
  }
</style>