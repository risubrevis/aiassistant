import { writable } from "svelte/store";

const SIDEBAR_KEY = "aiassistant.layout.sidebarWidth";
const RIGHT_KEY = "aiassistant.layout.rightSidebarWidth";
const RIGHT_OPEN_KEY = "aiassistant.layout.rightSidebarOpen";

export const SIDEBAR_MIN = 180;
export const SIDEBAR_MAX_RATIO = 0.5;
export const RIGHT_MIN = 200;
export const RIGHT_MAX_RATIO = 0.5;

const DEFAULT_SIDEBAR = 240;
const DEFAULT_RIGHT = 280;

function clampSidebar(w: number): number {
  const max = Math.floor(window.innerWidth * SIDEBAR_MAX_RATIO);
  return Math.min(Math.max(Math.round(w), SIDEBAR_MIN), Math.max(SIDEBAR_MIN, max));
}

function clampRight(w: number): number {
  const max = Math.floor(window.innerWidth * RIGHT_MAX_RATIO);
  return Math.min(Math.max(Math.round(w), RIGHT_MIN), Math.max(RIGHT_MIN, max));
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

function readRight(): number {
  try {
    const v = localStorage.getItem(RIGHT_KEY);
    const n = v ? parseInt(v, 10) : DEFAULT_RIGHT;
    return Number.isFinite(n) ? clampRight(n) : DEFAULT_RIGHT;
  } catch {
    return DEFAULT_RIGHT;
  }
}

function readRightOpen(): boolean {
  try {
    return localStorage.getItem(RIGHT_OPEN_KEY) !== "false";
  } catch {
    return true;
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

export const rightSidebarWidth = writable<number>(
  typeof window !== "undefined" ? readRight() : DEFAULT_RIGHT,
);

export function setRightSidebarWidth(w: number) {
  rightSidebarWidth.set(typeof window !== "undefined" ? clampRight(w) : Math.round(w));
}

export function saveRightSidebarWidth(w: number) {
  try {
    localStorage.setItem(RIGHT_KEY, String(w));
  } catch {
    // localStorage unavailable; applies for this session only
  }
}

export const rightSidebarOpen = writable<boolean>(
  typeof window !== "undefined" ? readRightOpen() : true,
);

export function setRightSidebarOpen(v: boolean) {
  rightSidebarOpen.set(v);
}

export function saveRightSidebarOpen(v: boolean) {
  try {
    localStorage.setItem(RIGHT_OPEN_KEY, String(v));
  } catch {
    // localStorage unavailable; applies for this session only
  }
}