import { writable } from "svelte/store";

/** Active settings tab (settings window only). */
export const settingsTab = writable<string>("general");