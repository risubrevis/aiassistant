import { writable, get } from "svelte/store";
import * as ipc from "$lib/tauri";
import * as tasksStore from "$lib/stores/tasks";
import { loadProjectChats, projectChats } from "$lib/stores/project";
import { loadProjectTasks } from "$lib/stores/projectTasks";
import { boardProjectId, homeView, promptsView } from "$lib/stores/app";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "$lib/stores/toasts";
import { m } from "$lib/i18n";
import type {
  Attachment,
  Chat,
  ChatSession,
  ChatStatus,
  ContentBlock,
  Message,
  Usage,
} from "$lib/tauri";

export type { Attachment };

export interface UiMessage {
  id: string;
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  blocks: ContentBlock[];
  streaming: boolean;
  model?: string | null;
  usage?: Usage | null;
  finish_reason?: string | null;
  thinking_ms?: number | null;
  created_at: number;
}

export const chats = writable<Chat[]>([]);
export const currentChatId = writable<string | null>(null);
export const messagesByChat = writable<Record<string, UiMessage[]>>({});
export const statusByChat = writable<Record<string, ChatStatus>>({});
export const sessionsByChat = writable<Record<string, ChatSession[]>>({});
export const pendingApprovals = writable<Record<string, ipc.ApprovalRequestEvent>>({});
export const pendingByChat = writable<Record<string, ipc.ChangeInfo[]>>({});
export interface PtyState { session_id: string; command: string; output: string; code: number | null; }
export const ptyByBlock = writable<Record<string, PtyState>>({});
export const attachmentsByChat = writable<Record<string, Attachment[]>>({});

export interface TurnError {
  chat_id: string;
  message_id: string;
  kind: string;
  message: string;
  retryable: boolean;
}
export const turnErrorByMessage = writable<Record<string, TurnError>>({});

export interface StreamRetry {
  attempt: number;
  max_attempts: number;
  detail: string;
}
export const streamRetryByMessage = writable<Record<string, StreamRetry>>({});

export type ChatSortMode = "updated" | "created" | "alpha" | "manual";
export const chatSortMode = writable<ChatSortMode>(
  typeof localStorage !== "undefined"
    ? ((localStorage.getItem("aiassistant.chatSortMode") as ChatSortMode | null) ?? "updated")
    : "updated",
);
chatSortMode.subscribe((v) => {
  try {
    localStorage.setItem("aiassistant.chatSortMode", v);
  } catch {}
});

export function sortChats(list: Chat[], mode: ChatSortMode): Chat[] {
  const pinnedFirst = (a: Chat, b: Chat) => b.pinned - a.pinned;
  const cmp: (a: Chat, b: Chat) => number =
    mode === "alpha"
      ? (a, b) => a.title.localeCompare(b.title)
      : mode === "created"
        ? (a, b) => b.created_at - a.created_at
        : mode === "manual"
          ? (a, b) => a.sort_order - b.sort_order
          : (a, b) => b.updated_at - a.updated_at;
  return [...list].sort((a, b) => pinnedFirst(a, b) || cmp(a, b));
}

const loaded = new Set<string>();

function parseBlocks(msg: Message): ContentBlock[] {
  if (msg.content_parts) {
    try {
      const parsed = JSON.parse(msg.content_parts) as { blocks?: ContentBlock[] };
      if (parsed.blocks && Array.isArray(parsed.blocks)) return parsed.blocks;
    } catch {
      // fall through
    }
  }
  return [{ id: "b-text", type: "text", text: msg.content }];
}

function toUi(msg: Message): UiMessage {
  return {
    id: msg.id,
    role: msg.role,
    content: msg.content,
    blocks: parseBlocks(msg),
    streaming: false,
    model: msg.model,
    usage: msg.usage ? safeParseUsage(msg.usage) : null,
    finish_reason: msg.finish_reason,
    thinking_ms: msg.thinking_ms,
    created_at: msg.created_at,
  };
}

function safeParseUsage(s: string): Usage | null {
  try {
    return JSON.parse(s) as Usage;
  } catch {
    return null;
  }
}

