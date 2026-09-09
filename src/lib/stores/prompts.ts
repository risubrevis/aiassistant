import { writable } from "svelte/store";
import * as ipc from "$lib/tauri";
import type { Chat, Prompt, PromptLaunchSettings } from "$lib/tauri";
import { chats, openChat } from "$lib/stores/chat";
import { loadProjectChats } from "$lib/stores/project";

export const prompts = writable<Prompt[]>([]);

export const favoritePrompts = writable<Prompt[]>([]);

export async function loadPrompts(): Promise<void> {
  try {
    prompts.set(await ipc.promptList());
  } catch (e) {
    console.error("promptList failed", e);
  }
}

export async function loadFavoritePrompts(): Promise<void> {
  try {
    favoritePrompts.set(await ipc.promptListFavorites());
  } catch (e) {
    console.error("promptListFavorites failed", e);
  }
}

export async function createPrompt(
  title: string,
  body: string,
  projectId: string | null,
  attachFiles: string[],
  skillIds: string[],
  isFavorite: boolean,
  launchSettings: PromptLaunchSettings,
): Promise<Prompt | null> {
  try {
    const prompt = await ipc.promptCreate(
      title,
      body,
      projectId,
      attachFiles,
      skillIds,
      isFavorite,
      launchSettings,
    );
    await loadPrompts();
    await loadFavoritePrompts();
    return prompt;
  } catch (e) {
    console.error("promptCreate failed", e);
    return null;
  }
}

export async function updatePrompt(
  id: string,
  title: string,
  body: string,
  projectId: string | null,
  attachFiles: string[],
  skillIds: string[],
  isFavorite: boolean,
  launchSettings: PromptLaunchSettings,
): Promise<void> {
  try {
    await ipc.promptUpdate(id, title, body, projectId, attachFiles, skillIds, isFavorite, launchSettings);
    await loadPrompts();
    await loadFavoritePrompts();
  } catch (e) {
    console.error("promptUpdate failed", e);
  }
}

export async function deletePrompt(id: string): Promise<void> {
  try {
    await ipc.promptDelete(id);
    await loadPrompts();
    await loadFavoritePrompts();
  } catch (e) {
    console.error("promptDelete failed", e);
  }
}

export async function setPromptFavorite(id: string, isFavorite: boolean): Promise<void> {
  try {
    await ipc.promptSetFavorite(id, isFavorite);
    await loadPrompts();
    await loadFavoritePrompts();
  } catch (e) {
    console.error("promptSetFavorite failed", e);
  }
}

export async function movePrompt(id: string, toPosition: number): Promise<void> {
  try {
    await ipc.promptMove(id, toPosition);
    await loadPrompts();
    await loadFavoritePrompts();
  } catch (e) {
    console.error("promptMove failed", e);
  }
}

// No try/catch: errors (e.g. missing attached files) propagate to the caller to show a toast.
// Register the chat in the master chats store (used by the main view to route
// $chats.find(id)) and refresh the project's chat list so a project-bound prompt
// shows up in the sidebar immediately instead of only after a restart.
export async function runPrompt(id: string): Promise<Chat> {
  const chat = await ipc.promptRun(id);
  chats.update((list) => (list.some((c) => c.id === chat.id) ? list : [chat, ...list]));
  if (chat.project_id) {
    await loadProjectChats(chat.project_id);
  }
  await loadPrompts();
  await loadFavoritePrompts();
  await openChat(chat.id);
  return chat;
}