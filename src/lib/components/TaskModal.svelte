<script lang="ts">
  import { ListTodo, Trash2, X } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import Select from "./Select.svelte";
  import RichTextEditor from "./RichTextEditor.svelte";
  import {
    createProjectTask,
    updateProjectTask,
    deleteProjectTask,
    TASK_COLUMNS,
    TASK_PRIORITIES,
  } from "$lib/stores/projectTasks";
  import type { ProjectTask, ProjectTaskStatus, ProjectTaskPriority } from "$lib/tauri";

  let {
    open,
    projectId,
    task = null,
    onclose,
  }: {
    open: boolean;
    projectId: string;
    task?: ProjectTask | null;
    onclose: () => void;
  } = $props();

  let title = $state("");
  let description = $state("");
  let status: ProjectTaskStatus = $state("backlog");
  let priority: ProjectTaskPriority = $state("medium");
  let saving = $state(false);

  let canSave = $derived(title.trim().length > 0 && !saving);

  function columnLabel(value: string): string {
    switch (value) {
      case "todo":
        return m.board_column_todo();
      case "in_progress":
        return m.board_column_in_progress();
      case "review":
        return m.board_column_review();
      case "done":
        return m.board_column_done();
      default:
        return m.board_column_backlog();
    }
  }

  function priorityLabel(value: string): string {
    switch (value) {
      case "medium":
        return m.ptask_priority_medium();
      case "high":
        return m.ptask_priority_high();
      case "urgent":
        return m.ptask_priority_urgent();
      default:
        return m.ptask_priority_low();
    }
  }

  const columnItems = $derived(TASK_COLUMNS.map((c) => ({ value: c, label: columnLabel(c) })));
  const priorityItems = $derived(
    TASK_PRIORITIES.map((p) => ({ value: p, label: priorityLabel(p) })),
  );

  // Reset form state whenever the modal opens or the edited task changes.
  $effect(() => {
    if (!open) return;
    title = task?.title ?? "";
    description = task?.description ?? "";
    status = task?.status ?? "backlog";
    priority = task?.priority ?? "medium";
  });

  async function save() {
    const trimmed = title.trim();
    if (!trimmed || saving) return;
    saving = true;
    try {
      if (task) {
        await updateProjectTask(projectId, task.id, trimmed, description, status, priority);
      } else {
        await createProjectTask(projectId, trimmed, description, status, priority);
      }
      onclose();
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!task || !confirm(m.ptask_delete_confirm())) return;
    await deleteProjectTask(projectId, task.id);
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
        <ListTodo size={15} />
        <span class="name">{task ? m.ptask_edit() : m.ptask_create()}</span>
        <button class="x" title={m.common_close()} onclick={onclose}><X size={15} /></button>
      </header>
      <div class="body">
        <div class="field">
          <span class="lbl">{m.ptask_title()}</span>
          <input
            class="input"
            type="text"
            bind:value={title}
            placeholder={m.ptask_title_placeholder()}
            spellcheck="false"
          />
        </div>
        <div class="field">
          <RichTextEditor
            bind:value={description}
            placeholder={m.ptask_description()}
            minHeight="160px"
          />
        </div>
        <div class="row">
          <div class="field">
            <span class="lbl">{m.ptask_status()}</span>
            <Select
              value={status}
              items={columnItems}
              onchange={(v) => (status = v as ProjectTaskStatus)}
            />
          </div>
          <div class="field">
            <span class="lbl">{m.ptask_priority()}</span>
            <Select
              value={priority}
              items={priorityItems}
              onchange={(v) => (priority = v as ProjectTaskPriority)}
            />
          </div>
        </div>
      </div>
      <footer class="foot">
        {#if task}
          <button class="btn danger" disabled={saving} onclick={() => void remove()}>
            <Trash2 size={13} /> {m.ptask_delete()}
          </button>
        {/if}
        <span class="spacer"></span>
        <button class="btn" onclick={onclose}>{m.ptask_cancel()}</button>
        <button class="btn primary" disabled={!canSave} onclick={() => void save()}>
          {task ? m.ptask_save() : m.ptask_create()}
        </button>
      </footer>
    </div>
  </div>
{/if}

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