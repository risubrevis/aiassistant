import { writable } from "svelte/store";
import type { TaskItem } from "$lib/tauri";

export const tasksByChat = writable<Record<string, TaskItem[]>>({});

/** Chat id of the currently open Tasks modal, null = closed. */
export const tasksModalChatId = writable<string | null>(null);

export function openTasksModal(chatId: string) {
  tasksModalChatId.set(chatId);
}

export function closeTasksModal() {
  tasksModalChatId.set(null);
}

export function applyTasksUpdate(chatId: string, tasks: TaskItem[]) {
  // Backend pre-orders the list (internal by position, then agent runs by
  // started_at) — preserve it; sorting by position alone would break run grouping.
  tasksByChat.update((m) => ({ ...m, [chatId]: [...tasks] }));
}

export function clearTasks(chatId: string) {
  tasksByChat.update((m) => {
    const next = { ...m };
    delete next[chatId];
    return next;
  });
}

export function taskCounts(tasks: TaskItem[]) {
  const total = tasks.filter((t) => t.status !== "cancelled").length;
  const done = tasks.filter((t) => t.status === "completed").length;
  const inProgress = tasks.filter((t) => t.status === "in_progress").length;
  return { total, done, inProgress, hasOpen: total - done > 0 };
}

export function groupTasks(tasks: TaskItem[]): {
  internal: TaskItem[];
  agentRuns: { runId: string; tasks: TaskItem[] }[];
} {
  const internal: TaskItem[] = [];
  const agentRuns: { runId: string; tasks: TaskItem[] }[] = [];
  for (const t of tasks) {
    if (t.source === "agent") {
      const runId = t.run_id ?? "";
      const last = agentRuns[agentRuns.length - 1];
      if (last && last.runId === runId) last.tasks.push(t);
      else agentRuns.push({ runId, tasks: [t] });
    } else {
      internal.push(t);
    }
  }
  return { internal, agentRuns };
}