export async function loadChats() {
  try {
    chats.set(await ipc.chatList());
  } catch (e) {
    console.error("chatList failed", e);
  }
}

export async function openChat(id: string) {
  currentChatId.set(id);
  homeView.set(false);
  boardProjectId.set(null);
  promptsView.set(false);
  ipc
    .taskList(id)
    .then((list) => tasksStore.applyTasksUpdate(id, list))
    .catch((e) => console.warn("taskList failed", e));
  if (!loaded.has(id)) {
    try {
      const msgs = await ipc.chatMessages(id);
      // Tool results live in separate role:"tool" messages; persisted assistant
      // tool_use blocks have no result. Join them by tool_call_id so tool-call
      // cards (and the terminal panel) can show past command output. Tool-role
      // messages themselves are hidden as bubbles (rendered inline as cards).
      const toolResults = new Map<string, { result: string; is_error: boolean }>();
      for (const tm of msgs) {
        if (tm.role !== "tool" || typeof tm.content !== "string") continue;
        let callId: string | undefined;
        let isError = false;
        if (typeof tm.content_parts === "string" && tm.content_parts.length > 0) {
          try {
            const p = JSON.parse(tm.content_parts);
            callId = typeof p?.tool_call_id === "string" ? p.tool_call_id : undefined;
            isError = Boolean(p?.is_error);
          } catch {
            // malformed content_parts — skip
          }
        }
        if (callId) toolResults.set(callId, { result: tm.content, is_error: isError });
      }
      const uiMsgs = msgs
        .filter((x) => x.role !== "tool")
        .map((m) => {
          const u = toUi(m);
          if (toolResults.size && u.blocks.some((b) => b.type === "tool_use" && b.tool_call_id)) {
            u.blocks = u.blocks.map((b) => {
              if (b.type !== "tool_use" || !b.tool_call_id) return b;
              const r = toolResults.get(b.tool_call_id);
              if (!r) return b;
              return { ...b, result: r.result, is_error: r.is_error, status: r.is_error ? "error" : "done" };
            });
          }
          return u;
        });
      messagesByChat.update((m) => ({ ...m, [id]: uiMsgs }));
      loaded.add(id);
    } catch (e) {
      console.error("chatMessages failed", e);
    }
    try {
      const sessions = await ipc.chatSessionsList(id);
      sessions.sort((a, b) => b.created_at - a.created_at);
      sessionsByChat.update((m) => ({ ...m, [id]: sessions }));
    } catch (e) {
      console.error("chatSessionsList failed", e);
    }
    try {
      const atts = await ipc.attachmentsForChat(id);
      attachmentsByChat.update((m) => ({ ...m, [id]: atts }));
    } catch (e) {
      console.error("attachmentsForChat failed", e);
      attachmentsByChat.update((m) => ({ ...m, [id]: [] }));
    }
  }
  statusByChat.update((s) => ({ ...s, [id]: s[id] ?? "idle" }));
}

export async function newChat(projectId?: string | null) {
  try {
    const chat = projectId ? await ipc.chatCreateInProject(projectId) : await ipc.chatCreate();
    chats.update((list) => [chat, ...list]);
    if (projectId) {
      void loadProjectChats(projectId);
    }
    messagesByChat.update((m) => ({ ...m, [chat.id]: [] }));
    loaded.add(chat.id);
    statusByChat.update((s) => ({ ...s, [chat.id]: "idle" }));
    attachmentsByChat.update((m) => ({ ...m, [chat.id]: [] }));
    await openChat(chat.id);
  } catch (e) {
    console.error("chatCreate failed", e);
  }
}

