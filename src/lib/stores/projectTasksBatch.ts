import { writable, get } from "svelte/store";
import * as ipc from "$lib/tauri";
import { statusByChat, chats } from "$lib/stores/chat";
import { tasksByProject, runProjectTask, loadProjectTasks } from "$lib/stores/projectTasks";
import { loadProjectChats } from "$lib/stores/project";
import { toast } from "$lib/stores/toasts";
import { m } from "$lib/i18n";
import type { Chat } from "$lib/tauri";

export type BatchMode = "sequential" | "simultaneous" | "one_chat";

export interface BatchRun {
  projectId: string;
  mode: BatchMode;
  taskIds: string[];
  chatIds: string[];
  currentIndex: number;
  active: boolean;
  cancelRequested: boolean;
  handled: Set<string>;
}

export const batchRuns = writable<Record<string, BatchRun | undefined>>({});

let statusUnsub: (() => void) | null = null;

function terminal(status: string | undefined): boolean {
  return status === "idle" || status === "error" || status === "cancelled";
}

function registerChat(chat: Chat) {
  chats.update((list) => (list.some((c) => c.id === chat.id) ? list : [chat, ...list]));
}

function teardownListener() {
  if (statusUnsub) {
    statusUnsub();
    statusUnsub = null;
  }
}

function finishBatch(projectId: string) {
  const run = get(batchRuns)[projectId];
  const cancelled = !!run?.cancelRequested;
  batchRuns.update((map) => {
    const r = map[projectId];
    if (!r) return map;
    return { ...map, [projectId]: { ...r, active: false } };
  });
  teardownListener();
  toast.success(cancelled ? m.board_run_all_stopped() : m.board_run_all_done());
}

async function runNextSequential(projectId: string) {
  const run = get(batchRuns)[projectId];
  if (!run || !run.active) return;
  if (run.cancelRequested || run.currentIndex >= run.taskIds.length) {
    finishBatch(projectId);
    return;
  }
  const taskId = run.taskIds[run.currentIndex];
  const chat = await runProjectTask(projectId, taskId);
  if (chat) {
    batchRuns.update((map) => {
      const r = map[projectId];
      if (!r) return map;
      return { ...map, [projectId]: { ...r, chatIds: [...r.chatIds, chat.id] } };
    });
  }
}

function onStatusChange(statusMap: Record<string, string | undefined>) {
  const map = get(batchRuns);
  for (const projectId of Object.keys(map)) {
    const run = map[projectId];
    if (!run || !run.active) continue;

    if (run.mode === "sequential") {
      const curChat = run.chatIds[run.currentIndex];
      if (curChat && terminal(statusMap[curChat]) && !run.handled.has(curChat)) {
        run.handled.add(curChat);
        if (run.cancelRequested || run.currentIndex >= run.taskIds.length - 1) {
          finishBatch(projectId);
        } else {
          batchRuns.update((mm) => {
            const r = mm[projectId];
            if (!r) return mm;
            return { ...mm, [projectId]: { ...r, currentIndex: r.currentIndex + 1 } };
          });
          void runNextSequential(projectId);
        }
      }
    } else if (run.mode === "simultaneous") {
      if (run.chatIds.length > 0 && run.chatIds.every((id) => terminal(statusMap[id]))) {
        finishBatch(projectId);
      }
    } else {
      const chat = run.chatIds[0];
      if (chat && terminal(statusMap[chat])) {
        finishBatch(projectId);
      }
    }
  }
}

export async function startBatchRun(projectId: string, mode: BatchMode) {
  const current = get(batchRuns)[projectId];
  if (current?.active) return;

  const todo = (get(tasksByProject)[projectId] ?? [])
    .filter((t) => t.status === "todo")
    .sort((a, b) => a.position - b.position);
  if (todo.length === 0) {
    toast.info(m.board_run_all_no_tasks());
    return;
  }
  const taskIds = todo.map((t) => t.id);

  batchRuns.update((map) => ({
    ...map,
    [projectId]: {
      projectId,
      mode,
      taskIds,
      chatIds: [],
      currentIndex: 0,
      active: true,
      cancelRequested: false,
      handled: new Set(),
    },
  }));

  if (!statusUnsub) {
    statusUnsub = statusByChat.subscribe(onStatusChange);
  }

  if (mode === "one_chat") {
    try {
      const chat = await ipc.projectTaskRunBatch(taskIds);
      registerChat(chat);
      batchRuns.update((map) => {
        const r = map[projectId];
        if (!r) return map;
        return { ...map, [projectId]: { ...r, chatIds: [chat.id] } };
      });
      await loadProjectTasks(projectId);
      await loadProjectChats(projectId);
    } catch (e) {
      console.error("projectTaskRunBatch failed", e);
      toast.error(m.toasts_chat_error());
      finishBatch(projectId);
    }
  } else if (mode === "simultaneous") {
    const ids: string[] = [];
    for (const id of taskIds) {
      try {
        const chat = await runProjectTask(projectId, id);
        if (chat) ids.push(chat.id);
      } catch (e) {
        console.error("batch runProjectTask failed", e);
      }
    }
    batchRuns.update((map) => {
      const r = map[projectId];
      if (!r) return map;
      return { ...map, [projectId]: { ...r, chatIds: ids } };
    });
  } else {
    await runNextSequential(projectId);
  }
}

export async function cancelBatchRun(projectId: string) {
  const run = get(batchRuns)[projectId];
  if (!run || !run.active) return;
  batchRuns.update((map) => {
    const r = map[projectId];
    if (!r) return map;
    return { ...map, [projectId]: { ...r, cancelRequested: true } };
  });
  for (const chatId of run.chatIds) {
    if (get(statusByChat)[chatId] === "running") {
      try {
        await ipc.chatCancel(chatId);
      } catch (e) {
        console.error("chatCancel failed", e);
      }
    }
  }
}