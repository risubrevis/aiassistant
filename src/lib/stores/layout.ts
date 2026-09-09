import { writable } from "svelte/store";

const SIDEBAR_KEY = "aiassistant.layout.sidebarWidth";
const RIGHT_KEY_PROJECT = "aiassistant.layout.rightSidebarWidth.project";
const RIGHT_KEY_STANDALONE = "aiassistant.layout.rightSidebarWidth.standalone";
const RIGHT_KEY_LEGACY = "aiassistant.layout.rightSidebarWidth";
const RIGHT_OPEN_PROJECT_KEY = "aiassistant.layout.rightSidebarOpen.project";
const RIGHT_OPEN_STANDALONE_KEY = "aiassistant.layout.rightSidebarOpen.standalone";
const RIGHT_OPEN_LEGACY_KEY = "aiassistant.layout.rightSidebarOpen";

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

function readRightWidth(key: string): number {
  try {
    const v = localStorage.getItem(key);
    if (v !== null) {
      const n = parseInt(v, 10);
      return Number.isFinite(n) ? clampRight(n) : DEFAULT_RIGHT;
    }
    const legacy = localStorage.getItem(RIGHT_KEY_LEGACY);
    const n = legacy ? parseInt(legacy, 10) : DEFAULT_RIGHT;
    return Number.isFinite(n) ? clampRight(n) : DEFAULT_RIGHT;
  } catch {
    return DEFAULT_RIGHT;
  }
}

function readRightOpenProject(): boolean {
  try {
    const v = localStorage.getItem(RIGHT_OPEN_PROJECT_KEY);
    if (v !== null) return v !== "false";
    return localStorage.getItem(RIGHT_OPEN_LEGACY_KEY) !== "false";
  } catch {
    return true;
  }
}

function readRightOpenStandalone(): boolean {
  try {
    const v = localStorage.getItem(RIGHT_OPEN_STANDALONE_KEY);
    if (v !== null) return v !== "false";
    return localStorage.getItem(RIGHT_OPEN_LEGACY_KEY) !== "false";
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

export const rightSidebarWidthProject = writable<number>(
  typeof window !== "undefined" ? readRightWidth(RIGHT_KEY_PROJECT) : DEFAULT_RIGHT,
);

export const rightSidebarWidthStandalone = writable<number>(
  typeof window !== "undefined" ? readRightWidth(RIGHT_KEY_STANDALONE) : DEFAULT_RIGHT,
);

export function setRightSidebarWidthProject(w: number) {
  rightSidebarWidthProject.set(typeof window !== "undefined" ? clampRight(w) : Math.round(w));
}

export function setRightSidebarWidthStandalone(w: number) {
  rightSidebarWidthStandalone.set(typeof window !== "undefined" ? clampRight(w) : Math.round(w));
}

export function saveRightSidebarWidthProject(w: number) {
  try {
    localStorage.setItem(RIGHT_KEY_PROJECT, String(w));
  } catch {
    // localStorage unavailable; applies for this session only
  }
}

export function saveRightSidebarWidthStandalone(w: number) {
  try {
    localStorage.setItem(RIGHT_KEY_STANDALONE, String(w));
  } catch {
    // localStorage unavailable; applies for this session only
  }
}

export const rightSidebarOpenProject = writable<boolean>(
  typeof window !== "undefined" ? readRightOpenProject() : true,
);

export const rightSidebarOpenStandalone = writable<boolean>(
  typeof window !== "undefined" ? readRightOpenStandalone() : true,
);

export function setRightSidebarOpenProject(v: boolean) {
  rightSidebarOpenProject.set(v);
}

export function saveRightSidebarOpenProject(v: boolean) {
  try {
    localStorage.setItem(RIGHT_OPEN_PROJECT_KEY, String(v));
  } catch {
    // localStorage unavailable; applies for this session only
  }
}

export function setRightSidebarOpenStandalone(v: boolean) {
  rightSidebarOpenStandalone.set(v);
}

export function saveRightSidebarOpenStandalone(v: boolean) {
  try {
    localStorage.setItem(RIGHT_OPEN_STANDALONE_KEY, String(v));
  } catch {
    // localStorage unavailable; applies for this session only
  }
}