import { writable } from "svelte/store";
import type { OsTag } from "$lib/os";

/** Resolved OS tag, set once on mount from tauri-plugin-os. */
export const osTag = writable<OsTag>("linux");

/** Main sidebar visibility (toggle_sidebar hotkey / toolbar button). */
export const sidebarVisible = writable(true);

/** Command palette (Cmd+K) overlay visibility. */
export const commandPaletteOpen = writable(false);

/** Home view visibility. When true the main area shows the Home screen instead of a chat. Opening a chat sets this false. */
export const homeView = writable(true);

/** Active project board view. When set (and homeView false, no current chat), the main area shows ProjectBoard instead of Home/Chat. */
export const boardProjectId = writable<string | null>(null);

/** Prompts view. When true (homeView false, no current chat), the main area shows the Prompts screen instead of Home. */
export const promptsView = writable(false);