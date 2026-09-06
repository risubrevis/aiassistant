import { boardProjectId, commandPaletteOpen, homeView, promptsView, sidebarVisible } from "$lib/stores/app";
import { currentChatId, newChat } from "$lib/stores/chat";
import { createProject } from "$lib/stores/project";
import { m } from "$lib/i18n";
import { COMPOSER_SEND_EVENT } from "$lib/events";
import { openSettingsWindow } from "$lib/tauri";

export async function newProjectAction() {
  const name = window.prompt(m.sidebar_new_project());
  if (!name || !name.trim()) return;
  await createProject(name.trim());
}

export function toggleSidebar() {
  sidebarVisible.update((v) => !v);
}

export function togglePalette() {
  commandPaletteOpen.update((v) => !v);
}

export function goHome() {
  homeView.set(true);
  promptsView.set(false);
  boardProjectId.set(null);
}

export function openPromptsView() {
  homeView.set(false);
  promptsView.set(true);
  boardProjectId.set(null);
  currentChatId.set(null);
}

export function openSettings() {
  void openSettingsWindow();
}

export function openSettingsTab(tab: string) {
  void openSettingsWindow(tab);
}

export function composerSend() {
  window.dispatchEvent(new CustomEvent(COMPOSER_SEND_EVENT));
}

/** Runs an action by its hotkey name (config.toml [hotkeys] key). Returns false for unknown names. */
export function handleHotkey(name: string): boolean {
  switch (name) {
    case "new_chat":
      void newChat();
      return true;
    case "new_project":
      void newProjectAction();
      return true;
    case "palette":
      togglePalette();
      return true;
    case "settings":
      openSettings();
      return true;
    case "toggle_sidebar":
      toggleSidebar();
      return true;
    case "send":
      composerSend();
      return true;
    default:
      return false;
  }
}