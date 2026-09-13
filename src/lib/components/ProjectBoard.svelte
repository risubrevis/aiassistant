<script lang="ts">
  import KanbanBoard from "./KanbanBoard.svelte";
  import TaskListView from "./TaskListView.svelte";
  import TaskModal from "./TaskModal.svelte";
  import RunAllModal from "./RunAllModal.svelte";
  import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
  import { tasksByProject, loadProjectTasks, runProjectTask } from "$lib/stores/projectTasks";
  import { batchRuns, startBatchRun, cancelBatchRun, type BatchMode } from "$lib/stores/projectTasksBatch";
  import { openChat } from "$lib/stores/chat";
  import { projects } from "$lib/stores/project";
  import { chatCancel } from "$lib/tauri";
  import { m } from "$lib/i18n";
  import type { ProjectTask } from "$lib/tauri";
  import { Plus, SquareKanban, List, Play, Square, LoaderCircle } from "@lucide/svelte";

  let { projectId }: { projectId: string } = $props();

  let viewMode = $state<"kanban" | "list">("kanban");
  let modalOpen = $state(false);
  let editingTask: ProjectTask | null = $state(null);
  let runAllOpen = $state(false);
  let stopConfirmOpen = $state(false);

  let tasks = $derived($tasksByProject[projectId] ?? []);
  let title = $derived($projects.find((p) => p.id === projectId)?.name || m.board_title());
  let todoTasks = $derived(tasks.filter((t) => t.status === "todo").sort((a, b) => a.position - b.position));
  let batchRun = $derived($batchRuns[projectId]);
  let batchActive = $derived(!!batchRun?.active);

  $effect(() => {
    void loadProjectTasks(projectId);
  });

  function onAddTask() {
    editingTask = null;
    modalOpen = true;
  }

  function onEditTask(task: ProjectTask) {
    editingTask = task;
    modalOpen = true;
  }

  async function onrun(id: string) {
    const chat = await runProjectTask(projectId, id);
    if (chat) await openChat(chat.id);
  }

  async function onstop(chatId: string) {
    try {
      await chatCancel(chatId);
    } catch (e) {
      console.error("chatCancel failed", e);
    }
  }

  async function onopenchat(chatId: string) {
    await openChat(chatId);
  }

  function onRunAll(mode: BatchMode) {
    runAllOpen = false;
    void startBatchRun(projectId, mode);
  }

  function onClickRunAll() {
    if (batchActive) {
      stopConfirmOpen = true;
    } else {
      runAllOpen = true;
    }
  }

  function onConfirmStop() {
    stopConfirmOpen = false;
    void cancelBatchRun(projectId);
  }
</script>

<div class="wrap">
  <header class="head">
    <span class="title">{title}</span>
    <div class="toggle">
      <button class:sel={viewMode === "kanban"} onclick={() => (viewMode = "kanban")}>
        <SquareKanban size={13} />
        <span>{m.board_view_kanban()}</span>
      </button>
      <button class:sel={viewMode === "list"} onclick={() => (viewMode = "list")}>
        <List size={13} />
        <span>{m.board_view_list()}</span>
      </button>
    </div>
    <button
      class="runall"
      class:active={batchActive}
      disabled={!batchActive && todoTasks.length === 0}
      onclick={onClickRunAll}
      title={batchActive ? m.board_run_all_stop() : m.board_run_all_title()}
    >
      {#if batchActive}
        <LoaderCircle size={13} class="spin" />
        <Square size={12} class="stop-ico" />
        <span>{m.board_run_all_stop()}</span>
      {:else}
        <Play size={13} />
        <span>{m.board_run_all()}</span>
      {/if}
    </button>
    <button class="add" onclick={onAddTask}>
      <Plus size={13} />
      <span>{m.board_add_task()}</span>
    </button>
  </header>
  <div class="boardarea">
    {#if tasks.length === 0}
      <div class="empty">
        <p>{m.board_empty()}</p>
        <button class="add" onclick={onAddTask}>
          <Plus size={13} />
          <span>{m.board_add_task()}</span>
        </button>
      </div>
    {:else if viewMode === "kanban"}
      <KanbanBoard {projectId} {tasks} onedit={onEditTask} {onrun} {onstop} {onopenchat} />
    {:else}
      <TaskListView {projectId} {tasks} onedit={onEditTask} {onrun} {onstop} {onopenchat} />
    {/if}
  </div>
</div>
<TaskModal open={modalOpen} {projectId} task={editingTask} onclose={() => (modalOpen = false)} />
<RunAllModal open={runAllOpen} todoCount={todoTasks.length} onrun={onRunAll} onclose={() => (runAllOpen = false)} />
<ConfirmDialog
  open={stopConfirmOpen}
  message={m.board_run_all_stop_confirm()}
  confirmLabel={m.board_run_all_stop()}
  variant="danger"
  onconfirm={onConfirmStop}
  oncancel={() => (stopConfirmOpen = false)}
/>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-shrink: 0;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--foreground);
  }
  .toggle {
    display: inline-flex;
    margin-left: auto;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
  .toggle button {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.25rem 0.6rem;
    border: none;
    background: transparent;
    color: var(--muted-foreground);
    font-size: 0.72rem;
    cursor: pointer;
  }
  .toggle button.sel {
    background: var(--accent);
    color: var(--foreground);
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    flex-shrink: 0;
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--foreground);
    font-size: 0.72rem;
    cursor: pointer;
  }
  .add:hover {
    background: var(--accent);
  }
  .runall {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    flex-shrink: 0;
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--foreground);
    font-size: 0.72rem;
    cursor: pointer;
  }
  .runall:hover:not(:disabled) {
    background: var(--accent);
  }
  .runall:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .runall.active {
    border-color: var(--destructive);
    color: var(--destructive);
  }
  .runall :global(.stop-ico) {
    color: var(--destructive);
  }
  .runall :global(.spin) {
    animation: runall-spin 1s linear infinite;
  }
  @keyframes runall-spin {
    to {
      transform: rotate(360deg);
    }
  }
  .boardarea {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0.5rem;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    flex: 1;
    color: var(--muted-foreground);
    font-size: 0.8rem;
    text-align: center;
  }
  .empty p {
    margin: 0;
  }
</style>