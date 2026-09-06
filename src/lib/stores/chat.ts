import { writable, get } from "svelte/store";
import * as ipc from "$lib/tauri";
import * as tasksStore from "$lib/stores/tasks";
import { loadProjectChats } from "$lib/stores/project";
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
      // Tool-role messages are rendered inline as tool-call cards; hide them as bubbles.
      messagesByChat.update((m) => ({ ...m, [id]: msgs.filter((x) => x.role !== "tool").map(toUi) }));
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

export async function deleteChat(id: string) {
  try {
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
    loaded.delete(id);
    if (get(currentChatId) === id) currentChatId.set(null);
  } catch (e) {
    console.error("chatDelete failed", e);
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
    chats.update((list) => list.map((c) => (c.id === id ? { ...c, pinned: pinned ? 1 : 0 } : c)));
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
  ensureAssistant(e.chat_id, e.message_id);
  messagesByChat.update((m) => {
    const list = m[e.chat_id] ?? [];
    const idx = list.findIndex((x) => x.id === e.message_id);
    if (idx < 0) return m;
    const msg = list[idx];
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
    const next = [...list];
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
}

export function applyTurnError(e: ipc.TurnErrorEvent) {
  ensureAssistant(e.chat_id, e.message_id);
  turnErrorByMessage.update((m) => ({ ...m, [e.message_id]: { ...e } }));
  statusByChat.update((s) => ({ ...s, [e.chat_id]: "error" }));
}

export function applyStatus(e: ipc.StatusEvent) {
  statusByChat.update((s) => ({ ...s, [e.chat_id]: e.status }));
}

export function applyChatRenamed(e: ipc.ChatRenamedEvent) {
  chats.update((list) => list.map((c) => (c.id === e.chat_id ? { ...c, title: e.title } : c)));
}

export function applyCompacted(s: ChatSession) {
  sessionsByChat.update((m) => {
    const list = (m[s.chat_id] ?? []).filter((x) => x.id !== s.id);
    return { ...m, [s.chat_id]: [s, ...list] };
  });
}