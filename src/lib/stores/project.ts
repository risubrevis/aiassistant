import { writable } from "svelte/store";
import * as ipc from "$lib/tauri";
import type { Chat, Project, ChangedFileView } from "$lib/tauri";
import { COMPOSER_INSERT_EVENT } from "$lib/events";
import { tasksByProject } from "./projectTasks";

export const projects = writable<Project[]>([]);
export const currentProjectId = writable<string | null>(null);
export const projectChats = writable<Record<string, Chat[]>>({});
export const fileChanges = writable<Record<string, { path: string; kind: string }[]>>({});
export const changedFileViews = writable<Record<string, ChangedFileView[]>>({});
export const pendingAsk = writable<Record<string, ipc.AskUserEvent>>({});

export const projectSettingsOpen = writable(false);
export const projectSettingsId = writable<string | null>(null);

export const projectRulesOpen = writable(false);
export const projectRulesId = writable<string | null>(null);
export function openProjectRules(id: string) {
  projectRulesId.set(id);
  projectRulesOpen.set(true);
}

export type ProjectSortMode = "updated" | "created" | "alpha" | "manual";
export const projectSortMode = writable<ProjectSortMode>(
  typeof localStorage !== "undefined"
    ? ((localStorage.getItem("aiassistant.projectSortMode") as ProjectSortMode | null) ?? "updated")
    : "updated",
);
projectSortMode.subscribe((v) => {
  try {
    localStorage.setItem("aiassistant.projectSortMode", v);
  } catch {}
});

export function sortProjects(list: Project[], mode: ProjectSortMode): Project[] {
  const pinnedFirst = (a: Project, b: Project) => b.pinned - a.pinned;
  const cmp: (a: Project, b: Project) => number =
    mode === "alpha"
      ? (a, b) => a.name.localeCompare(b.name)
      : mode === "created"
        ? (a, b) => b.created_at - a.created_at
        : mode === "manual"
          ? (a, b) => a.sort_order - b.sort_order
          : (a, b) => b.updated_at - a.updated_at;
  return [...list].sort((a, b) => pinnedFirst(a, b) || cmp(a, b));
}

export async function loadProjects() {
  try {
    projects.set(await ipc.projectList());
  } catch (e) {
    console.error("projectList failed", e);
  }
}

export async function createProject(name: string, color = "#6366f1") {
  try {
    const project = await ipc.projectCreate(name, color);
    projects.update((list) => [...list, project]);
    return project;
  } catch (e) {
    console.error("projectCreate failed", e);
    return null;
  }
}

// No try/catch: errors propagate to the caller to show a toast.
export async function deleteProject(id: string) {
  await ipc.projectDelete(id);
  projects.update((list) => list.filter((p) => p.id !== id));
  projectChats.update((m) => {
    const next = { ...m };
    delete next[id];
    return next;
  });
  fileChanges.update((m) => {
    const next = { ...m };
    delete next[id];
    return next;
  });
  changedFileViews.update((m) => {
    const next = { ...m };
    delete next[id];
    return next;
  });
  tasksByProject.update((m) => {
    const next = { ...m };
    delete next[id];
    return next;
  });
  currentProjectId.update((cur) => (cur === id ? null : cur));
}

export async function openProject(id: string) {
  currentProjectId.set(id);
  await loadProjectChats(id);
}

export async function loadProjectChats(id: string) {
  try {
    const chats = await ipc.projectChats(id);
    projectChats.update((m) => ({ ...m, [id]: chats }));
  } catch (e) {
    console.error("projectChats failed", e);
  }
}

export async function toggleProjectPinned(id: string, pinned: boolean) {
  try {
    await ipc.projectSetPinned(id, pinned);
    projects.update((list) => list.map((p) => (p.id === id ? { ...p, pinned: pinned ? 1 : 0 } : p)));
  } catch (e) {
    console.error("projectSetPinned failed", e);
  }
}

export async function reorderProjects(orderedIds: string[]) {
  try {
    await ipc.projectReorder(orderedIds);
    projects.update((list) => {
      const order = new Map(orderedIds.map((id, i) => [id, i]));
      return list.map((p) => ({ ...p, sort_order: order.get(p.id) ?? p.sort_order }));
    });
  } catch (e) {
    console.error("projectReorder failed", e);
  }
}

export async function renameProject(id: string, name: string) {
  try {
    const p = await ipc.projectGet(id);
    if (!p) return;
    const updated = { ...p, name };
    await ipc.projectUpdate(updated);
    projects.update((list) => list.map((x) => (x.id === id ? updated : x)));
  } catch (e) {
    console.error("renameProject failed", e);
  }
}

export function applyFileChanged(e: ipc.FileChangedEvent) {
  fileChanges.update((m) => {
    const list = m[e.project_id] ?? [];
    const next = [{ path: e.path, kind: e.kind }, ...list].slice(0, 50);
    return { ...m, [e.project_id]: next };
  });
}

export async function loadChangedFileViews(projectId: string) {
  try {
    const views = await ipc.projectChangedFiles(projectId);
    changedFileViews.update((m) => ({ ...m, [projectId]: views }));
  } catch (e) {
    console.error("projectChangedFiles failed", e);
  }
}

export function insertFileRef(rel: string) {
  window.dispatchEvent(
    new CustomEvent(COMPOSER_INSERT_EVENT, { detail: { text: "@file " + rel } }),
  );
}

export function insertAllFileRefs(rels: string[]) {
  window.dispatchEvent(
    new CustomEvent(COMPOSER_INSERT_EVENT, {
      detail: { text: rels.map((r) => "@file " + r).join("\n") },
    }),
  );
}

export function applyAskUser(e: ipc.AskUserEvent) {
  pendingAsk.update((m) => ({ ...m, [e.block_id]: e }));
}

export async function resolveAsk(blockId: string, answer: string) {
  const ask = (() => {
    let v: ipc.AskUserEvent | undefined;
    pendingAsk.update((m) => {
      v = m[blockId];
      return m;
    });
    return v;
  })();
  if (!ask) return;
  try {
    await ipc.askUserReply(ask.request_id, answer);
  } catch (e) {
    console.error("askUserReply failed", e);
  }
  pendingAsk.update((m) => {
    const next = { ...m };
    delete next[blockId];
    return next;
  });
}

export function openProjectSettings(id: string) {
  projectSettingsId.set(id);
  projectSettingsOpen.set(true);
}