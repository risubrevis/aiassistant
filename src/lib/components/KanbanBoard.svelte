<script lang="ts">
  import TaskCard from "./TaskCard.svelte";
  import { TASK_COLUMNS, moveProjectTask } from "$lib/stores/projectTasks";
  import { m } from "$lib/i18n";
  import type { ProjectTask, ProjectTaskStatus } from "$lib/tauri";

  let {
    projectId,
    tasks,
    onedit,
    onrun,
    onstop,
    onopenchat,
  }: {
    projectId: string;
    tasks: ProjectTask[];
    onedit: (task: ProjectTask) => void;
    onrun: (id: string) => void;
    onstop: (chatId: string) => void;
    onopenchat: (chatId: string) => void;
  } = $props();

  let dragTask: ProjectTask | null = $state(null);
  let dragOverStatus: ProjectTaskStatus | null = $state(null);
  let dragOverIndex: number | null = $state(null);

  function columnLabel(status: ProjectTaskStatus): string {
    switch (status) {
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

  function columnTasks(status: ProjectTaskStatus): ProjectTask[] {
    return tasks.filter((t) => t.status === status).sort((a, b) => a.position - b.position);
  }

  // Insert index within the target column excluding the dragged task itself,
  // matching the optimistic reorder in moveProjectTask.
  function dropPosition(status: ProjectTaskStatus): number {
    const rendered = columnTasks(status);
    const column = rendered.filter((t) => t.id !== dragTask?.id);
    if (dragOverIndex == null || dragOverIndex >= rendered.length) return column.length;
    return rendered.slice(0, dragOverIndex).filter((t) => t.id !== dragTask?.id).length;
  }

  function onCardDragStart(e: DragEvent, task: ProjectTask) {
    dragTask = task;
    if (!e.dataTransfer) return;
    e.dataTransfer.setData("text/plain", task.id);
    e.dataTransfer.effectAllowed = "move";
  }

  function clearDrag() {
    dragTask = null;
    dragOverStatus = null;
    dragOverIndex = null;
  }

  function onDropColumn(status: ProjectTaskStatus) {
    if (dragTask) void moveProjectTask(projectId, dragTask.id, status, dropPosition(status));
    clearDrag();
  }
</script>

<div class="board" role="presentation" ondragend={clearDrag}>
  {#each TASK_COLUMNS as status (status)}
    {@const list = columnTasks(status)}
    <section class="column" class:over={!!dragTask && dragOverStatus === status}>
      <header class="colhead">
        <span class="label">{columnLabel(status)}</span>
        <span class="count">{list.length}</span>
      </header>
      <div
        class="colbody"
        role="presentation"
        ondragover={(e) => {
          e.preventDefault();
          if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
          dragOverStatus = status;
          dragOverIndex = null;
        }}
        ondrop={(e) => {
          e.preventDefault();
          onDropColumn(status);
        }}
      >
        {#each list as task, i (task.id)}
          <div
            class="cardwrap"
            role="presentation"
            ondragover={(e) => {
              e.preventDefault();
              e.stopPropagation();
              if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
              dragOverStatus = status;
              dragOverIndex = i;
            }}
          >
            <TaskCard {task} {onrun} {onedit} {onstop} {onopenchat} ondragstart={onCardDragStart} />
          </div>
        {:else}
          <div class="placeholder"></div>
        {/each}
      </div>
    </section>
  {/each}
</div>

<style>
  .board {
    display: flex;
    align-items: stretch;
    gap: 0.5rem;
    height: 100%;
    min-height: 0;
    overflow-x: auto;
  }
  .column {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 220px;
    min-height: 0;
  }
  .colhead {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-shrink: 0;
    padding: 0.25rem 0.35rem;
    font-size: 0.72rem;
    font-weight: 600;
    color: var(--foreground);
  }
  .count {
    padding: 0 0.4rem;
    border-radius: 9999px;
    background: var(--accent);
    color: var(--muted-foreground);
    font-size: 0.62rem;
    font-weight: 500;
    line-height: 1.15rem;
  }
  .colbody {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    flex: 1;
    min-height: 2.5rem;
    overflow-y: auto;
    padding: 0.4rem;
    border: 1px dashed transparent;
    border-radius: var(--radius-md);
  }
  .column.over .colbody {
    border-color: var(--muted-foreground);
    background: var(--accent);
  }
  .cardwrap {
    flex-shrink: 0;
  }
  .placeholder {
    height: 3.5rem;
    border: 1px dashed var(--border);
    border-radius: var(--radius-md);
  }
</style>