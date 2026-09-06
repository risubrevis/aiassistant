<script lang="ts">
  import { Play, Square, MessageSquare } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { statusByChat } from "$lib/stores/chat";
  import type { ProjectTask, ProjectTaskPriority } from "$lib/tauri";

  let {
    task,
    onrun,
    onedit,
    onstop,
    onopenchat,
    draggable = true,
    ondragstart,
  }: {
    task: ProjectTask;
    onrun: (id: string) => void;
    onedit: (task: ProjectTask) => void;
    onstop: (chatId: string) => void;
    onopenchat: (chatId: string) => void;
    draggable?: boolean;
    ondragstart?: (e: DragEvent, task: ProjectTask) => void;
  } = $props();

  let running = $derived(!!task.chat_id && $statusByChat[task.chat_id] === "running");
  let linked = $derived(!!task.chat_id);

  const priorityColors: Record<ProjectTaskPriority, string> = {
    low: "#9ca3af",
    medium: "#3b82f6",
    high: "#f59e0b",
    urgent: "#ef4444",
  };

  function priorityLabel(priority: ProjectTaskPriority): string {
    switch (priority) {
      case "urgent":
        return m.ptask_priority_urgent();
      case "high":
        return m.ptask_priority_high();
      case "medium":
        return m.ptask_priority_medium();
      default:
        return m.ptask_priority_low();
    }
  }
</script>

<div
  class="card"
  draggable={draggable}
  role="button"
  tabindex="0"
  onclick={() => onedit(task)}
  onkeydown={(e) => e.key === "Enter" && onedit(task)}
  ondragstart={(e) => ondragstart?.(e, task)}
>
  <div class="top">
    <span class="priority" style={`--pc: ${priorityColors[task.priority]}`}>
      <span class="dot"></span>
      <span class="plabel">{priorityLabel(task.priority)}</span>
    </span>
    <span class="title">{task.title}</span>
  </div>
  <div class="bottom">
    {#if running}
      <span class="status running">{m.ptask_running()}</span>
    {:else if !linked}
      <span class="status">{m.ptask_no_chat()}</span>
    {/if}
    <div class="actions">
      {#if !linked}
        <button
          class="act run"
          title={m.ptask_run()}
          onclick={(e) => {
            e.stopPropagation();
            onrun(task.id);
          }}
          onmousedown={(e) => e.stopPropagation()}
        >
          <Play size={13} />
        </button>
      {:else if running}
        <button
          class="act stop"
          title={m.ptask_stop()}
          onclick={(e) => {
            e.stopPropagation();
            onstop(task.chat_id!);
          }}
          onmousedown={(e) => e.stopPropagation()}
        >
          <Square size={12} />
        </button>
      {:else}
        <button
          class="act open"
          title={m.ptask_open_chat()}
          onclick={(e) => {
            e.stopPropagation();
            onopenchat(task.chat_id!);
          }}
          onmousedown={(e) => e.stopPropagation()}
        >
          <MessageSquare size={13} />
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    padding: 0.5rem 0.625rem;
    font-size: 0.8rem;
    color: var(--foreground);
    cursor: default;
    user-select: none;
  }
  .card:hover {
    border-color: var(--muted-foreground);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
  }
  .priority {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    flex-shrink: 0;
    font-size: 0.65rem;
  }
  .priority .dot {
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 9999px;
    background: var(--pc);
  }
  .priority .plabel {
    color: var(--muted-foreground);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bottom {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.68rem;
  }
  .status {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted-foreground);
  }
  .status.running {
    color: #22c55e;
  }
  .actions {
    display: inline-flex;
    gap: 0.25rem;
    flex-shrink: 0;
    margin-left: auto;
  }
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.4rem;
    height: 1.4rem;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted-foreground);
    cursor: pointer;
  }
  .act:hover {
    background: var(--accent);
  }
  .act.run {
    color: #22c55e;
  }
  .act.stop {
    color: #ef4444;
  }
</style>