// No try/catch: errors propagate to the caller to show a toast.
export async function deleteChat(id: string) {
  const chat = get(chats).find((c) => c.id === id);
  const projectId = chat?.project_id ?? null;
  await ipc.chatDelete(id);
  chats.update((list) => list.filter((c) => c.id !== id));
  messagesByChat.update((m) => {
    const next = { ...m };
    delete next[id];
    return next;
  });
  sessionsByChat.update((m) => {
    const next = { ...m };
    delete next[id];
    return next;
  });
  attachmentsByChat.update((m) => {
    const next = { ...m };
    delete next[id];
    return next;
  });
  tasksStore.clearTasks(id);
  loaded.delete(id);
  if (get(currentChatId) === id) currentChatId.set(null);
  // Refresh the project's chat list and Kanban board: the deleted chat must
  // disappear from the sidebar, and any task linked to it must reflect the
  // DB's ON DELETE SET NULL (chat_id cleared) instead of stale in-memory data.
  if (projectId) {
    void loadProjectChats(projectId);
    void loadProjectTasks(projectId);
  }
}

export async function renameChat(id: string, title: string) {
  try {
    await ipc.chatRename(id, title);
    chats.update((list) => list.map((c) => (c.id === id ? { ...c, title } : c)));
  } catch (e) {
    console.error("chatRename failed", e);
  }
}

export async function toggleChatPinned(id: string, pinned: boolean) {
  try {
    await ipc.chatSetPinned(id, pinned);
    const val = pinned ? 1 : 0;
    chats.update((list) => list.map((c) => (c.id === id ? { ...c, pinned: val } : c)));
    // Project chats are rendered from the projectChats store, not the global
    // chats store — mirror the pin there too so the star updates and the chat
    // sorts to the top of its project immediately.
    projectChats.update((m) => {
      const next = { ...m };
      for (const pid of Object.keys(next)) {
        next[pid] = next[pid].map((c) => (c.id === id ? { ...c, pinned: val } : c));
      }
      return next;
    });
  } catch (e) {
    console.error("chatSetPinned failed", e);
  }
}

export async function reorderChats(orderedIds: string[]) {
  try {
    await ipc.chatReorder(orderedIds);
    chats.update((list) => {
      const order = new Map(orderedIds.map((id, i) => [id, i]));
      return list.map((c) => ({ ...c, sort_order: order.get(c.id) ?? c.sort_order }));
    });
  } catch (e) {
    console.error("chatReorder failed", e);
  }
}

export async function moveChatToProject(chatId: string, projectId: string | null) {
  try {
    const prevProjectId = get(chats).find((c) => c.id === chatId)?.project_id ?? null;
    await ipc.chatSetProject(chatId, projectId);
    chats.update((list) => {
      const maxSort = list
        .filter((c) => (projectId ? c.project_id === projectId : !c.project_id))
        .reduce((max, c) => Math.max(max, c.sort_order), -1);
      return list.map((c) =>
        c.id === chatId ? { ...c, project_id: projectId, sort_order: maxSort + 1 } : c,
      );
    });
    if (projectId) void loadProjectChats(projectId);
    if (prevProjectId && prevProjectId !== projectId) void loadProjectChats(prevProjectId);
  } catch (e) {
    console.error("chatSetProject failed", e);
  }
}

export async function setChatModel(
  id: string,
  providerId: string | null,
  modelId: string | null,
) {
  try {
    await ipc.chatSetModel(id, providerId, modelId);
    chats.update((list) =>
      list.map((c) =>
        c.id === id ? { ...c, provider_id: providerId, model_id: modelId } : c,
      ),
    );
  } catch (e) {
    console.error("chatSetModel failed", e);
  }
}

export async function setChatThinking(id: string, enabled: boolean, effort: string) {
  try {
    await ipc.chatSetThinking(id, enabled, effort);
  } catch (e) {
    console.error("chatSetThinking failed", e);
  }
}

function mergeSetting(settings: string | null, key: string, value: string): string {
  let obj: Record<string, unknown> = {};
  try {
    if (settings) obj = JSON.parse(settings) as Record<string, unknown>;
  } catch {
    obj = {};
  }
  obj[key] = value;
  return JSON.stringify(obj);
}

export async function setChatMode(id: string, mode: string) {
  try {
    await ipc.chatSetMode(id, mode);
    chats.update((list) =>
      list.map((c) =>
        c.id === id ? { ...c, settings: mergeSetting(c.settings, "mode", mode) } : c,
      ),
    );
  } catch (e) {
    console.error("chatSetMode failed", e);
  }
}

