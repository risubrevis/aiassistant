import { writable } from "svelte/store";

export type MemoryMode = "chat" | "project";
export const memoryModalOpen = writable(false);
export const memoryModalMode = writable<MemoryMode>("chat");
export const memoryModalId = writable<string | null>(null);

export function openChatMemory(chatId: string) {
  memoryModalMode.set("chat");
  memoryModalId.set(chatId);
  memoryModalOpen.set(true);
}

export function openProjectMemory(projectId: string) {
  memoryModalMode.set("project");
  memoryModalId.set(projectId);
  memoryModalOpen.set(true);
}