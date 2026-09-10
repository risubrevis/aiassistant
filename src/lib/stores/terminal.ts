import { writable, get } from "svelte/store";
import * as ipc from "$lib/tauri";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "$lib/stores/toasts";
import { m } from "$lib/i18n";

export type TermStatus = "idle" | "running" | "needs_input";

export interface AgentCmdEntry {
  kind: "agent";
  block_id: string;
  command: string;
  output: string;
  code: number | null;
  needsInput: boolean;
  is_error: boolean;
  ts: number;
  lastOutputAt: number;
}

export const termStatusByChat = writable<Record<string, TermStatus>>({});
export const termAgentEntriesByChat = writable<Record<string, AgentCmdEntry[]>>({});

// Best-effort: strips ANSI escape sequences (CSI, OSC) and carriage returns.
export function stripAnsi(s: string): string {
  return s
    .replace(/\x1b\[[0-9;?]*[ -\/]*[@-~]/g, "")
    .replace(/\x1b\][^\x07]*(?:\x07|\x1b\\)/g, "")
    .replace(/\r/g, "");
}

const MAX_PTY_OUTPUT = 65536;

const NEEDS_INPUT_RE =
  /(password|passphrase|passwd)\s*(for[^:]+)?\s*:[\s]*$|^\s*\[?sudo\]?\s*password\b[\s:]*|continue\s*\??\s*\(?(y\/n|yes\/no)\)?|\(y\/n\)|yes\/no|\busername\s+for\b|enter\s+(your\s+)?password|are\s+you\s+sure|do\s+you\s+want\s+to|confirm\s*[:?]|\bproceed\s*[:?]/im;

// Best-effort heuristic for interactive prompts; may miss unusual prompts.
export function detectNeedsInput(outputTail: string): boolean {
  const tail = stripAnsi(outputTail).slice(-400);
  return NEEDS_INPUT_RE.test(tail);
}

const NEEDS_INPUT_IDLE_MS = 1500;

// A running command is waiting for user input when either a known prompt sits on
// the last output line, or — language-agnostic — output has stalled on an
// unterminated line: interactive prompts are printed without a trailing newline
// so the user types on the same line, while normal command output ends with \n.
function entryNeedsInput(e: AgentCmdEntry, now: number): boolean {
  if (e.code !== null) return false;
  const stripped = stripAnsi(e.output);
  const lastLine = stripped.slice(stripped.lastIndexOf("\n") + 1);
  if (detectNeedsInput(lastLine)) return true;
  return (
    stripped.length > 0 &&
    now - e.lastOutputAt > NEEDS_INPUT_IDLE_MS &&
    !stripped.endsWith("\n")
  );
}

// Periodic re-evaluation: a command that printed a prompt and is now idle won't
// emit further output events, so onChatPtyOutput won't fire again — the timer
// flips needsInput once the idle threshold passes.
function refreshNeedsInput(chatId: string, now: number): void {
  const list = get(termAgentEntriesByChat)[chatId];
  if (!list || !list.some((e) => e.code === null)) return;
  let changed = false;
  const next = list.map((e) => {
    if (e.code !== null) return e;
    const ni = entryNeedsInput(e, now);
    if (ni !== e.needsInput) {
      changed = true;
      return { ...e, needsInput: ni };
    }
    return e;
  });
  if (!changed) return;
  termAgentEntriesByChat.update((map) => ({ ...map, [chatId]: next }));
  recomputeStatus(chatId);
}

export function recomputeStatus(chatId: string): void {
  const entries = get(termAgentEntriesByChat)[chatId] ?? [];
  const running = entries.some((e) => e.code === null);
  const needsInput = running && entries.some((e) => e.code === null && e.needsInput);
  const status: TermStatus = needsInput ? "needs_input" : running ? "running" : "idle";
  termStatusByChat.update((map) => ({ ...map, [chatId]: status }));
}

let initialized = false;

// App-lifetime listeners (never unlistened).
export function initTerminalListeners(): void {
  if (initialized) return;
  initialized = true;

  void ipc.onChatPtyStart((e) => {
    termAgentEntriesByChat.update((map) => {
      const list = map[e.chat_id] ?? [];
      if (list.some((en) => en.block_id === e.block_id)) return map;
      const entry: AgentCmdEntry = {
        kind: "agent",
        block_id: e.block_id,
        command: e.command,
        output: "",
        code: null,
        needsInput: false,
        is_error: false,
        ts: Date.now(),
        lastOutputAt: Date.now(),
      };
      return { ...map, [e.chat_id]: [...list, entry] };
    });
    recomputeStatus(e.chat_id);
  });

  void ipc.onChatPtyOutput((e) => {
    const now = Date.now();
    termAgentEntriesByChat.update((map) => {
      const list = map[e.chat_id];
      if (!list) return map;
      const idx = list.findIndex((en) => en.block_id === e.block_id);
      if (idx === -1) return map;
      const cur = list[idx];
      const raw = cur.output + stripAnsi(e.data);
      const trimmed =
        raw.length > MAX_PTY_OUTPUT ? raw.slice(raw.length - MAX_PTY_OUTPUT) : raw;
      const next = [...list];
      next[idx] = {
        ...cur,
        output: trimmed,
        lastOutputAt: now,
        needsInput: entryNeedsInput({ ...cur, output: trimmed, lastOutputAt: now }, now),
      };
      return { ...map, [e.chat_id]: next };
    });
    recomputeStatus(e.chat_id);
  });

  void ipc.onChatPtyDone((e) => {
    termAgentEntriesByChat.update((map) => {
      const list = map[e.chat_id];
      if (!list) return map;
      const idx = list.findIndex((en) => en.block_id === e.block_id);
      if (idx === -1) return map;
      const next = [...list];
      next[idx] = { ...list[idx], code: e.code, needsInput: false, is_error: e.code !== 0 };
      return { ...map, [e.chat_id]: next };
    });
    recomputeStatus(e.chat_id);
  });

  // Re-evaluate needs_input for idle-but-running commands (a prompt that stopped
  // emitting output). Cheap: only chats with running entries are considered.
  setInterval(() => {
    const now = Date.now();
    const map = get(termAgentEntriesByChat);
    for (const chatId of Object.keys(map)) {
      const list = map[chatId];
      if (list && list.some((e) => e.code === null)) refreshNeedsInput(chatId, now);
    }
  }, 1000);
}

