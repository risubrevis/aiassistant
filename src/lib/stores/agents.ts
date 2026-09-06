import { writable, derived, get } from "svelte/store";
import * as ipc from "$lib/tauri";
import type { AgentEvent, AgentRun, AgentRunDiff } from "$lib/tauri";

const EVENT_CAP = 200;
const ACTIVE_STATUSES = new Set(["queued", "running"]);

export const agents = writable<ipc.AgentInfo[]>([]);
export const agentRuns = writable<AgentRun[]>([]);
export const eventsByRun = writable<Record<string, AgentEvent[]>>({});
export const runDiffs = writable<Record<string, AgentRunDiff>>({});
export const runDiffLoading = writable<Record<string, boolean>>({});
export const lastBatchComplete = writable<ipc.AgentBatchCompleteEvent | null>(null);

export const agentsPanelOpen = writable(false);
export const agentModalRunId = writable<string | null>(null);
export const agentInfoModalOpen = writable(false);
export const agentAvailable = writable(false);

export const runningCount = derived(
  agentRuns,
  (runs) => runs.filter((r) => ACTIVE_STATUSES.has(r.status)).length,
);

// Events may arrive before a DB-backed run is loaded (or after a reload);
// synthesize a minimal entry so the panel stays live.
function stubRun(
  e: { run_id: string; chat_id: string; parent_id: string | null; agent_id: string },
  status: string,
): AgentRun {
  return {
    id: e.run_id,
    chat_id: e.chat_id,
    parent_tool_call_id: e.parent_id,
    parent_id: null,
    agent_connection_id: e.agent_id,
    subtask_index: 0,
    subtask_prompt: "",
    cwd: "",
    worktree_branch: null,
    agent_session_id: null,
    status,
    result_summary: null,
    started_at: null,
    ended_at: null,
    created_at: Date.now(),
  };
}

export async function refreshAgentAvailability() {
  const list = get(agents);
  agentAvailable.set(list.some((a) => a.status === "ok"));
}

export async function loadAgents() {
  try {
    agents.set(await ipc.agentList());
  } catch (e) {
    console.error("agentList failed", e);
  }
}

export async function loadAgentRuns(chatId: string | null = null) {
  try {
    agentRuns.set(await ipc.agentRunsList(chatId));
  } catch (e) {
    console.error("agentRunsList failed", e);
  }
}

export async function loadRunDiff(runId: string) {
  runDiffLoading.update((m) => ({ ...m, [runId]: true }));
  try {
    const diff = await ipc.agentRunDiff(runId);
    runDiffs.update((m) => ({ ...m, [runId]: diff }));
  } catch (e) {
    console.error("agentRunDiff failed", e);
  } finally {
    runDiffLoading.update((m) => ({ ...m, [runId]: false }));
  }
}

export function applyAgentProgress(e: ipc.AgentProgressEvent) {
  eventsByRun.update((m) => {
    const next = [...(m[e.run_id] ?? []), e.event];
    return { ...m, [e.run_id]: next.slice(-EVENT_CAP) };
  });
  agentRuns.update((runs) => {
    const idx = runs.findIndex((r) => r.id === e.run_id);
    if (idx < 0) return [...runs, stubRun(e, "running")];
    if (runs[idx].status === "running") return runs;
    const next = [...runs];
    next[idx] = { ...next[idx], status: "running" };
    return next;
  });
}

export function applyAgentStatus(e: ipc.AgentStatusEvent) {
  let known = false;
  let hasWorktree = false;
  agentRuns.update((runs) => {
    const idx = runs.findIndex((r) => r.id === e.run_id);
    if (idx < 0) return runs;
    known = true;
    hasWorktree = runs[idx].worktree_branch != null;
    const next = [...runs];
    next[idx] = {
      ...next[idx],
      status: e.status,
      result_summary: e.result_summary ?? next[idx].result_summary,
    };
    return next;
  });
  if (!known) {
    agentRuns.update((runs) => {
      if (runs.some((r) => r.id === e.run_id)) return runs;
      const stub = stubRun(e, e.status);
      if (e.result_summary != null) stub.result_summary = e.result_summary;
      return [...runs, stub];
    });
    void loadAgentRuns();
  }
  if (e.status === "done" && hasWorktree) void loadRunDiff(e.run_id);
}

export function applyAgentBatchComplete(e: ipc.AgentBatchCompleteEvent) {
  lastBatchComplete.set(e);
  void loadAgentRuns(e.chat_id);
}

export async function cancelRun(runId: string) {
  try {
    await ipc.agentRunCancel(runId);
    agentRuns.update((runs) =>
      runs.map((r) =>
        r.id === runId && ACTIVE_STATUSES.has(r.status) ? { ...r, status: "cancelled" } : r,
      ),
    );
  } catch (e) {
    console.error("agentRunCancel failed", e);
  }
}

export async function cancelAgentRunsInChat(agentId: string, chatId: string) {
  const runs = get(agentRuns).filter(
    (r) => r.chat_id === chatId && r.agent_connection_id === agentId && ACTIVE_STATUSES.has(r.status),
  );
  await Promise.all(runs.map((r) => cancelRun(r.id)));
}

export async function approveRun(runId: string) {
  try {
    await ipc.agentRunApprove(runId);
    await loadAgentRuns();
  } catch (e) {
    console.error("agentRunApprove failed", e);
  }
}

export async function rejectRun(runId: string) {
  try {
    await ipc.agentRunReject(runId);
    await loadAgentRuns();
  } catch (e) {
    console.error("agentRunReject failed", e);
  }
}