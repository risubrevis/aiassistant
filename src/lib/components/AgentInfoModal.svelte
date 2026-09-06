<script lang="ts">
  import { Bot, X, Square, LoaderCircle } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import type { AgentRun } from "$lib/tauri";
  import { agents, agentRuns, agentInfoModalOpen, cancelAgentRunsInChat } from "$lib/stores/agents";

  let { chatId }: { chatId: string } = $props();

  const ACTIVE_RUN_STATUSES = new Set(["queued", "running"]);

  let availableAgents = $derived($agents.filter((a) => a.row.is_active && a.status === "ok"));

  function runsForAgent(agentId: string): AgentRun[] {
    return $agentRuns.filter(
      (r) =>
        r.chat_id === chatId &&
        r.agent_connection_id === agentId &&
        ACTIVE_RUN_STATUSES.has(r.status),
    );
  }

  let hasRunning = $derived(availableAgents.some((a) => runsForAgent(a.row.id).length > 0));

  let now = $state(Date.now());
  let timerHandle: ReturnType<typeof setInterval> | undefined;

  $effect(() => {
    if (!$agentInfoModalOpen) return;
    if (hasRunning && timerHandle === undefined) {
      timerHandle = setInterval(() => {
        now = Date.now();
      }, 1000);
    } else if (!hasRunning && timerHandle !== undefined) {
      clearInterval(timerHandle);
      timerHandle = undefined;
    }
    return () => {
      if (timerHandle !== undefined) {
        clearInterval(timerHandle);
        timerHandle = undefined;
      }
    };
  });

  function timerText(runs: AgentRun[]): string {
    const earliest = [...runs].sort(
      (a, b) => (a.started_at ?? Infinity) - (b.started_at ?? Infinity),
    )[0];
    if (earliest.started_at === null) return "…";
    return formatElapsed(Math.max(0, now - earliest.started_at));
  }

  function formatElapsed(ms: number): string {
    const totalSec = Math.floor(ms / 1000);
    const h = Math.floor(totalSec / 3600);
    const min = Math.floor((totalSec % 3600) / 60);
    const s = totalSec % 60;
    if (h > 0) return `${h}h ${min}m ${s}s`;
    if (min > 0) return `${min}m ${s}s`;
    return `${s}s`;
  }

  let stopping = $state<Set<string>>(new Set());

  async function stopAgent(agentId: string) {
    stopping = new Set(stopping).add(agentId);
    try {
      await cancelAgentRunsInChat(agentId, chatId);
    } finally {
      const next = new Set(stopping);
      next.delete(agentId);
      stopping = next;
    }
  }

  function close() {
    agentInfoModalOpen.set(false);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if $agentInfoModalOpen}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">
        <Bot size={15} />
        <span class="name">{m.agent_info_title()}</span>
        <button class="x" title={m.common_close()} onclick={close}><X size={15} /></button>
      </header>
      <div class="body">
        {#if availableAgents.length === 0}
          <div class="empty">{m.agent_no_agent()}</div>
        {:else}
          <div class="alist">
            {#each availableAgents as agent (agent.row.id)}
              {@const runs = runsForAgent(agent.row.id)}
              <div class="arow" class:active={runs.length > 0}>
                <span class="dot" class:on={runs.length > 0}></span>
                <div class="amain">
                  <div class="aname">{agent.row.name}</div>
                  {#if agent.row.description}
                    <div class="adesc">{agent.row.description}</div>
                  {/if}
                </div>
                {#if runs.length > 0}
                  <div class="aside">
                    {#if runs.length > 1}
                      <span class="rcount">{m.agents_runs_count({ n: runs.length })}</span>
                    {/if}
                    <span class="timer">{timerText(runs)}</span>
                    <button
                      class="stop"
                      title={stopping.has(agent.row.id) ? m.agents_stopping() : m.agents_stop()}
                      disabled={stopping.has(agent.row.id)}
                      onclick={() => void stopAgent(agent.row.id)}
                    >
                      {#if stopping.has(agent.row.id)}
                        <LoaderCircle size={12} class="spin" />
                        <span>{m.agents_stopping()}</span>
                      {:else}
                        <Square size={12} />
                        <span>{m.agents_stop()}</span>
                      {/if}
                    </button>
                  </div>
                {:else}
                  <span class="idle">{m.agents_idle()}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={close}>{m.common_close()}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
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
    width: 560px;
    max-width: 92vw;
    max-height: 82vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
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
    flex-shrink: 0;
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
    padding: 0.75rem 0.875rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .alist {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }
  .arow {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.5rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--muted);
    min-width: 0;
  }
  .arow.active {
    border-color: hsl(140 50% 50% / 0.35);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 9999px;
    background: var(--muted-foreground);
    opacity: 0.55;
    flex-shrink: 0;
  }
  .dot.on {
    background: hsl(140 50% 50%);
    opacity: 1;
    animation: pulse 1.2s infinite;
  }
  .amain {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .aname {
    font-size: 0.8125rem;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .adesc {
    font-size: 0.72rem;
    color: var(--muted-foreground);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .aside {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    flex-shrink: 0;
  }
  .rcount {
    font-size: 0.68rem;
    color: var(--muted-foreground);
    white-space: nowrap;
  }
  .timer {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: hsl(140 50% 50%);
    white-space: nowrap;
  }
  .idle {
    font-size: 0.68rem;
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .stop {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.2rem 0.5rem;
    border: 1px solid hsl(0 65% 58% / 0.4);
    border-radius: var(--radius-sm);
    background: transparent;
    color: hsl(0 65% 58%);
    font-size: 0.7rem;
    cursor: default;
    flex-shrink: 0;
  }
  .stop:hover:not(:disabled) {
    background: hsl(0 65% 58% / 0.12);
  }
  .stop:disabled {
    opacity: 0.55;
  }
  .stop :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .empty {
    color: var(--muted-foreground);
    font-size: 0.78rem;
    padding: 0.5rem 0;
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
</style>