export interface HotkeySpec {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
  key: string;
}

// Mirrors [hotkeys] defaults in src-tauri config; used until config.toml loads.
export const DEFAULT_HOTKEYS: Record<string, string> = {
  new_chat: "CmdOrCtrl+N",
  new_project: "CmdOrCtrl+Shift+N",
  palette: "CmdOrCtrl+K",
  send: "CmdOrCtrl+Enter",
  toggle_sidebar: "CmdOrCtrl+B",
  settings: "CmdOrCtrl+Comma",
};

const MODIFIERS: Record<string, "ctrl" | "alt" | "shift" | "meta"> = {
  ctrl: "ctrl",
  control: "ctrl",
  cmd: "meta",
  command: "meta",
  meta: "meta",
  alt: "alt",
  option: "alt",
  opt: "alt",
  shift: "shift",
};

const NAMED_KEYS: Record<string, string> = {
  comma: ",",
  plus: "+",
  minus: "-",
  space: " ",
};

const CMD_OR_CTRL = new Set(["cmdorctrl", "cmdorcontrol"]);

export function parseHotkey(s: string, isMac = false): HotkeySpec | null {
  const spec: HotkeySpec = { ctrl: false, alt: false, shift: false, meta: false, key: "" };
  for (const raw of s.split("+")) {
    const part = raw.trim().toLowerCase();
    if (!part) continue;
    if (CMD_OR_CTRL.has(part)) {
      if (isMac) spec.meta = true;
      else spec.ctrl = true;
      continue;
    }
    const mod = MODIFIERS[part];
    if (mod) {
      spec[mod] = true;
      continue;
    }
    spec.key = NAMED_KEYS[part] ?? part;
  }
  return spec.key ? spec : null;
}

export function hotkeyMatches(e: KeyboardEvent, hk: HotkeySpec): boolean {
  return (
    hk.key === e.key.toLowerCase() &&
    hk.ctrl === e.ctrlKey &&
    hk.alt === e.altKey &&
    hk.shift === e.shiftKey &&
    hk.meta === e.metaKey
  );
}

export function formatHotkey(combo: string, isMac = false): string {
  const hk = parseHotkey(combo, isMac);
  if (!hk) return "";
  const key = hk.key.length === 1 ? hk.key.toUpperCase() : capKey(hk.key);
  if (isMac) {
    let out = "";
    if (hk.ctrl) out += "\u2303";
    if (hk.alt) out += "\u2325";
    if (hk.shift) out += "\u21e7";
    if (hk.meta) out += "\u2318";
    return out + key;
  }
  const parts: string[] = [];
  if (hk.ctrl) parts.push("Ctrl");
  if (hk.alt) parts.push("Alt");
  if (hk.shift) parts.push("Shift");
  if (hk.meta) parts.push("Meta");
  parts.push(key);
  return parts.join("+");
}

function capKey(key: string): string {
  return key.charAt(0).toUpperCase() + key.slice(1);
}