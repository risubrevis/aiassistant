<script lang="ts">
  import { tick } from "svelte";
  import { ListTodo, X } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { taskClear } from "$lib/tauri";
  import { tasksByChat, clearTasks, groupTasks } from "$lib/stores/tasks";
  import { agentRuns } from "$lib/stores/agents";
  import TaskRow from "./TaskRow.svelte";

  let {
    open,
    chatId,
    onclose,
  }: {
    open: boolean;
    chatId: string;
    onclose: () => void;
  } = $props();

  let tasks = $derived($tasksByChat[chatId] ?? []);
  let groups = $derived(groupTasks(tasks));

  let listEl: HTMLDivElement | undefined = $state();

  function runLabel(runId: string): string {
    const run = $agentRuns.find((r) => r.id === runId);
    if (!run) return m.tasks_section_agent();
    const sub =
      run.subtask_index != null ? ` · ${m.tasks_subtask()} ${run.subtask_index + 1}` : "";
    return `${m.tasks_section_agent()}${sub}`;
  }

  function runPrompt(runId: string): string {
    return $agentRuns.find((r) => r.id === runId)?.subtask_prompt ?? "";
  }

  async function onClear() {
    try {
      await taskClear(chatId);
    } catch (e) {
      console.warn("taskClear failed", e);
    }
    clearTasks(chatId);
  }

  // Keep the first running task (across all sections) in view when the modal
  // opens or tasks change.
  $effect(() => {
    if (!open) return;
    void tasks;
    void tick().then(() => {
      const running = listEl?.querySelector<HTMLElement>(".task.in_progress");
      if (running) running.scrollIntoView({ block: "nearest" });
    });
  });

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">
        <ListTodo size={15} />
        <span class="name">{m.tasks_title()}</span>
        <button class="x" title={m.common_close()} onclick={onclose}><X size={15} /></button>
      </header>
      <div class="body" bind:this={listEl}>
        {#if tasks.length === 0}
          <div class="empty">{m.tasks_empty()}</div>
        {:else}
          {#if groups.internal.length > 0}
            <section class="group">
              <div class="sec-head">{m.tasks_section_plan()}</div>
              <div class="rows">
                {#each groups.internal as t (t.id)}
                  <TaskRow task={t} />
                {/each}
              </div>
            </section>
          {/if}
          {#each groups.agentRuns as g (g.runId)}
            <section class="group">
              <div class="sec-head agent">
                <span class="sec-label">{runLabel(g.runId)}</span>
                {#if runPrompt(g.runId)}
                  <span class="sec-prompt" title={runPrompt(g.runId)}>{runPrompt(g.runId)}</span>
                {/if}
              </div>
              <div class="rows">
                {#each g.tasks as t (t.id)}
                  <TaskRow task={t} />
                {/each}
              </div>
            </section>
          {/each}
        {/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" disabled={tasks.length === 0} onclick={() => void onClear()}>
          {m.tasks_clear()}
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
    width: 480px;
    max-width: 92vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
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
    padding: 0.5rem 0.875rem;
  }
  .group {
    padding: 0.4rem 0;
  }
  .group + .group {
    border-top: 1px solid var(--border);
  }
  .sec-head {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    font-size: 0.72rem;
    font-weight: 600;
    margin-bottom: 0.3rem;
    min-width: 0;
  }
  .sec-head.agent .sec-label {
    color: hsl(260 70% 60%);
  }
  .sec-prompt {
    flex: 1;
    min-width: 0;
    font-weight: 400;
    color: var(--muted-foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 1.5rem 0.5rem;
    color: var(--muted-foreground);
    font-size: 0.78rem;
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
  .btn:hover {
    background: var(--accent);
  }
  .btn:disabled {
    opacity: 0.5;
  }
</style>