export async function setChatCommandToggle(id: string, value: string) {
  try {
    await ipc.chatSetCommandToggle(id, value);
    chats.update((list) =>
      list.map((c) =>
        c.id === id
          ? { ...c, settings: mergeSetting(c.settings, "command_toggle", value) }
          : c,
      ),
    );
  } catch (e) {
    console.error("chatSetCommandToggle failed", e);
  }
}

export async function setChatEditToggle(id: string, value: string) {
  try {
    await ipc.chatSetEditToggle(id, value);
    chats.update((list) =>
      list.map((c) =>
        c.id === id
          ? { ...c, settings: mergeSetting(c.settings, "edit_toggle", value) }
          : c,
      ),
    );
  } catch (e) {
    console.error("chatSetEditToggle failed", e);
  }
}

export interface RightPanelState {
  open: boolean;
  mode: "context" | "terminal";
  width: number;
}

export function chatRightPanel(chat: Chat, defaultWidth: number): RightPanelState {
  try {
    const s = chat.settings ? (JSON.parse(chat.settings) as Record<string, string>) : null;
    const rp = s?.right_panel ? (JSON.parse(s.right_panel) as Partial<RightPanelState>) : null;
    return {
      open: rp?.open ?? true,
      mode: rp?.mode === "terminal" ? "terminal" : "context",
      width: typeof rp?.width === "number" ? rp.width : defaultWidth,
    };
  } catch {
    return { open: true, mode: "context", width: defaultWidth };
  }
}

export async function setChatRightPanel(id: string, open: boolean, mode: string, width: number) {
  try {
    await ipc.chatSetRightPanel(id, open, mode, width);
    const value = JSON.stringify({ open, mode, width });
    chats.update((list) =>
      list.map((c) =>
        c.id === id
          ? { ...c, settings: mergeSetting(c.settings, "right_panel", value) }
          : c,
      ),
    );
  } catch (e) {
    console.error("chatSetRightPanel failed", e);
  }
}

export async function sendMessage(text: string, attachmentIds: string[] = [], skillIds: string[] = []) {
  const id = get(currentChatId);
  if (!id || (!text.trim() && attachmentIds.length === 0)) return;
  // A chat without a selected model cannot be sent to.
  const chat = get(chats).find((c) => c.id === id);
  if (!chat?.model_id) return;
  const now = Date.now();
  const userMsg: UiMessage = {
    id: `local-${now}`,
    role: "user",
    content: text,
    blocks: [{ id: "b-text", type: "text", text }],
    streaming: false,
    created_at: now,
  };
  messagesByChat.update((m) => ({ ...m, [id]: [...(m[id] ?? []), userMsg] }));
  statusByChat.update((s) => ({ ...s, [id]: "running" }));
  try {
    await ipc.chatSend(id, text, attachmentIds, skillIds);
    const atts = await ipc.attachmentsForChat(id);
    attachmentsByChat.update((m) => ({ ...m, [id]: atts }));
  } catch (e) {
    console.error("chatSend failed", e);
    statusByChat.update((s) => ({ ...s, [id]: "error" }));
  }
}

function buildSynthesisPrompt(parentId: string, anyError: boolean): string {
  const note = anyError ? "Some runs finished with errors." : "All runs finished successfully.";
  return `Background agent batch completed (parent_id: ${parentId}). ${note} Call agent__batch_results with parent_id="${parentId}" to retrieve the full results, then synthesize a concise summary, highlight any issues, and propose next steps.`;
}

export async function sendBackgroundSynthesis(chatId: string, parentId: string, anyError: boolean) {
  const prompt = buildSynthesisPrompt(parentId, anyError);
  const now = Date.now();
  const userMsg: UiMessage = {
    id: `local-synth-${now}`,
    role: "user",
    content: prompt,
    blocks: [{ id: "b-synth", type: "text", text: prompt }],
    streaming: false,
    created_at: now,
  };
  messagesByChat.update((m) => ({ ...m, [chatId]: [...(m[chatId] ?? []), userMsg] }));
  statusByChat.update((s) => ({ ...s, [chatId]: "running" }));
  try {
    await ipc.chatSend(chatId, prompt, [], []);
  } catch (e) {
    console.error("sendBackgroundSynthesis failed", e);
    statusByChat.update((s) => ({ ...s, [chatId]: "error" }));
  }
}

