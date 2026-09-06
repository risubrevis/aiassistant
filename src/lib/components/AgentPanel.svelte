<script lang="ts">
  import {
    Bot,
    Loader,
    Check,
    X,
    AlertTriangle,
    Clock,
    GitMerge,
    Circle,
    ChevronDown,
    Trash2,
  } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import type { AgentRun } from "$lib/tauri";
  import {
    agentRuns,
    agentsPanelOpen,
    agentModalRunId,
    runningCount,
    cancelRun,
    approveRun,
    rejectRun,
  } from "$lib/stores/agents";

  function icon(status: string) {
    switch (status) {
      case "running": return Loader;
      case "done": return Check;
      case "error": return AlertTriangle;
      case "timeout": return Clock;
      case "cancelled": return Circle;
      case "merged": return GitMerge;
      default: return Circle;
    }
  }

  function statusLabel(status: string): string {
    switch (status) {
      case "running": return m.agents_status_running();
      case "done": return m.agents_status_done();
      case "error": return m.agents_status_error();
      case "timeout": return m.agents_status_timeout();
      case "cancelled": return m.agents_status_cancelled();
      case "merged": return m.agents_status_merged();
      case "queued": return m.agents_status_queued();
      default: return status;
    }
  }

  function truncate(s: string, n = 60): string {
    return s.length > n ? s.slice(0, n - 1) + "…" : s;
  }

  function elapsed(r: AgentRun): string {
    if (r.started_at == null || r.ended_at == null) return "—";
    const ms = Math.max(0, r.ended_at - r.started_at);
    if (ms < 1000) return `${ms}ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
    const sec = Math.round(ms / 1000);
    return `${Math.floor(sec / 60)}m ${String(sec % 60).padStart(2, "0")}s`;
  }

  function open(runId: string) {
    agentModalRunId.set(runId);
  }

  let busyRun = $state<Record<string, "approve" | "reject">>({});

  async function handleApproveRun(runId: string) {
    if (busyRun[runId]) return;
    busyRun = { ...busyRun, [runId]: "approve" };
    try { await approveRun(runId); } catch (e) { console.error(e); }
    const next = { ...busyRun }; delete next[runId]; busyRun = next;
  }

  async function handleRejectRun(runId: string) {
    if (busyRun[runId]) return;
    busyRun = { ...busyRun, [runId]: "reject" };
    try { await rejectRun(runId); } catch (e) { console.error(e); }
    const next = { ...busyRun }; delete next[runId]; busyRun = next;
  }
</script>

{#if $agentsPanelOpen}
  <div class="panel">
    <div class="head">
      <Bot size={13} />
      <span class="t">{m.agents_panel_title()}</span>
      {#if $runningCount > 0}
        <span class="count">{$runningCount}</span>
      {/if}
      <button class="fold" title={m.common_close()} onclick={() => agentsPanelOpen.set(false)}>
        <ChevronDown size={13} />
      </button>
    </div>
    <div class="runs">
      {#each $agentRuns as r (r.id)}
        {@const Icon = icon(r.status)}
        <div class="run" role="button" tabindex="0" onclick={() => open(r.id)}
          onkeydown={(e) => e.key === "Enter" && open(r.id)}>
          <span class="st {r.status}" title={statusLabel(r.status)}>
            <Icon size={12} />
          </span>
          <span class="ag" title={r.agent_connection_id}>{r.agent_connection_id}</span>
          <span class="task" title={r.subtask_prompt}>{r.subtask_prompt ? truncate(r.subtask_prompt) : "…"}</span>
          <span class="time">{elapsed(r)}</span>
          <span class="acts">
            {#if r.status === "queued" || r.status === "running"}
              <button class="mini" title={m.agents_cancel()}
                onclick={(e) => { e.stopPropagation(); void cancelRun(r.id); }}>
                <X size={12} />
              </button>
            {/if}
            {#if r.status === "done" && r.worktree_branch}
              {#if busyRun[r.id]}
                <span class="mini-busy" title={busyRun[r.id] === "approve" ? m.agents_approve() : m.agents_reject()}>
                  <Loader size={12} class="spin" />
                </span>
              {:else}
                <button class="mini ok" title={m.agents_approve()}
                  onclick={(e) => { e.stopPropagation(); void handleApproveRun(r.id); }}>
                  <Check size={12} />
                </button>
                <button class="mini danger" title={m.agents_reject()}
                  onclick={(e) => { e.stopPropagation(); void handleRejectRun(r.id); }}>
                  <Trash2 size={12} />
                </button>
              {/if}
            {/if}
          </span>
        </div>
      {/each}
      {#if $agentRuns.length === 0}
        <div class="empty">{m.agents_no_runs()}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .panel { border-bottom: 1px solid var(--border); background: var(--background); }
  .head { display: flex; align-items: center; gap: 0.4rem; padding: 0.35rem 0.75rem; font-size: 0.72rem; font-weight: 600; color: var(--foreground); }
  .count { font-size: 0.65rem; padding: 0 0.4rem; border-radius: 9999px; background: hsl(217 91% 60%); color: white; }
  .fold { margin-left: auto; display: inline-flex; align-items: center; justify-content: center; height: 1.25rem; width: 1.25rem; border: none; border-radius: var(--radius-sm); background: transparent; color: var(--muted-foreground); cursor: default; }
  .fold:hover { background: var(--accent); color: var(--accent-foreground); }
  .runs { display: flex; flex-direction: column; gap: 2px; max-height: 180px; overflow-y: auto; padding: 0 0.4rem 0.4rem; }
  .run { display: flex; align-items: center; gap: 0.45rem; padding: 0.25rem 0.4rem; border-radius: var(--radius-sm); font-size: 0.75rem; color: var(--foreground); cursor: default; min-width: 0; }
  .run:hover { background: var(--accent); }
  .st { display: inline-flex; color: var(--muted-foreground); flex-shrink: 0; }
  .st.running { color: hsl(217 91% 60%); }
  .st.running :global(svg) { animation: spin 1s linear infinite; }
  .st.done { color: hsl(140 60% 40%); }
  .st.error { color: var(--destructive); }
  .st.timeout { color: hsl(38 92% 50%); }
  .st.merged { color: hsl(260 70% 60%); }
  .ag { font-weight: 500; flex-shrink: 0; max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .task { flex: 1; color: var(--muted-foreground); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .time { font-size: 0.68rem; color: var(--muted-foreground); flex-shrink: 0; }
  .acts { display: inline-flex; gap: 0.2rem; flex-shrink: 0; }
  .mini { display: inline-flex; align-items: center; justify-content: center; height: 1.35rem; width: 1.35rem; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--background); color: var(--muted-foreground); cursor: default; }
  .mini.ok { color: hsl(140 60% 40%); }
  .mini.danger { color: var(--destructive); }
  .mini:hover { background: var(--accent); }
  .mini-busy { display: inline-flex; align-items: center; justify-content: center; height: 1.35rem; width: 1.35rem; color: var(--muted-foreground); }
  :global(.spin) { animation: spin 1s linear infinite; }
  .empty { padding: 0.5rem; color: var(--muted-foreground); font-size: 0.75rem; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>