// Best-effort rebuild of agent entries from persisted message history. Tool
// results are joined onto assistant tool_use blocks in chat.ts openChat (by
// tool_call_id), so block.result is available here. block_ids already present
// are skipped; entries without a stored result are skipped (a past command
// without a persisted result is not running).
export function reconstructFromMessages(chatId: string, messages: any[]): void {
  const known = new Set((get(termAgentEntriesByChat)[chatId] ?? []).map((e) => e.block_id));
  // Cheap first pass: collect new run_command blocks. If none, exit early so
  // streaming text deltas don't do work on each token.
  const pending: { block: any; msg: any }[] = [];
  for (const msg of messages) {
    if (!msg || msg.role !== "assistant" || !Array.isArray(msg.blocks)) continue;
    for (const block of msg.blocks) {
      if (block && block.type === "tool_use" && block.name === "run_command" && block.id && !known.has(block.id)) {
        pending.push({ block, msg });
      }
    }
  }
  if (pending.length === 0) return;
  const rebuilt: AgentCmdEntry[] = [];
  for (const { block, msg } of pending) {
    known.add(block.id);
    let command = "";
    try {
      const input = block.input ? JSON.parse(block.input) : null;
      command = input && typeof input.command === "string" ? input.command : "";
    } catch {
      command = "";
    }
    const result = typeof block.result === "string" ? block.result : undefined;
    if (typeof result !== "string") continue; // no stored result → not running, skip
    const isError = Boolean(block.is_error);
    let code: number;
    let output: string;
    const matched = /^\[exit (-?\d+)\]\n([\s\S]*)$/.exec(result);
    if (matched) {
      code = Number(matched[1]);
      output = matched[2];
    } else if (result.startsWith("[timed out")) {
      const nl = result.indexOf("\n");
      // Timed out: command is no longer running; -1 marks non-zero exit.
      code = -1;
      output = nl >= 0 ? result.slice(nl + 1) : "";
    } else {
      // Non-[exit] result (e.g. "pty spawn failed: ..."). Use is_error flag; treat as completed.
      code = isError ? -1 : 0;
      output = result;
    }
    // Stored results contain raw PTY bytes (ANSI escapes); strip for clean display,
    // matching the live onChatPtyOutput path which strips per-chunk.
    output = stripAnsi(output);
    rebuilt.push({
      kind: "agent",
      block_id: block.id,
      command,
      output,
      code,
      needsInput: false,
      is_error: isError,
      ts: typeof msg.created_at === "number" ? msg.created_at : 0,
      lastOutputAt: 0,
    });
  }
  if (rebuilt.length === 0) return;
  termAgentEntriesByChat.update((map) => {
    const cur = map[chatId] ?? [];
    const ids = new Set(cur.map((e) => e.block_id));
    return { ...map, [chatId]: [...cur, ...rebuilt.filter((e) => !ids.has(e.block_id))] };
  });
  recomputeStatus(chatId);
}

function safeTitle(title: string): string {
  const s = title
    .trim()
    .replace(/[\\/:*?"<>|]/g, "_")
    .replace(/\s+/g, " ")
    .trim();
  return s.length > 0 ? s : "chat";
}

export async function exportTerminalToFile(
  chatId: string,
  chatTitle: string,
): Promise<boolean> {
  try {
    let text = "";
    for (const e of get(termAgentEntriesByChat)[chatId] ?? []) {
      text += `$ ${e.command}\n${e.output}\n[exit ${e.code === null ? "running" : e.code}]\n\n`;
    }
    if (!text) return false;
    const path = await save({
      defaultPath: `${safeTitle(chatTitle)}-terminal.txt`,
      filters: [{ name: "Text", extensions: ["txt"] }],
    });
    if (!path) return false;
    await ipc.writeTextFile(path, text);
    toast.success(m.terminal_export_done());
    return true;
  } catch (err) {
    console.error("exportTerminalToFile failed", err);
    toast.error(String(err));
    return false;
  }
}

initTerminalListeners();