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

  // Insert index within the target section excluding the dragged task itself,
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

  function onDropSection(status: ProjectTaskStatus) {
    if (dragTask) void moveProjectTask(projectId, dragTask.id, status, dropPosition(status));
    clearDrag();
  }
</script>

<div class="list" role="presentation" ondragend={clearDrag}>
  {#each TASK_COLUMNS as status (status)}
    {@const list = columnTasks(status)}
    <section class="section" class:over={!!dragTask && dragOverStatus === status}>
      <header class="sechead">
        <span class="label">{columnLabel(status)}</span>
        <span class="count">{list.length}</span>
      </header>
      <div
        class="body"
        role="presentation"
        ondragover={(e) => {
          e.preventDefault();
          if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
          dragOverStatus = status;
          dragOverIndex = null;
        }}
        ondrop={(e) => {
          e.preventDefault();
          onDropSection(status);
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
  .list {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .sechead {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.25rem 0.35rem;
    border-bottom: 1px solid var(--border);
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
  .body {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.4rem;
    border-radius: var(--radius-md);
  }
  .section.over .body {
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