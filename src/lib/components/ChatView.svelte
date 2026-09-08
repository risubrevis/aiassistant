<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { messagesByChat, statusByChat, generateMarkdown, saveMarkdownContent, sessionsByChat, compactChat } from "$lib/stores/chat";
  import type { UiMessage } from "$lib/stores/chat";
  import { config as configStore } from "$lib/stores/config";
  import { projects } from "$lib/stores/project";
  import { configGet, setMode, setCommandToggle, setEditToggle, chatContextWindow, projectContextSummary, type Chat, type ChatSession, type ProjectContextSummary } from "$lib/tauri";
  import { m } from "$lib/i18n";
  import { toast } from "$lib/stores/toasts";
  import { renderMarkdown } from "$lib/markdown";
  import { Bot, Download, FileText, Info, ScrollText, ListTodo, ChevronDown, ChevronRight, LoaderCircle, Sparkles, PanelRight, PanelRightOpen } from "@lucide/svelte";
  import ModelSelector from "./ModelSelector.svelte";
  import MessageItem from "./MessageItem.svelte";
  import AssistantTurn from "./AssistantTurn.svelte";
  import Composer from "./Composer.svelte";
  import InteractionOverlay from "./InteractionOverlay.svelte";
  import ContextPanel from "./ContextPanel.svelte";
  import AgentPanel from "./AgentPanel.svelte";
  import AgentRunModal from "./AgentRunModal.svelte";
  import AgentInfoModal from "./AgentInfoModal.svelte";
  import ChatInfoModal from "./ChatInfoModal.svelte";
  import TasksModal from "./TasksModal.svelte";
  import {
    agents,
    agentRuns,
    agentsPanelOpen,
    agentInfoModalOpen,
    refreshAgentAvailability,
  } from "$lib/stores/agents";
  import { FOCUS_COMPOSER_EVENT, DROP_FILES_EVENT } from "$lib/events";
  import { tasksByChat, tasksModalChatId, openTasksModal, closeTasksModal, taskCounts } from "$lib/stores/tasks";
  import { rightSidebarOpen, setRightSidebarOpen, saveRightSidebarOpen } from "$lib/stores/layout";

  let { chat }: { chat: Chat } = $props();

  let showThinking = $state(false);
  let bottomEl: HTMLDivElement | undefined = $state();
  let lastChatId: string | null = null;
  let dragOver = $state(false);

  onMount(async () => {
    try {
      const cfg = await configGet();
      showThinking = cfg.appearance.show_thinking;
    } catch {
      // default false
    }
  });

  // Native drag-and-drop of files onto the chat area.
  onMount(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        const ev = e.payload;
        if (ev.type === "enter" || ev.type === "over") {
          dragOver = true;
        } else if (ev.type === "leave") {
          dragOver = false;
        } else if (ev.type === "drop") {
          dragOver = false;
          const paths = ev.paths ?? [];
          if (paths.length) {
            window.dispatchEvent(new CustomEvent(DROP_FILES_EVENT, { detail: { paths } }));
          }
        }
      })
      .then((u) => (unlisten = u))
      .catch((e) => console.error("onDragDropEvent failed", e));
    return () => unlisten?.();
  });

  let messages = $derived(($messagesByChat[chat.id] ?? []) as UiMessage[]);

  type RenderItem =
    | { key: string; kind: "assistant"; messages: UiMessage[] }
    | { key: string; kind: "single"; message: UiMessage };

  let renderItems = $derived.by<RenderItem[]>(() => {
    const items: RenderItem[] = [];
    let buf: UiMessage[] = [];
    const flush = () => {
      if (buf.length) {
        items.push({ key: "a:" + buf[0].id, kind: "assistant", messages: buf });
        buf = [];
      }
    };
    for (const msg of messages) {
      if (msg.role === "assistant") {
        buf.push(msg);
      } else {
        flush();
        items.push({ key: "s:" + msg.id, kind: "single", message: msg });
      }
    }
    flush();
    return items;
  });
  let sessions = $derived(($sessionsByChat[chat.id] ?? []) as ChatSession[]);
  let latestSession = $derived(sessions[0]);
  let summaryOpen = $state(false);
  let compacting = $state(false);
  let exporting = $state(false);
  let infoOpen = $state(false);
  let contextWindow = $state(0);
  let confirmOpen = $state(false);
  let compactionResult = $state<{
    compactedCount: number;
    remainingCount: number;
    tokenCount: number;
  } | null>(null);
  let status = $derived($statusByChat[chat.id] ?? "idle");
  let running = $derived(status === "running");
  let modelSelected = $derived(Boolean(chat.model_id));
  const ACTIVE_RUN_STATUSES = new Set(["queued", "running"]);
  let agentRunningInChat = $derived(
    $agentRuns.some((r) => r.chat_id === chat.id && ACTIVE_RUN_STATUSES.has(r.status)),
  );
  let agentTooltip = $derived(
    agentRunningInChat
      ? m.agents_running_in_chat()
      : $agents.length === 0
        ? m.agent_no_agent()
        : m.agent_available(),
  );
  let mode = $derived($configStore?.defaults.mode ?? "plan");
  let commandToggle = $derived($configStore?.defaults.command_toggle ?? "manual");
  let editToggle = $derived($configStore?.defaults.edit_toggle ?? "ask");
  let project = $derived($projects.find((p) => p.id === chat.project_id) ?? null);
  let counts = $derived(taskCounts($tasksByChat[chat.id] ?? []));
  let ctxSummary = $state<ProjectContextSummary | null>(null);
  $effect(() => {
    const pid = chat.project_id;
    if (!pid) {
      ctxSummary = null;
      return;
    }
    projectContextSummary(pid)
      .then((s) => (ctxSummary = s))
      .catch(() => (ctxSummary = null));
  });
  let ctxTooltip = $derived.by(() => {
    if (!ctxSummary) return "";
    const parts: string[] = [];
    if (ctxSummary.rule_files.length) parts.push(ctxSummary.rule_files.join(", "));
    if (ctxSummary.skills.length) parts.push(`${ctxSummary.skills.length} ${m.context_skills()}`);
    return parts.join(" · ");
  });

  let thresholdPct = $derived($configStore?.defaults.auto_collapse_context_pct ?? 90);

  async function loadContextWindow() {
    try {
      contextWindow = await chatContextWindow(chat.id);
    } catch {
      contextWindow = 0;
    }
  }

  $effect(() => {
    void chat.id;
    void chat.model_id;
    void chat.provider_id;
    void $configStore;
    void loadContextWindow();
  });

  let lastPromptUsage = $derived.by(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i];
      if (m.role === "assistant" && m.usage && m.usage.prompt_tokens > 0) {
        return { tokens: m.usage.prompt_tokens, createdAt: m.created_at };
      }
    }
    return null;
  });

  let contextTokens = $derived.by(() => {
    if (lastPromptUsage && (!latestSession || lastPromptUsage.createdAt >= latestSession.created_at)) {
      return lastPromptUsage.tokens;
    }
    if (latestSession) {
      const idx = messages.findIndex((m) => m.id === latestSession.boundary_message_id);
      const tail = idx >= 0 ? messages.slice(idx) : messages;
      const chars = tail.reduce((s, m) => s + m.content.length, 0) + latestSession.summary.length;
      return Math.round(chars / 4);
    }
    if (messages.length > 0) {
      const chars = messages.reduce((s, m) => s + m.content.length, 0);
      return Math.round(chars / 4);
    }
    return 0;
  });

  type CompactColor = "grey" | "green" | "orange" | "red";
  let compactColor = $derived.by<CompactColor>(() => {
    if (messages.length === 0) return "grey";
    if (thresholdPct <= 0 || contextWindow <= 0) return "green";
    const ratio = contextTokens / contextWindow;
    const threshold = thresholdPct / 100;
    if (ratio >= threshold) return "red";
    if (ratio >= threshold - 0.1) return "orange";
    return "green";
  });

  const MODES = ["minimal", "plan", "write"] as const;

  function renderSummary(summary: string): string {
    return renderMarkdown(summary);
  }

  function askCompact() {
    if (compacting || running || messages.length === 0) return;
    confirmOpen = true;
  }

  async function runCompact() {
    confirmOpen = false;
    if (compacting || running) return;
    compacting = true;
    try {
      const s = await compactChat(chat.id);
      const idx = messages.findIndex((m) => m.id === s.boundary_message_id);
      const compactedCount = idx >= 0 ? idx : 0;
      const remainingCount = idx >= 0 ? messages.length - idx : messages.length;
      compactionResult = { compactedCount, remainingCount, tokenCount: s.token_count };
      toast.success(
        m.compaction_done(),
        `${m.compaction_result_compacted()}: ${compactedCount} · ${m.compaction_result_remaining()}: ${remainingCount}`,
      );
    } catch (e) {
      const msg = String(e);
      if (msg.includes("too short")) {
        toast.warning(m.compaction_too_short());
      } else {
        toast.error(m.compaction_failed(), msg);
      }
    } finally {
      compacting = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") confirmOpen = false;
  }

  async function doExport() {
    if (exporting) return;
    exporting = true;
    try {
      const content = await generateMarkdown(chat.id);
      exporting = false;
      const ok = await saveMarkdownContent(content, chat.title);
      if (ok) toast.success(m.chat_export_done());
    } catch (e) {
      console.error("export failed", e);
      toast.error(m.chat_export_failed());
    } finally {
      exporting = false;
    }
  }

  $effect(() => {
    void messages;
    void compactionResult;
    void tick().then(() => bottomEl?.scrollIntoView({ behavior: "smooth" }));
  });

  $effect(() => {
    void $agents;
    void refreshAgentAvailability();
  });

  $effect(() => {
    const id = chat.id;
    if (id !== lastChatId) {
      const empty = untrack(() => ($messagesByChat[id] ?? []).length === 0);
      if (empty || modelSelected) {
        void tick().then(() => window.dispatchEvent(new CustomEvent(FOCUS_COMPOSER_EVENT)));
      }
      compactionResult = null;
      confirmOpen = false;
    }
    lastChatId = id;
  });
</script>

<div class="chat-wrap flex min-h-0 flex-1">
  <div class="chat-view flex min-h-0 flex-1 flex-col">
    <header class="chat-header">
      {#if project}
        <span class="pbadge" style="background:{project.color || '#6366f1'}"></span>
        <span class="pname" title={project.name}>{project.name}</span>
        {#if ctxSummary && (ctxSummary.rule_files.length || ctxSummary.skills.length)}
          <span class="ctx-badge" title={ctxTooltip}>
            <FileText size={12} />
            <span>{ctxSummary.rule_files.length}</span>
            <Sparkles size={12} />
            <span>{ctxSummary.skills.length}</span>
          </span>
        {/if}
        <span class="sep">/</span>
      {/if}
      <span class="title" class:thinking={running}>{chat.title}</span>
      <button
        class="export-btn"
        title={exporting ? m.chat_exporting() : m.chat_export()}
        disabled={exporting}
        onclick={() => void doExport()}
      >
        {#if exporting}
          <LoaderCircle size={15} class="spin" />
        {:else}
          <Download size={15} />
        {/if}
      </button>
      <button class="export-btn" title={m.info_title()} onclick={() => (infoOpen = true)}>
        <Info size={15} />
      </button>
      <button
        class="export-btn"
        title={$rightSidebarOpen ? m.context_toggle_collapse() : m.context_toggle_expand()}
        onclick={() => { const v = !$rightSidebarOpen; setRightSidebarOpen(v); saveRightSidebarOpen(v); }}
      >
        {#if $rightSidebarOpen}<PanelRightOpen size={15} />{:else}<PanelRight size={15} />{/if}
      </button>
    </header>

    <div class="messages flex-1 overflow-y-auto px-4 py-2">
      {#if latestSession}
        <div class="compaction-banner">
          <button class="cb-head" onclick={() => (summaryOpen = !summaryOpen)}>
            {#if summaryOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
            <FileText size={14} />
            <span class="cb-title">{m.compaction_summary()}</span>
            <span class="cb-meta">· {latestSession.token_count} tokens</span>
          </button>
          {#if summaryOpen}
            <div class="cb-body">{@html renderSummary(latestSession.summary)}</div>
          {/if}
        </div>
      {/if}
      {#each renderItems as item (item.key)}
        {#if item.kind === "assistant"}
          <AssistantTurn messages={item.messages} showThinking={showThinking} />
        {:else}
          <MessageItem message={item.message} showThinking={showThinking} />
        {/if}
      {/each}
      {#if compactionResult}
        <div class="compact-result">
          <div class="cr-head">
            <ScrollText size={14} />
            <span>{m.compaction_result_title()}</span>
          </div>
          <div class="cr-grid">
            <div class="cr-row">
              <span class="cr-lbl">{m.compaction_result_compacted()}</span>
              <b class="cr-val">{compactionResult.compactedCount}</b>
            </div>
            <div class="cr-row">
              <span class="cr-lbl">{m.compaction_result_remaining()}</span>
              <b class="cr-val">{compactionResult.remainingCount}</b>
            </div>
            <div class="cr-row">
              <span class="cr-lbl">{m.compaction_result_summary()}</span>
              <b class="cr-val">{compactionResult.tokenCount} {m.compaction_result_tokens()}</b>
            </div>
          </div>
        </div>
      {/if}
      {#if messages.length === 0}
        <div class="empty">{m.sidebar_no_chats()}</div>
      {/if}
      <div bind:this={bottomEl}></div>
    </div>

    {#if $agentsPanelOpen}
      <AgentPanel />
    {/if}
    <div class="composer-area">
      <InteractionOverlay chatId={chat.id} />
      <Composer running={running} inProject={!!project} modelSelected={modelSelected}>
      <button
        class="ag-btn {agentRunningInChat ? "running" : ""}"
        title={agentTooltip}
        onclick={() => agentInfoModalOpen.set(true)}
      >
        <Bot size={14} />
      </button>
      <button class="export-btn tasks-btn" title={m.tasks_title()} onclick={() => openTasksModal(chat.id)}>
        <ListTodo size={15} />
        {#if counts.total > 0}
          <span class="tasks-chip" class:open={counts.hasOpen} class:done={!counts.hasOpen}>
            {counts.done}/{counts.total}
          </span>
        {/if}
      </button>
      <button
        class="export-btn compact-btn {compactColor}"
        title={compacting ? m.compaction_compacting() : m.compaction_compact_now()}
        disabled={compacting || running || messages.length === 0}
        onclick={askCompact}
      >
        {#if compacting}
          <LoaderCircle size={15} class="spin" />
        {:else}
          <ScrollText size={15} />
        {/if}
      </button>
      <div class="modes" title="Mode">
        {#each MODES as md}
          <button class="mode-btn" class:active={mode === md} onclick={() => setMode(md)}>{md}</button>
        {/each}
      </div>
      {#if mode === "write"}
        <div class="toggle" title="Commands: Manual/Auto">
          <button class="mode-btn" class:active={commandToggle === "manual"} onclick={() => setCommandToggle("manual")}>Manual</button>
          <button class="mode-btn" class:active={commandToggle === "auto"} onclick={() => setCommandToggle("auto")}>Auto</button>
        </div>
        <div class="toggle" title="Edits: Ask/Auto">
          <button class="mode-btn" class:active={editToggle === "ask"} onclick={() => setEditToggle("ask")}>Ask</button>
          <button class="mode-btn" class:active={editToggle === "auto"} onclick={() => setEditToggle("auto")}>Auto</button>
        </div>
      {/if}
      <ModelSelector chatId={chat.id} providerId={chat.provider_id} modelId={chat.model_id} />
      </Composer>
    </div>

    {#if dragOver}
      <div class="drop-overlay" aria-hidden="true">
        <span class="drop-hint">{m.composer_drop_here()}</span>
      </div>
    {/if}
  </div>

  {#if $rightSidebarOpen}
    {#if project}
      <ContextPanel projectId={project.id} projectName={project.name} chatId={chat.id} />
    {:else}
      <ContextPanel chatId={chat.id} />
    {/if}
  {/if}

  <AgentRunModal />
  <AgentInfoModal chatId={chat.id} />
  <ChatInfoModal open={infoOpen} chatId={chat.id} onclose={() => (infoOpen = false)} />
  <TasksModal open={$tasksModalChatId === chat.id} chatId={chat.id} onclose={closeTasksModal} />

  {#if confirmOpen}
    <div
      class="compact-overlay"
      role="presentation"
      onkeydown={onKeydown}
    >
      <div
        class="compact-dialog"
        role="dialog"
        tabindex="-1"
        onclick={(e) => e.stopPropagation()}
        onkeydown={onKeydown}
      >
        <header class="compact-head">
          <ScrollText size={15} />
          <span>{m.compaction_confirm_title()}</span>
        </header>
        <div class="compact-body">
          <p>{m.compaction_confirm_body()}</p>
          <p class="compact-warn">{m.compaction_confirm_warning()}</p>
        </div>
        <footer class="compact-foot">
          <span class="compact-spacer"></span>
          <button class="compact-cancel" onclick={() => (confirmOpen = false)}>
            {m.common_cancel()}
          </button>
          <button class="compact-run" onclick={() => void runCompact()}>
            {m.compaction_confirm_run()}
          </button>
        </footer>
      </div>
    </div>
  {/if}
</div>

<svelte:window onkeydown={onKeydown} />

<style>
  .chat-view {
    min-width: 0;
    position: relative;
  }
  .composer-area {
    position: relative;
  }
  .drop-overlay {
    position: absolute;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    color: var(--foreground);
  }
  .drop-overlay::before {
    content: "";
    position: absolute;
    inset: 0;
    background: var(--background);
    opacity: 0.7;
  }
  .drop-hint {
    position: relative;
    padding: 1.5rem 2.5rem;
    border: 2px dashed var(--ring);
    border-radius: var(--radius-lg);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.9rem;
    font-weight: 500;
  }
  .chat-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    height: var(--header-height);
    padding: 0 0.75rem;
    border-bottom: 1px solid var(--border);
    background: var(--background);
  }
  .title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.875rem;
    font-weight: 500;
  }
  .title.thinking {
    background: linear-gradient(90deg, hsl(217 91% 60%), hsl(265 80% 65%), hsl(217 91% 60%));
    background-size: 200% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    animation: title-think 2.2s ease-in-out infinite, title-shift 4s linear infinite;
  }
  @keyframes title-think {
    0%,
    100% {
      opacity: 0.75;
    }
    50% {
      opacity: 1;
    }
  }
  @keyframes title-shift {
    to {
      background-position: 200% 0;
    }
  }
  .pbadge {
    width: 10px;
    height: 10px;
    border-radius: 9999px;
    flex-shrink: 0;
  }
  .pname {
    font-size: 0.78rem;
    color: var(--muted-foreground);
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sep {
    color: var(--muted-foreground);
  }
  .ctx-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 0.68rem;
    color: var(--muted-foreground);
    cursor: default;
    margin-left: 0.2rem;
  }
  .export-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 1.75rem;
    min-width: 1.75rem;
    padding: 0 0.3rem;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .export-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .export-btn:disabled {
    opacity: 0.5;
  }
  .export-btn :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .tasks-btn {
    gap: 0.25rem;
    border: 1px solid var(--border);
    background: var(--background);
    border-radius: var(--radius-md);
  }
  .tasks-chip {
    font-size: 0.65rem;
    line-height: 1;
    padding: 0.15rem 0.35rem;
    border-radius: 9999px;
    background: var(--secondary);
    color: var(--secondary-foreground);
  }
  .tasks-chip.open {
    background: hsl(217 91% 60%);
    color: white;
  }
  .tasks-chip.done {
    background: hsl(140 60% 40%);
    color: white;
  }
  .ag-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.75rem;
    height: 1.75rem;
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 0;
    color: var(--muted-foreground);
    cursor: default;
    flex-shrink: 0;
  }
  .ag-btn:hover {
    background: var(--accent);
  }
  .ag-btn.running {
    color: hsl(140 50% 50%);
    border-color: hsl(140 50% 50%);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }
  .modes { display: inline-flex; align-items: center; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden; height: 1.75rem; }
  .mode-btn { display: inline-flex; align-items: center; height: 100%; padding: 0 0.5rem; border: none; background: var(--background); color: var(--muted-foreground); font-size: 0.7rem; text-transform: capitalize; cursor: default; border-right: 1px solid var(--border); }
  .mode-btn:last-child { border-right: none; }
  .mode-btn:hover { background: var(--accent); }
  .mode-btn.active { background: var(--secondary); color: var(--secondary-foreground); }
  .toggle { display: inline-flex; align-items: center; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden; height: 1.75rem; }
  .empty {
    text-align: center;
    color: var(--muted-foreground);
    font-size: 0.875rem;
    padding: 2rem;
  }
  .compaction-banner {
    margin: 0.25rem 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    overflow: hidden;
  }
  .cb-head {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: none;
    background: transparent;
    color: var(--muted-foreground);
    font-size: 0.75rem;
    text-align: left;
    cursor: default;
  }
  .cb-head:hover {
    background: var(--accent);
  }
  .cb-head :global(svg) {
    flex-shrink: 0;
  }
  .cb-title {
    color: var(--foreground);
    font-weight: 500;
  }
  .cb-meta {
    color: var(--muted-foreground);
    font-size: 0.72rem;
    white-space: nowrap;
  }
  .cb-body {
    max-height: 40vh;
    overflow-y: auto;
    padding: 0.4rem 0.6rem;
    border-top: 1px solid var(--border);
    background: var(--muted);
    color: var(--foreground);
    font-size: 0.8rem;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .compact-btn {
    border: 1px solid var(--border);
    background: var(--background);
    border-radius: var(--radius-md);
  }
  .compact-btn.green { color: hsl(140 50% 45%); }
  .compact-btn.orange { color: hsl(38 90% 50%); }
  .compact-btn.red { color: hsl(0 70% 55%); }
  .compact-btn.grey { color: var(--muted-foreground); }
  .compact-result {
    margin: 0.5rem auto;
    max-width: 520px;
    border: 1px solid var(--border);
    border-left: 3px solid hsl(140 50% 45%);
    border-radius: var(--radius-md);
    background: var(--background);
    padding: 0.5rem 0.7rem;
    font-size: 0.78rem;
  }
  .cr-head {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--foreground);
    font-weight: 600;
    margin-bottom: 0.3rem;
  }
  .cr-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem 1.2rem;
  }
  .cr-row {
    display: inline-flex;
    align-items: baseline;
    gap: 0.3rem;
    color: var(--muted-foreground);
  }
  .cr-val {
    color: var(--foreground);
    font-variant-numeric: tabular-nums;
  }
  .compact-overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .compact-dialog {
    width: 440px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
  }
  .compact-head {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
    font-size: 0.875rem;
    font-weight: 600;
  }
  .compact-body {
    padding: 0.875rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    font-size: 0.8125rem;
    color: var(--foreground);
  }
  .compact-warn {
    color: var(--muted-foreground);
    font-size: 0.75rem;
  }
  .compact-foot {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.55rem 0.875rem;
    border-top: 1px solid var(--border);
  }
  .compact-spacer { flex: 1; }
  .compact-cancel,
  .compact-run {
    padding: 0.32rem 0.8rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8rem;
    cursor: default;
  }
  .compact-cancel:hover { background: var(--accent); }
  .compact-run {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .compact-run:hover { filter: brightness(1.08); }
</style>