export async function cancelTurn() {
  const id = get(currentChatId);
  if (!id) return;
  try {
    await ipc.chatCancel(id);
  } catch (e) {
    console.error("chatCancel failed", e);
  }
}

export async function regenerateMessage(messageId: string) {
  const id = get(currentChatId);
  if (!id) return;
  try {
    await ipc.chatRegenerate(id, messageId);
    statusByChat.update((s) => ({ ...s, [id]: "running" }));
  } catch (e) {
    console.error("chatRegenerate failed", e);
  }
}

export async function retryTurn(messageId: string) {
  turnErrorByMessage.update((m) => { const n = { ...m }; delete n[messageId]; return n; });
  await regenerateMessage(messageId);
}

export async function editMessage(messageId: string, newText: string) {
  const id = get(currentChatId);
  if (!id) return;
  try {
    await ipc.chatEditMessage(id, messageId, newText);
    statusByChat.update((s) => ({ ...s, [id]: "running" }));
  } catch (e) {
    console.error("chatEditMessage failed", e);
  }
}

function sanitizeFileName(name: string): string {
  const s = name
    .trim()
    .replace(/[\\/:*?"<>|]/g, "_")
    .replace(/\s+/g, " ")
    .trim();
  return s.length > 0 ? s : "chat";
}

function exportFileName(chatTitle: string): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const ts = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`;
  return `${ts}_${sanitizeFileName(chatTitle)}.md`;
}

export async function generateMarkdown(chatId: string): Promise<string> {
  return ipc.chatExportMarkdown(chatId);
}

export async function saveMarkdownContent(content: string, chatTitle: string): Promise<boolean> {
  const path = await save({
    defaultPath: exportFileName(chatTitle),
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  if (!path) return false;
  await ipc.writeTextFile(path, content);
  return true;
}

export async function exportMarkdown(): Promise<void> {
  const id = get(currentChatId);
  if (!id) return;
  try {
    const content = await generateMarkdown(id);
    const title = get(chats).find((c) => c.id === id)?.title ?? "chat";
    const saved = await saveMarkdownContent(content, title);
    if (saved) toast.success(m.chat_export_done());
  } catch (e) {
    console.error("exportMarkdown failed", e);
    toast.error(m.chat_export_failed());
  }
}

export async function compactChat(id: string): Promise<ChatSession> {
  const s = await ipc.chatCompact(id);
  applyCompacted(s);
  return s;
}

// --- streaming event reducers ---

function ensureAssistant(chatId: string, messageId: string): UiMessage {
  let existing: UiMessage | undefined;
  messagesByChat.update((m) => {
    const list = m[chatId] ?? [];
    existing = list.find((x) => x.id === messageId);
    if (!existing) {
      existing = {
        id: messageId,
        role: "assistant",
        content: "",
        blocks: [],
        streaming: true,
        created_at: Date.now(),
      };
      m = { ...m, [chatId]: [...list, existing] };
    }
    return m;
  });
  return existing!;
}

export function applyBlockStart(e: ipc.BlockStartEvent) {
  const before = get(messagesByChat)[e.chat_id] ?? [];
  const existed = before.some((x) => x.id === e.message_id);
  ensureAssistant(e.chat_id, e.message_id);
  messagesByChat.update((m) => {
    const list = m[e.chat_id] ?? [];
    let base = list;
    // New assistant iteration started → previous iterations in this turn are done streaming.
    if (!existed) {
      const hasStreaming = list.some((x) => x.id !== e.message_id && x.role === "assistant" && x.streaming);
      if (hasStreaming) {
        base = list.map((x) =>
          x.id !== e.message_id && x.role === "assistant" && x.streaming
            ? { ...x, streaming: false }
            : x,
        );
      }
    }
    const idx = base.findIndex((x) => x.id === e.message_id);
    if (idx < 0) return m;
    const msg = base[idx];
    if (msg.blocks.some((b) => b.id === e.block_id)) return m;
    const block: ContentBlock =
      e.block_type === "tool_use"
        ? {
            id: e.block_id,
            type: "tool_use",
            name: e.info?.name ?? "",
            tool_call_id: e.info?.tool_call_id ?? "",
            input: "",
            status: "running",
          }
        : { id: e.block_id, type: e.block_type, text: "" };
    const next = [...base];
    next[idx] = { ...msg, blocks: [...msg.blocks, block] };
    return { ...m, [e.chat_id]: next };
  });
}

export function applyBlockDelta(e: ipc.BlockDeltaEvent) {
  messagesByChat.update((m) => {
    const list = m[e.chat_id] ?? [];
    const idx = list.findIndex((x) => x.id === e.message_id);
    if (idx < 0) return m;
    const msg = list[idx];
    const next = [...list];
    next[idx] = {
      ...msg,
      blocks: msg.blocks.map((b) => {
        if (b.id !== e.block_id) return b;
        if (e.partial_json) {
          return { ...b, input: (b.input ?? "") + e.partial_json };
        }
        return { ...b, text: (b.text ?? "") + (e.text ?? "") };
      }),
    };
    return { ...m, [e.chat_id]: next };
  });
}

export function applyBlockStop(e: ipc.BlockStopEvent) {
  messagesByChat.update((m) => {
    const list = m[e.chat_id] ?? [];
    const idx = list.findIndex((x) => x.id === e.message_id);
    if (idx < 0) return m;
    const msg = list[idx];
    const next = [...list];
    next[idx] = {
      ...msg,
      blocks: msg.blocks.map((b) =>
        b.id === e.block_id && b.type === "tool_use" && b.status === "running"
          ? { ...b, status: "done" }
          : b,
      ),
    };
    return { ...m, [e.chat_id]: next };
  });
}

export function applyToolResult(e: ipc.ToolResultEvent) {
  messagesByChat.update((m) => {
    const list = m[e.chat_id] ?? [];
    const idx = list.findIndex((x) => x.id === e.message_id);
    if (idx < 0) return m;
    const msg = list[idx];
    const next = [...list];
    next[idx] = {
      ...msg,
      blocks: msg.blocks.map((b) =>
        b.id === e.block_id
          ? { ...b, result: e.result, is_error: e.is_error, status: e.is_error ? "error" : "done" }
          : b,
      ),
    };
    pendingApprovals.update((pa) => {
      if (pa[e.block_id]) {
        const next = { ...pa };
        delete next[e.block_id];
        return next;
      }
      return pa;
    });
    return { ...m, [e.chat_id]: next };
  });
}

export function applyApprovalRequest(e: ipc.ApprovalRequestEvent) {
  pendingApprovals.update((m) => ({ ...m, [e.block_id]: e }));
}

export function applyPendingUpdate(e: ipc.PendingUpdateEvent) {
  pendingByChat.update((m) => ({ ...m, [e.chat_id]: e.changes }));
}

export function applyPtyStart(e: ipc.PtyStartEvent) {
  ptyByBlock.update((m) => ({ ...m, [e.block_id]: { session_id: e.session_id, command: e.command, output: "", code: null } }));
}
export function applyPtyOutput(e: ipc.PtyOutputEvent) {
  ptyByBlock.update((m) => {
    const cur = m[e.block_id];
    if (!cur) return m;
    return { ...m, [e.block_id]: { ...cur, output: cur.output + e.data } };
  });
}
export function applyPtyDone(e: ipc.PtyDoneEvent) {
  ptyByBlock.update((m) => {
    const cur = m[e.block_id];
    if (!cur) return m;
    return { ...m, [e.block_id]: { ...cur, code: e.code } };
  });
}

export async function approvePending(chatId: string) {
  try { await ipc.pendingApprove(chatId); } catch (e) { console.error(e); }
  pendingByChat.update((m) => { const n = { ...m }; delete n[chatId]; return n; });
}

export async function rejectPending(chatId: string) {
  try { await ipc.pendingReject(chatId); } catch (e) { console.error(e); }
  pendingByChat.update((m) => { const n = { ...m }; delete n[chatId]; return n; });
}

export function applyMessageDone(e: ipc.MessageDoneEvent) {
  messagesByChat.update((m) => {
    const list = m[e.chat_id] ?? [];
    const idx = list.findIndex((x) => x.id === e.message_id);
    if (idx < 0) return m;
    const msg = list[idx];
    const text = msg.blocks
      .filter((b) => b.type === "text")
      .map((b) => b.text ?? "")
      .join("");
    const next = [...list];
    next[idx] = {
      ...msg,
      streaming: false,
      content: text || msg.content,
      usage: e.usage ?? null,
      finish_reason: e.finish_reason,
    };
    return { ...m, [e.chat_id]: next };
  });
  if (e.finish_reason !== "error") {
    turnErrorByMessage.update((m) => { const n = { ...m }; delete n[e.message_id]; return n; });
  }
  streamRetryByMessage.update((s) => { const n = { ...s }; delete n[e.message_id]; return n; });
}

export function applyTurnError(e: ipc.TurnErrorEvent) {
  ensureAssistant(e.chat_id, e.message_id);
  turnErrorByMessage.update((m) => ({ ...m, [e.message_id]: { ...e } }));
  statusByChat.update((s) => ({ ...s, [e.chat_id]: "error" }));
  streamRetryByMessage.update((s) => { const n = { ...s }; delete n[e.message_id]; return n; });
}

export function applyStreamRetry(e: ipc.StreamRetryEvent) {
  ensureAssistant(e.chat_id, e.message_id);
  messagesByChat.update((m) => {
    const list = m[e.chat_id] ?? [];
    const idx = list.findIndex((x) => x.id === e.message_id);
    if (idx < 0) return m;
    const next = [...list];
    next[idx] = { ...next[idx], blocks: [], streaming: true };
    return { ...m, [e.chat_id]: next };
  });
  streamRetryByMessage.update((s) => ({ ...s, [e.message_id]: { attempt: e.attempt, max_attempts: e.max_attempts, detail: e.detail } }));
}

export function applyStatus(e: ipc.StatusEvent) {
  statusByChat.update((s) => ({ ...s, [e.chat_id]: e.status }));
  // Terminal states: no turn is running for this chat. Clear any stuck
  // streaming flags and retry indicators (e.g. a cancel during a retry
  // backoff aborts the task without emitting message_done).
  if (e.status === "idle" || e.status === "cancelled") {
    const ids: string[] = [];
    messagesByChat.update((m) => {
      const list = m[e.chat_id] ?? [];
      if (!list.some((x) => x.streaming)) return m;
      const next = list.map((x) => {
        if (x.streaming) ids.push(x.id);
        return x.streaming ? { ...x, streaming: false } : x;
      });
      return { ...m, [e.chat_id]: next };
    });
    if (ids.length) {
      streamRetryByMessage.update((s) => {
        const n = { ...s };
        for (const id of ids) delete n[id];
        return n;
      });
    }
  }
}

export function applyChatRenamed(e: ipc.ChatRenamedEvent) {
  chats.update((list) => list.map((c) => (c.id === e.chat_id ? { ...c, title: e.title } : c)));
  projectChats.update((map) => {
    let changed = false;
    const next: Record<string, Chat[]> = {};
    for (const [pid, list] of Object.entries(map)) {
      if (list.some((c) => c.id === e.chat_id)) {
        next[pid] = list.map((c) => (c.id === e.chat_id ? { ...c, title: e.title } : c));
        changed = true;
      } else {
        next[pid] = list;
      }
    }
    return changed ? next : map;
  });
}

export function applyCompacted(s: ChatSession) {
  sessionsByChat.update((m) => {
    const list = (m[s.chat_id] ?? []).filter((x) => x.id !== s.id);
    return { ...m, [s.chat_id]: [s, ...list] };
  });
}