import { writable } from "svelte/store";

const SIDEBAR_KEY = "aiassistant.layout.sidebarWidth";

export const SIDEBAR_MIN = 180;
export const SIDEBAR_MAX_RATIO = 0.5;
export const RIGHT_MIN = 200;
export const RIGHT_MAX_RATIO = 0.5;

const DEFAULT_SIDEBAR = 240;
export const DEFAULT_RIGHT = 280;

function clampSidebar(w: number): number {
  const max = Math.floor(window.innerWidth * SIDEBAR_MAX_RATIO);
  return Math.min(Math.max(Math.round(w), SIDEBAR_MIN), Math.max(SIDEBAR_MIN, max));
}

export function clampRight(w: number): number {
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

const SETTINGS_NAV_KEY = "aiassistant.layout.settingsNavWidth";
export const SETTINGS_NAV_MIN = 120;
export const SETTINGS_NAV_MAX = 360;
const DEFAULT_SETTINGS_NAV = 160;

function clampSettingsNav(w: number): number {
  return Math.min(Math.max(Math.round(w), SETTINGS_NAV_MIN), SETTINGS_NAV_MAX);
}

function readSettingsNav(): number {
  try {
    const v = localStorage.getItem(SETTINGS_NAV_KEY);
    const n = v ? parseInt(v, 10) : DEFAULT_SETTINGS_NAV;
    return Number.isFinite(n) ? clampSettingsNav(n) : DEFAULT_SETTINGS_NAV;
  } catch {
    return DEFAULT_SETTINGS_NAV;
  }
}

export const settingsNavWidth = writable<number>(
  typeof window !== "undefined" ? readSettingsNav() : DEFAULT_SETTINGS_NAV,
);

export function setSettingsNavWidth(w: number) {
  settingsNavWidth.set(typeof window !== "undefined" ? clampSettingsNav(w) : Math.round(w));
}

export function saveSettingsNavWidth(w: number) {
  try {
    localStorage.setItem(SETTINGS_NAV_KEY, String(w));
  } catch {
    // localStorage unavailable; applies for this session only
  }
}