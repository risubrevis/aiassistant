<script lang="ts">
  import KanbanBoard from "./KanbanBoard.svelte";
  import TaskListView from "./TaskListView.svelte";
  import TaskModal from "./TaskModal.svelte";
  import { tasksByProject, loadProjectTasks, runProjectTask } from "$lib/stores/projectTasks";
  import { openChat } from "$lib/stores/chat";
  import { projects } from "$lib/stores/project";
  import { chatCancel } from "$lib/tauri";
  import { m } from "$lib/i18n";
  import type { ProjectTask } from "$lib/tauri";
  import { Plus, SquareKanban, List } from "@lucide/svelte";

  let { projectId }: { projectId: string } = $props();

  let viewMode = $state<"kanban" | "list">("kanban");
  let modalOpen = $state(false);
  let editingTask: ProjectTask | null = $state(null);

  let tasks = $derived($tasksByProject[projectId] ?? []);
  let title = $derived($projects.find((p) => p.id === projectId)?.name || m.board_title());

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