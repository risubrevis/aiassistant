import { writable } from "svelte/store";
import { configGet, onConfigReloaded } from "$lib/tauri";
import type { AppConfig } from "$lib/tauri";

export const config = writable<AppConfig | null>(null);

export async function loadConfig() {
  try {
    config.set(await configGet());
  } catch (e) {
    console.error("configGet failed", e);
  }
}

let inited = false;
export function initConfigStore() {
  if (inited) return;
  inited = true;
  void loadConfig();
  onConfigReloaded(() => loadConfig());
}