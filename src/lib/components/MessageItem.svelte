<script lang="ts">
  import { untrack } from "svelte";
  import { marked } from "marked";
  import {
    AlertCircle,
    Bot,
    Brain,
    Check,
    ChevronRight,
    Copy,
    File as FileIcon,
    Image as ImageIcon,
    Loader,
    Pencil,
    RefreshCw,
    Send,
    Wrench,
  } from "@lucide/svelte";
  import type { UiMessage } from "$lib/stores/chat";
  import { pendingApprovals, ptyByBlock, attachmentsByChat, turnErrorByMessage, regenerateMessage, retryTurn, editMessage } from "$lib/stores/chat";
  import {
    approveRequest,
    ptyInput,
    attachmentReadDataUrl,
    type ContentBlock,
    type AskUserEvent,
    type ApprovalRequestEvent,
  } from "$lib/tauri";
  import { pendingAsk, resolveAsk } from "$lib/stores/project";
  import { m } from "$lib/i18n";
  import { formatSize } from "$lib/utils";

  let { message, showThinking = false }: { message: UiMessage; showThinking?: boolean } = $props();

  let editing = $state(false);
  let editText = $state("");
  let askText = $state("");
  let askSelected: Record<string, Set<number>> = $state({});

  // Intentional snapshot of the prop: the user can toggle openness freely afterwards.
  let thinkingOpen = $state(untrack(() => showThinking));
  let thinkingElapsedMs = $state(0);
  let cardOpen = $state<Record<string, boolean>>({});
  let copiedId = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  let resolving = $state<Record<string, boolean>>({});

  async function handleApprove(reqId: string, approved: boolean) {
    if (resolving[reqId] !== undefined) return;
    resolving = { ...resolving, [reqId]: approved };
    try {
      await approveRequest(reqId, approved);
    } catch (e) {
      console.error("approveRequest failed", e);
      const next = { ...resolving };
      delete next[reqId];
      resolving = next;
    }
  }

  marked.setOptions({ breaks: true, gfm: true });

  function render(text: string): string {
    if (!text) return "";
    try {
      return marked.parse(text) as string;
    } catch {
      return text;
    }
  }

  function prettyJson(s: string): string {
    if (!s) return "";
    try {
      return JSON.stringify(JSON.parse(s), null, 2);
    } catch {
      return s;
    }
  }

  function isAgentTool(name: string | undefined): boolean {
    return name === "agent__run_batch" || (name?.startsWith("agent__") ?? false);
  }

  function errorKindLabel(kind: string): string {
    switch (kind) {
      case "auth": return m.error_kind_auth();
      case "rate_limit": return m.error_kind_rate_limit();
      case "insufficient_quota": return m.error_kind_insufficient_quota();
      case "context_length": return m.error_kind_context_length();
      case "content_filter": return m.error_kind_content_filter();
      case "network": return m.error_kind_network();
      case "server": return m.error_kind_server();
      case "parse": return m.error_kind_parse();
      default: return m.error_kind_status();
    }
  }

  function isMcpTool(name: string | undefined): boolean {
    return name?.startsWith("mcp__") ?? false;
  }

  function agentToolLabel(name: string): string {
    if (name === "agent__run_batch") return m.agents_batch();
    const id = name.replace(/^agent__/, "").replace(/__run$/, "");
    return id || name;
  }

  function toolName(b: ContentBlock): string {
    if (isAgentTool(b.name)) return agentToolLabel(b.name ?? "");
    if (isMcpTool(b.name)) return (b.name ?? "").replace(/^mcp__/, "");
    return b.name ?? "";
  }

  function toolStatusLabel(b: ContentBlock): string {
    if (b.is_error || b.status === "error") return m.message_error();
    if (b.status === "running") return m.message_running();
    return m.message_done();
  }

  function fmtMs(ms?: number | null): string {
    if (!ms || ms <= 0) return "";
    if (ms < 1000) return `${Math.round(ms)}ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
    const mins = Math.floor(ms / 60000);
    const secs = Math.round((ms % 60000) / 1000);
    return `${mins}m ${secs}s`;
  }

  function copyText(id: string, text: string) {
    if (!text) return;
    navigator.clipboard
      .writeText(text)
      .then(() => {
        copiedId = id;
        clearTimeout(copyTimer);
        copyTimer = setTimeout(() => (copiedId = null), 2000);
      })
      .catch(() => {});
  }

  function startEdit() {
    editText = message.content;
    editing = true;
  }
  function saveEdit() {
    const t = editText.trim();
    if (t) void editMessage(message.id, t);
    editing = false;
  }
  function cancelEdit() {
    editing = false;
  }
  function sendAsk(blockId: string) {
    const t = askText.trim();
    if (!t) return;
    void resolveAsk(blockId, t);
    askText = "";
  }

  function pickOption(blockId: string, ask: AskUserEvent, idx: number) {
    if (ask.multi_select) {
      const set = new Set(askSelected[blockId] ?? []);
      if (set.has(idx)) set.delete(idx);
      else set.add(idx);
      askSelected = { ...askSelected, [blockId]: set };
    } else {
      void resolveAsk(blockId, ask.options[idx]);
    }
  }

  function confirmMulti(blockId: string, ask: AskUserEvent) {
    const set = askSelected[blockId];
    const chosen = ask.options
      .map((opt, i) => (set?.has(i) ? opt : null))
      .filter((x): x is string => x !== null);
    if (chosen.length === 0) return;
    void resolveAsk(blockId, chosen.join(", "));
    askSelected = { ...askSelected, [blockId]: new Set() };
  }

  let isUser = $derived(message.role === "user");
  let userAttachments = $derived.by(() => {
    if (!isUser) return [];
    for (const list of Object.values($attachmentsByChat)) {
      const found = list.filter((a) => a.message_id === message.id);
      if (found.length) return found;
    }
    return [];
  });
  let imgCache = $state<Record<string, string>>({});

  $effect(() => {
    for (const a of userAttachments) {
      if (!a.is_image || a.id in imgCache) continue;
      const id = a.id;
      void attachmentReadDataUrl(id)
        .then((url) => (imgCache = { ...imgCache, [id]: url }))
        .catch(() => (imgCache = { ...imgCache, [id]: "" }));
    }
  });

  let textBlocks = $derived(message.blocks.filter((b) => b.type === "text"));
  let thinkingBlocks = $derived(message.blocks.filter((b) => b.type === "thinking"));
  let toolBlocks = $derived(message.blocks.filter((b) => b.type === "tool_use"));
  let streaming = $derived(message.streaming);
  let thinkingActive = $derived(
    streaming &&
      thinkingBlocks.some((b) => (b.text ?? "").length > 0) &&
      textBlocks.length === 0 &&
      toolBlocks.length === 0,
  );

  let usage = $derived(message.usage);
  let promptRate = $derived.by(() => {
    const ttft = usage?.time_to_first_token_ms ?? 0;
    if (!usage || usage.prompt_tokens <= 0 || ttft <= 0) return 0;
    return usage.prompt_tokens / (ttft / 1000);
  });
  let evalRate = $derived.by(() => {
    const gen = usage?.generation_duration_ms ?? 0;
    if (!usage || usage.completion_tokens <= 0 || gen <= 0) return 0;
    return usage.completion_tokens / (gen / 1000);
  });

  let mainCopyText = $derived(textBlocks.map((b) => b.text ?? "").join("\n\n"));
  let thinkingCopyText = $derived(thinkingBlocks.map((b) => b.text ?? "").join("\n\n"));
  let thinkingTimeLabel = $derived.by(() => {
    if (thinkingActive) return fmtMs(thinkingElapsedMs) || "0.0s";
    return fmtMs(message.thinking_ms) || fmtMs(thinkingElapsedMs);
  });

  $effect(() => {
    if (!thinkingActive) return;
    const startedAt = Date.now() - untrack(() => thinkingElapsedMs);
    const timer = setInterval(() => (thinkingElapsedMs = Date.now() - startedAt), 100);
    return () => clearInterval(timer);
  });
  $effect(() => () => clearTimeout(copyTimer));
</script>

<div class="msg" class:user={isUser}>
  {#if isUser && editing}
    <div class="edit-box">
      <textarea bind:value={editText} rows="3"></textarea>
      <div class="edit-actions">
        <button class="apr no" onclick={cancelEdit}>{m.message_edit_cancel()}</button>
        <button class="apr yes" onclick={saveEdit}>{m.message_edit_save()}</button>
      </div>
    </div>
  {:else}
    <div class="msg-body" class:user={isUser}>
      {#if isUser && !streaming}
        <button class="msg-action" title={m.message_edit()} onclick={startEdit}>
          <Pencil size={12} />
        </button>
      {/if}

      {#if isUser && userAttachments.length > 0}
        <div class="attachments">
          {#each userAttachments as a (a.id)}
            {#if a.is_image && imgCache[a.id]}
              <img class="att-image" src={imgCache[a.id]} alt={a.file_name} title={a.file_name} />
            {:else}
              <span
                class="att-chip"
                title={a.file_name}
                aria-label="{(a.is_image ? m.attachment_image() : m.attachment_file()) + ": " + a.file_name}"
              >
                {#if a.is_image}
                  <ImageIcon size={12} />
                {:else}
                  <FileIcon size={12} />
                {/if}
                <span class="att-chip-name">{a.file_name}</span>
                {#if !a.is_image}
                  <span class="att-chip-size">{formatSize(a.file_size)}</span>
                {/if}
              </span>
            {/if}
          {/each}
        </div>
      {/if}

      {#if !isUser && thinkingBlocks.length > 0}
        <div class="think-card">
          <button
            type="button"
            class="card-head"
            title={thinkingOpen ? m.message_hide_details() : m.message_show_details()}
            aria-expanded={thinkingOpen}
            onclick={() => (thinkingOpen = !thinkingOpen)}
          >
            <Brain size={13} />
            <span class="head-label">{thinkingActive ? m.message_model_thinking() : m.message_thinking()}</span>
            {#if thinkingTimeLabel}
              <span class="head-time">{thinkingTimeLabel}</span>
            {/if}
            <span class="chev" class:rot={thinkingOpen}><ChevronRight size={12} /></span>
          </button>
          {#if thinkingOpen}
            <div class="think-text">{thinkingCopyText}</div>
          {/if}
          {#if thinkingTimeLabel}
            <div class="card-foot">
              <span class="foot-time">{thinkingTimeLabel}</span>
            </div>
          {/if}
        </div>
      {/if}

      {#if !isUser}
        {#each toolBlocks as b (b.id)}
          {@render toolCard(b)}
        {/each}
      {/if}

      {#each textBlocks as b (b.id)}
        <div class="prose">{@html render(b.text ?? "")}</div>
      {/each}

      {#if streaming && textBlocks.length > 0}
        <span class="cursor"></span>
      {/if}
      {#if !isUser && message.finish_reason === "error"}
        {@render errorBanner()}
      {/if}

      {#if !isUser}
        <div class="msg-foot">
          {#if message.model}
            <span class="foot-model">{message.model}</span>
          {/if}
          {#if usage && usage.prompt_tokens > 0}
            <span class="foot-item">{m.message_tokens_prompt()}: {usage.prompt_tokens}</span>
          {/if}
          {#if usage && usage.completion_tokens > 0}
            <span class="foot-item">{m.message_tokens_completion()}: {usage.completion_tokens}</span>
          {/if}
          {#if usage && usage.total_tokens > 0}
            <span class="foot-item">{m.message_tokens_total()}: {usage.total_tokens}</span>
          {/if}
          {#if (usage?.total_duration_ms ?? 0) > 0}
            <span class="foot-item">{m.message_duration()}: {fmtMs(usage!.total_duration_ms)}</span>
          {/if}
          {#if promptRate > 0}
            <span class="foot-item">{promptRate.toFixed(1)} {m.message_tokens_rate()}</span>
          {/if}
          {#if evalRate > 0}
            <span class="foot-item">{evalRate.toFixed(1)} {m.message_tokens_rate()}</span>
          {/if}
          {#if !streaming}
            <div class="foot-actions">
              <button
                type="button"
                class="copy-btn"
                title={m.message_regenerate()}
                onclick={() => regenerateMessage(message.id)}
              >
                <RefreshCw size={12} />
              </button>
              {@render copyBtn("msg:" + message.id, mainCopyText)}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

{#snippet errorBanner()}
  {@const err = $turnErrorByMessage[message.id]}
  <div class="turn-error">
    <div class="turn-error-head">
      <AlertCircle size={13} />
      <span class="turn-error-title">{err ? errorKindLabel(err.kind) : m.message_error()}</span>
    </div>
    {#if err}
      <div class="turn-error-msg">{err.message}</div>
    {/if}
    {#if err?.retryable}
      <button class="retry-btn" onclick={() => retryTurn(message.id)}>
        <RefreshCw size={12} />
        {m.common_retry()}
      </button>
    {/if}
  </div>
{/snippet}

{#snippet aprActions(ap: ApprovalRequestEvent)}
  {#if resolving[ap.request_id] !== undefined}
    <div class="approval-actions">
      <span class="apr resolving">
        <Loader size={13} class="spin" />
        {resolving[ap.request_id] ? "Approving…" : "Rejecting…"}
      </span>
    </div>
  {:else}
    <div class="approval-actions">
      <button class="apr yes" onclick={() => handleApprove(ap.request_id, true)}>Approve</button>
      <button class="apr no" onclick={() => handleApprove(ap.request_id, false)}>Reject</button>
    </div>
  {/if}
{/snippet}

{#snippet toolCard(b: ContentBlock)}
  {@const ap = $pendingApprovals[b.id]}
  {@const ask = $pendingAsk[b.id]}
  {@const pty = $ptyByBlock[b.id]}
  {@const agent = isAgentTool(b.name)}
  {@const open = cardOpen[b.id] ?? Boolean(ap || ask || pty)}
  {#if agent}
    <div class="tool-card" class:err={b.is_error}>
      <button
        type="button"
        class="card-head"
        title={open ? m.message_hide_details() : m.message_show_details()}
        aria-expanded={open}
        onclick={() => (cardOpen[b.id] = !open)}
      >
        <Bot size={13} />
        <span class="head-label">{m.message_agent_call()}</span>
        <span class="tool-name">{toolName(b)}</span>
        {#if ap}
          <span class="badge">needs approval</span>
        {:else if b.status === "running"}
          <Loader size={12} class="spin" />
          <span class="working">{m.message_running()}</span>
        {:else if b.is_error}
          <AlertCircle size={12} />
        {:else if b.status === "done"}
          <Check size={12} />
        {/if}
        <span class="chev" class:rot={open}><ChevronRight size={12} /></span>
      </button>

      {#if open}
        {#if ap}
          <div class="approval">
            <div class="approval-summary">{ap.summary}</div>
            {#if ap.path}
              <div class="approval-path" title={ap.path}>{ap.path}</div>
            {/if}
            {#if ap.escape}
              <div class="approval-escape">{m.approval_escape_warning()}</div>
              {#if ap.symlink_target}
                <div class="approval-path" title={ap.symlink_target}>→ {ap.symlink_target}</div>
              {/if}
            {/if}
            {#if ap.destructive_mode}
              <div class="approval-mode">{ap.destructive_mode === "trash" ? m.approval_trash() : m.approval_permanent()}</div>
            {/if}
            {#if ap.preview}
              <pre class="tool-input">{ap.preview}</pre>
            {/if}
            {@render aprActions(ap)}
          </div>
        {:else if b.result}
          <pre class="tool-result" class:err={b.is_error}>{b.result}</pre>
        {/if}
      {/if}

      <div class="card-foot">
        <span class="tool-name">{toolName(b)}</span>
        <span class="foot-sep">·</span>
        <span class="foot-status" class:st-err={b.is_error || b.status === "error"}>{toolStatusLabel(b)}</span>
        {@render copyBtn("tool:" + b.id, b.result ?? b.input ?? "")}
      </div>
    </div>
  {:else}
    <div class="tool-card" class:err={b.is_error}>
      <button
        type="button"
        class="card-head"
        title={open ? m.message_hide_details() : m.message_show_details()}
        aria-expanded={open}
        onclick={() => (cardOpen[b.id] = !open)}
      >
        {#if isMcpTool(b.name)}
          <Wrench size={13} />
          <span class="head-label">{m.message_mcp_call()}</span>
        {:else}
          <Wrench size={13} />
          <span class="head-label">{m.message_tool_call()}</span>
        {/if}
        <span class="tool-name">{toolName(b)}</span>
        {#if ap}
          <span class="badge">needs approval</span>
        {:else if b.status === "running"}
          <Loader size={12} class="spin" />
          <span class="working">{m.message_running()}</span>
        {:else if b.is_error}
          <AlertCircle size={12} />
        {:else if b.status === "done"}
          <Check size={12} />
        {/if}
        <span class="chev" class:rot={open}><ChevronRight size={12} /></span>
      </button>

      {#if open}
        {#if ap}
          <div class="approval">
            <div class="approval-summary">{ap.summary}</div>
            {#if ap.path}
              <div class="approval-path" title={ap.path}>{ap.path}</div>
            {/if}
            {#if ap.escape}
              <div class="approval-escape">{m.approval_escape_warning()}</div>
              {#if ap.symlink_target}
                <div class="approval-path" title={ap.symlink_target}>→ {ap.symlink_target}</div>
              {/if}
            {/if}
            {#if ap.destructive_mode}
              <div class="approval-mode">{ap.destructive_mode === "trash" ? m.approval_trash() : m.approval_permanent()}</div>
            {/if}
            {#if ap.preview}
              <pre class="tool-input">{ap.preview}</pre>
            {/if}
            {@render aprActions(ap)}
          </div>
        {:else if ask}
          <div class="ask">
            <div class="ask-q">{ask.question}</div>
            {#if ask.context}
              <div class="ask-ctx">{ask.context}</div>
            {/if}
            {#if ask.options && ask.options.length > 0}
              <div class="ask-options">
                {#each ask.options as opt, i}
                  <button
                    type="button"
                    class="ask-opt"
                    class:sel={ask.multi_select && (askSelected[b.id]?.has(i) ?? false)}
                    onclick={() => pickOption(b.id, ask, i)}
                  >
                    {opt}
                  </button>
                {/each}
              </div>
            {/if}
            {#if ask.multi_select && ask.options && ask.options.length > 0}
              <div class="ask-confirm">
                <button class="apr yes" onclick={() => confirmMulti(b.id, ask)}>
                  {m.ask_user_confirm()}
                </button>
              </div>
            {/if}
            <div class="ask-input">
              <input
                type="text"
                bind:value={askText}
                placeholder={m.ask_user_placeholder()}
                onkeydown={(e) => {
                  if (e.key === "Enter") sendAsk(b.id);
                }}
              />
              <button class="apr yes" onclick={() => sendAsk(b.id)}>
                <Send size={12} /> {m.ask_user_send()}
              </button>
            </div>
          </div>
        {:else if pty}
          <div class="pty">
            <div class="pty-cmd">$ {pty.command}</div>
            <pre class="pty-out">{pty.output || "(running…)"}</pre>
            {#if pty.code === null}
              <div class="pty-in">
                <input
                  type="text"
                  placeholder="type input + Enter (e.g. sudo password)…"
                  onkeydown={(e) => {
                    if (e.key === "Enter") {
                      const t = (e.target as HTMLInputElement).value;
                      ptyInput(pty.session_id, t + "\n");
                      (e.target as HTMLInputElement).value = "";
                    }
                  }}
                />
              </div>
            {:else}
              <div class="pty-exit">exit {pty.code}</div>
            {/if}
          </div>
        {:else}
          {#if b.input}
            <pre class="tool-input">{prettyJson(b.input)}</pre>
          {/if}
          {#if b.result}
            <pre class="tool-result" class:err={b.is_error}>{b.result}</pre>
          {/if}
        {/if}
      {/if}

      <div class="card-foot">
        <span class="tool-name">{toolName(b)}</span>
        <span class="foot-sep">·</span>
        <span class="foot-status" class:st-err={b.is_error || b.status === "error"}>{toolStatusLabel(b)}</span>
        {@render copyBtn("tool:" + b.id, b.result ?? b.input ?? "")}
      </div>
    </div>
  {/if}
{/snippet}

{#snippet copyBtn(id: string, text: string)}
  <button
    type="button"
    class="copy-btn"
    class:copied={copiedId === id}
    title={copiedId === id ? m.message_copied() : m.message_copy()}
    onclick={() => copyText(id, text)}
  >
    <Copy size={12} />
    {#if copiedId === id}
      <span class="copy-ok">{m.message_copied()}</span>
    {/if}
  </button>
{/snippet}

<style>
  .msg {
    display: flex;
    margin: 0.5rem 0;
    position: relative;
  }
  .msg.user {
    justify-content: flex-end;
  }
  .msg-body {
    flex: 1 1 auto;
    min-width: 0;
    padding: 0.625rem 0.875rem;
    border-radius: var(--radius-lg);
    background: var(--secondary);
    color: var(--secondary-foreground);
    font-size: 0.875rem;
    line-height: 1.5;
    word-break: break-word;
    overflow-wrap: break-word;
  }
  .msg-body.user {
    flex: 0 1 auto;
    max-width: 78%;
    background: var(--primary);
    color: var(--primary-foreground);
  }
  .attachments {
    display: flex;
    flex-wrap: wrap;
    gap: 0.375rem;
    margin-bottom: 0.375rem;
  }
  .att-image {
    display: block;
    max-width: 160px;
    max-height: 120px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
  }
  .att-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    max-width: 240px;
    padding: 0.2rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.75rem;
    line-height: 1.4;
  }
  .att-chip :global(svg) {
    flex-shrink: 0;
  }
  .att-chip-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .att-chip-size {
    color: var(--muted-foreground);
    font-size: 0.7rem;
    white-space: nowrap;
  }
  .prose :global(p) { margin: 0.4em 0; }
  .prose :global(p:first-child) { margin-top: 0; }
  .prose :global(p:last-child) { margin-bottom: 0; }
  .prose :global(pre) {
    margin: 0.5em 0;
    padding: 0.625rem 0.75rem;
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: 0.8125rem;
  }
  .prose :global(code) { font-family: var(--font-mono); font-size: 0.8125rem; }
  .prose :global(:not(pre) > code) { padding: 0.1em 0.3em; background: var(--muted); border-radius: 4px; }
  .prose :global(ul), .prose :global(ol) { margin: 0.4em 0; padding-left: 1.25em; }
  .prose :global(ul) { list-style: disc; }
  .prose :global(ol) { list-style: decimal; }
  .prose :global(ul ul) { list-style: circle; }
  .prose :global(li) { display: list-item; margin: 0.15em 0; }
  .prose :global(li > p) { margin: 0; }

  .prose :global(h1) { margin: 0.7em 0 0.3em; font-size: 1.25em; font-weight: 700; }
  .prose :global(h2) { margin: 0.65em 0 0.3em; font-size: 1.12em; font-weight: 700; }
  .prose :global(h3) { margin: 0.6em 0 0.3em; font-size: 1em; font-weight: 600; }
  .prose :global(h4), .prose :global(h5), .prose :global(h6) { margin: 0.6em 0 0.25em; font-size: 0.95em; font-weight: 600; }
  .prose :global(:is(h1, h2, h3, h4, h5, h6):first-child) { margin-top: 0; }

  .prose :global(table) {
    display: block;
    max-width: 100%;
    overflow-x: auto;
    margin: 0.5em 0;
    border-collapse: collapse;
    font-size: 0.85em;
  }
  .prose :global(th),
  .prose :global(td) {
    border: 1px solid var(--border);
    padding: 0.3em 0.5em;
    text-align: left;
    vertical-align: top;
  }
  .prose :global(th) { background: var(--muted); font-weight: 600; }
  .prose :global(tbody tr:nth-child(even)) { background: color-mix(in srgb, var(--muted) 50%, transparent); }

  .prose :global(a) { color: hsl(210 90% 50%); text-decoration: none; }
  .prose :global(a:hover) { text-decoration: underline; }
  .prose :global(a:visited) { color: hsl(210 90% 50%); }

  .prose :global(blockquote) {
    margin: 0.4em 0;
    padding-left: 0.75em;
    border-left: 3px solid var(--border);
    color: var(--muted-foreground);
    font-style: italic;
  }
  .prose :global(blockquote blockquote) {
    border-left-width: 4px;
    border-left-color: var(--muted-foreground);
  }

  .prose :global(hr) { border: none; border-top: 1px solid var(--border); margin: 0.6em 0; }
  .prose :global(img) { max-width: 100%; }
  .prose :global(em) { font-style: italic; }
  .prose :global(strong) { font-weight: 600; }

  .think-card,
  .tool-card {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    padding: 0.35rem 0.5rem;
    max-width: 100%;
  }
  .think-card { margin: 0.05rem 0 0.4rem; }
  .tool-card { margin: 0.4rem 0; }
  .tool-card.err { border-color: var(--destructive); }
  .card-head {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    font-size: 0.75rem;
    color: var(--muted-foreground);
    text-align: left;
    cursor: default;
  }
  .card-head :global(svg) { flex-shrink: 0; }
  .head-label { flex-shrink: 0; }
  .tool-name {
    font-weight: 600;
    color: var(--foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .card-head .tool-name { flex: 1 1 auto; min-width: 0; }
  .card-foot .tool-name { flex-shrink: 1; max-width: 100%; }
  .head-time {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }
  .working { font-style: italic; color: var(--muted-foreground); flex-shrink: 0; }
  .chev {
    display: inline-flex;
    flex-shrink: 0;
    transition: transform 0.15s ease;
  }
  .chev.rot { transform: rotate(90deg); }
  .think-text {
    margin-top: 0.35rem;
    padding: 0.4rem 0.5rem;
    border-radius: var(--radius-sm);
    background: var(--muted);
    color: var(--muted-foreground);
    font-style: italic;
    font-size: 0.8125rem;
    white-space: pre-wrap;
    max-height: 320px;
    overflow-y: auto;
  }

  .card-foot,
  .msg-foot {
    margin-top: 0.4rem;
    padding-top: 0.35rem;
    border-top: 1px solid var(--border);
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
    font-size: 0.7rem;
    color: var(--muted-foreground);
    min-width: 0;
  }
  .foot-model { white-space: nowrap; }
  .foot-item { white-space: nowrap; }
  .foot-time { font-variant-numeric: tabular-nums; }
  .foot-sep { flex-shrink: 0; }
  .foot-status { white-space: nowrap; }
  .foot-status.st-err { color: var(--destructive); }
  .foot-actions {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    flex-shrink: 0;
  }
  .foot-actions .copy-btn {
    margin-left: 0;
  }
  .copy-btn {
    margin-left: auto;
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.1rem 0.3rem;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted-foreground);
    font-size: 0.7rem;
    cursor: default;
  }
  .copy-btn:hover { background: var(--accent); color: var(--foreground); }
  .copy-btn.copied,
  .copy-btn.copied:hover {
    color: hsl(140 60% 40%);
    background: transparent;
  }
  .copy-ok { font-weight: 500; }
  .badge {
    font-size: 0.62rem;
    color: hsl(38 92% 40%);
    border: 1px solid hsl(38 92% 60%);
    border-radius: 9999px;
    padding: 0 0.4rem;
    flex-shrink: 0;
  }

  .tool-input, .tool-result {
    margin: 0.35rem 0 0;
    padding: 0.4rem 0.5rem;
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 0.75rem;
    white-space: pre-wrap;
    max-height: 200px;
    overflow-y: auto;
  }
  .tool-input { background: var(--muted); }
  .tool-result { background: var(--muted); }
  .tool-result.err { color: var(--destructive); }
  .approval { margin-top: 0.35rem; display: flex; flex-direction: column; gap: 0.3rem; }
  .approval-summary { font-size: 0.75rem; color: var(--foreground); }
  .approval-path { font-family: var(--font-mono); font-size: 0.7rem; color: var(--muted-foreground); word-break: break-all; }
  .approval-escape { font-size: 0.72rem; color: var(--destructive); font-weight: 600; }
  .approval-mode { font-size: 0.68rem; text-transform: uppercase; letter-spacing: 0.03em; color: var(--muted-foreground); }
  .approval-actions { display: flex; gap: 0.4rem; }
  .apr {
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 0.75rem;
    cursor: default;
  }
  .apr.yes { background: hsl(140 60% 40%); color: white; border-color: hsl(140 60% 40%); }
  .apr.no { background: var(--destructive); color: var(--destructive-foreground); border-color: var(--destructive); }
  .apr.resolving {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    color: var(--muted-foreground);
    background: var(--muted);
    border-color: var(--border);
  }
  .apr:disabled {
    opacity: 0.55;
  }
  .pty { margin-top: 0.35rem; display: flex; flex-direction: column; gap: 0.3rem; }
  .pty-cmd { font-family: var(--font-mono); font-size: 0.7rem; color: var(--muted-foreground); }
  .pty-out {
    margin: 0;
    padding: 0.4rem;
    background: hsl(0 0% 8%);
    color: hsl(0 0% 92%);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    white-space: pre-wrap;
    max-height: 240px;
    overflow-y: auto;
  }
  .pty-in input {
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-family: var(--font-mono);
    font-size: 0.75rem;
  }
  .pty-exit { font-size: 0.7rem; color: var(--muted-foreground); }
  :global(.spin) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .msg-action {
    position: absolute;
    top: 0.3rem;
    right: 0.3rem;
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    opacity: 0;
    border-radius: var(--radius-sm);
    padding: 0.15rem;
    cursor: default;
    z-index: 1;
  }
  .msg:hover .msg-action { opacity: 0.7; }
  .msg-action:hover { opacity: 1; background: var(--accent); }
  .msg.user .msg-action { color: var(--primary-foreground); }
  .edit-box {
    max-width: 78%;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .edit-box textarea {
    width: 100%;
    padding: 0.5rem 0.625rem;
    border: 1px solid var(--ring);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-sans);
    outline: none;
    resize: vertical;
  }
  .edit-actions { display: flex; gap: 0.4rem; justify-content: flex-end; }
  .ask { margin-top: 0.35rem; display: flex; flex-direction: column; gap: 0.3rem; }
  .ask-q { font-size: 0.78rem; color: var(--foreground); }
  .ask-input { display: flex; gap: 0.3rem; }
  .ask-input input {
    flex: 1;
    min-width: 0;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.75rem;
    outline: none;
  }
  .ask-ctx { font-size: 0.72rem; color: var(--muted-foreground); }
  .ask-options { display: flex; flex-wrap: wrap; gap: 0.3rem; }
  .ask-opt {
    padding: 0.2rem 0.5rem;
    font-size: 0.75rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--foreground);
    cursor: pointer;
    transition: background 0.12s, border-color 0.12s;
  }
  .ask-opt:hover { border-color: var(--accent); }
  .ask-opt.sel {
    background: var(--primary);
    color: var(--primary-foreground);
    border-color: var(--primary);
  }
  .ask-confirm { display: flex; justify-content: flex-end; }

  .cursor {
    display: inline-block;
    width: 7px;
    height: 1em;
    margin-left: 2px;
    vertical-align: text-bottom;
    background: currentcolor;
    animation: blink 1s step-end infinite;
  }
  @keyframes blink { 50% { opacity: 0; } }
  .err { margin-top: 0.4em; color: var(--destructive); font-size: 0.8125rem; }
  .turn-error {
    margin-top: 0.4rem;
    padding: 0.5rem 0.6rem;
    border: 1px solid var(--destructive);
    border-radius: 6px;
    background: color-mix(in srgb, var(--destructive) 10%, transparent);
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .turn-error-head { display: flex; align-items: center; gap: 0.35rem; color: var(--destructive); font-size: 0.8125rem; font-weight: 600; }
  .turn-error-msg { font-size: 0.75rem; color: var(--foreground); opacity: 0.9; word-break: break-word; white-space: pre-wrap; }
  .retry-btn {
    align-self: flex-start;
    display: inline-flex; align-items: center; gap: 0.3rem;
    font-size: 0.75rem; padding: 0.25rem 0.55rem;
    border: 1px solid var(--border); border-radius: 5px;
    background: var(--muted); color: var(--foreground); cursor: pointer;
  }
  .retry-btn:hover { background: var(--accent); }
</style>