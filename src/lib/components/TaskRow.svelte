<script lang="ts">
  import { Circle, Loader, Check, XCircle } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import type { TaskItem, TaskStatus } from "$lib/tauri";

  let { task }: { task: TaskItem } = $props();

  let Icon = $derived(statusIcon(task.status));

  function statusIcon(status: TaskStatus) {
    if (status === "completed") return Check;
    if (status === "in_progress") return Loader;
    if (status === "cancelled") return XCircle;
    return Circle;
  }

  function statusLabel(status: TaskStatus): string {
    switch (status) {
      case "in_progress":
        return m.tasks_status_in_progress();
      case "completed":
        return m.tasks_status_completed();
      case "cancelled":
        return m.tasks_status_cancelled();
      default:
        return m.tasks_status_pending();
    }
  }

  function rowText(t: TaskItem): string {
    return t.status === "in_progress" && t.active_form ? t.active_form : t.content;
  }
</script>

<div class="task {task.status}">
  <span class="ico"><Icon size={13} /></span>
  <span class="content">{rowText(task)}</span>
  <span class="label">{statusLabel(task.status)}</span>
</div>

<style>
  .task {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    font-size: 0.8rem;
    color: var(--foreground);
    padding: 0.2rem 0.25rem;
    border-radius: var(--radius-sm);
  }
  .task .ico {
    display: inline-flex;
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .task.in_progress {
    color: hsl(217 91% 60%);
  }
  .task.in_progress .ico {
    color: hsl(217 91% 60%);
  }
  .task.in_progress .ico :global(svg) {
    animation: spin 1s linear infinite;
  }
  .task.completed,
  .task.cancelled {
    color: var(--muted-foreground);
  }
  .task.completed .ico {
    color: hsl(140 60% 40%);
  }
  .task.completed .content,
  .task.cancelled .content {
    text-decoration: line-through;
  }
  .content {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .label {
    flex-shrink: 0;
    font-size: 0.68rem;
    color: var(--muted-foreground);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>