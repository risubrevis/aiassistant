import { writable } from "svelte/store";

const SIDEBAR_KEY = "aiassistant.layout.sidebarWidth";

export const SIDEBAR_MIN = 180;
export const SIDEBAR_MAX_RATIO = 0.5;

const DEFAULT_SIDEBAR = 240;

function clampSidebar(w: number): number {
  const max = Math.floor(window.innerWidth * SIDEBAR_MAX_RATIO);
  return Math.min(Math.max(Math.round(w), SIDEBAR_MIN), Math.max(SIDEBAR_MIN, max));
}

function readSidebar(): number {
  try {
    const v = localStorage.getItem(SIDEBAR_KEY);
    const n = v ? parseInt(v, 10) : DEFAULT_SIDEBAR;
    return Number.isFinite(n) ? clampSidebar(n) : DEFAULT_SIDEBAR;
  } catch {
    return DEFAULT_SIDEBAR;
  }
}

export const sidebarWidth = writable<number>(
  typeof window !== "undefined" ? readSidebar() : DEFAULT_SIDEBAR,
);

export function setSidebarWidth(w: number) {
  sidebarWidth.set(typeof window !== "undefined" ? clampSidebar(w) : Math.round(w));
}

export function saveSidebarWidth(w: number) {
  try {
    localStorage.setItem(SIDEBAR_KEY, String(w));
  } catch {
    // localStorage unavailable; applies for this session only
  }
}