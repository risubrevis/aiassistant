import { writable } from "svelte/store";
import * as ipc from "$lib/tauri";
import type { Chat, ProjectTask, ProjectTaskStatus, ProjectTaskPriority } from "$lib/tauri";
import { loadProjectChats } from "$lib/stores/project";
import { chats } from "$lib/stores/chat";

export const tasksByProject = writable<Record<string, ProjectTask[]>>({});

export const TASK_COLUMNS: ProjectTaskStatus[] = ["backlog", "todo", "in_progress", "review", "done"];
export const TASK_PRIORITIES: ProjectTaskPriority[] = ["low", "medium", "high", "urgent"];

export async function loadProjectTasks(projectId: string) {
  try {
    const tasks = await ipc.projectTaskList(projectId);
    tasksByProject.update((m) => ({ ...m, [projectId]: tasks }));
  } catch (e) {
    console.error("projectTaskList failed", e);
  }
}

export async function createProjectTask(
  projectId: string,
  title: string,
  description: string,
  status: ProjectTaskStatus,
  priority: ProjectTaskPriority,
) {
  try {
    const task = await ipc.projectTaskCreate(projectId, title, description, status, priority);
    await loadProjectTasks(projectId);
    return task;
  } catch (e) {
    console.error("projectTaskCreate failed", e);
    return null;
  }
}

export async function updateProjectTask(
  projectId: string,
  id: string,
  title: string,
  description: string,
  status: ProjectTaskStatus,
  priority: ProjectTaskPriority,
) {
  try {
    await ipc.projectTaskUpdate(id, title, description, status, priority);
    await loadProjectTasks(projectId);
  } catch (e) {
    console.error("projectTaskUpdate failed", e);
  }
}

export async function deleteProjectTask(projectId: string, id: string) {
  try {
    await ipc.projectTaskDelete(id);
    await loadProjectTasks(projectId);
  } catch (e) {
    console.error("projectTaskDelete failed", e);
  }
}

export async function moveProjectTask(
  projectId: string,
  id: string,
  toStatus: ProjectTaskStatus,
  toPosition: number,
) {
  tasksByProject.update((m) => {
    const list = m[projectId];
    const task = list?.find((t) => t.id === id);
    if (!list || !task) return m;
    const fromStatus = task.status;
    const column = list
      .filter((t) => t.status === toStatus && t.id !== id)
      .sort((a, b) => a.position - b.position);
    const at = Math.max(0, Math.min(toPosition, column.length));
    column.splice(at, 0, { ...task, status: toStatus });
    const positions = new Map<string, number>();
    column.forEach((t, i) => positions.set(t.id, i));
    if (fromStatus !== toStatus) {
      list
        .filter((t) => t.status === fromStatus && t.id !== id)
        .sort((a, b) => a.position - b.position)
        .forEach((t, i) => positions.set(t.id, i));
    }
    return {
      ...m,
      [projectId]: list.map((t) => ({ ...t, position: positions.get(t.id) ?? t.position })),
    };
  });
  try {
    await ipc.projectTaskMove(id, toStatus, toPosition);
  } catch (e) {
    console.error("projectTaskMove failed", e);
  }
  await loadProjectTasks(projectId);
}

export async function runProjectTask(projectId: string, id: string) {
  try {
    const chat = await ipc.projectTaskRun(id);
    // Register the newly created chat in the master chats store so the main
    // view (which routes via $chats.find(id)) can open it instead of Home.
    chats.update((list) => (list.some((c) => c.id === chat.id) ? list : [chat, ...list]));
    await loadProjectChats(projectId);
    await loadProjectTasks(projectId);
    return chat;
  } catch (e) {
    console.error("projectTaskRun failed", e);
    return null;
  }
}