<script lang="ts">
  import { untrack } from "svelte";
  import {
    AlertCircle,
    Bot,
    Brain,
    Check,
    ChevronRight,
    Copy,
    Loader,
    RefreshCw,
    Wrench,
  } from "@lucide/svelte";
  import { renderMarkdown } from "$lib/markdown";
  import { m } from "$lib/i18n";
  import type { UiMessage } from "$lib/stores/chat";
  import {
    pendingApprovals,
    ptyByBlock,
    turnErrorByMessage,
    regenerateMessage,
    retryTurn,
  } from "$lib/stores/chat";
  import { pendingAsk } from "$lib/stores/project";
  import { attachmentReadDataUrl, type ContentBlock } from "$lib/tauri";

  let { messages, showThinking = false }: { messages: UiMessage[]; showThinking?: boolean } = $props();

  // Intentional snapshot of the prop: the user can toggle openness freely afterwards.
  let processOpen = $state(untrack(() => showThinking));
  let totalElapsedMs = $state(0);
  let cardOpen = $state<Record<string, boolean>>({});
  let copiedId = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

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

  function regenerate() {
    void regenerateMessage(firstMsg.id);
  }

  function retry() {
    void retryTurn(errEntry?.message_id ?? lastMsg.id);
  }

  let firstMsg = $derived(messages[0]);
  let lastMsg = $derived(messages[messages.length - 1]);
  let anyStreaming = $derived(messages.some((m) => m.streaming));
  let allThinking = $derived(messages.flatMap((m) => m.blocks.filter((b) => b.type === "thinking")));
  let allTools = $derived(messages.flatMap((m) => m.blocks.filter((b) => b.type === "tool_use")));
  let answerTextBlocks = $derived(lastMsg.blocks.filter((b) => b.type === "text"));
  let intermediateTextBlocks = $derived(
    messages.slice(0, -1).flatMap((m) => m.blocks.filter((b) => b.type === "text")),
  );
  let hasProcess = $derived(
    allThinking.length > 0 || allTools.length > 0 || intermediateTextBlocks.length > 0,
  );
  let thinkingPhase = $derived(anyStreaming && answerTextBlocks.length === 0);
  let answerText = $derived(answerTextBlocks.map((b) => b.text ?? "").join("\n\n"));
  let thinkingCopyText = $derived(allThinking.map((b) => b.text ?? "").join("\n\n"));

  let aggUsage = $derived.by(() => {
    let prompt = 0,
      completion = 0,
      total = 0;
    for (const m of messages) {
      const u = m.usage;
      if (u) {
        prompt += u.prompt_tokens;
        completion += u.completion_tokens;
        total += u.total_tokens;
      }
    }
    return { prompt, completion, total };
  });
  let model = $derived(lastMsg.model);

  let errEntry = $derived.by(() => {
    for (const m of messages) {
      const e = $turnErrorByMessage[m.id];
      if (e) return e;
    }
    return undefined;
  });
  let hasError = $derived(messages.some((m) => m.finish_reason === "error") || Boolean(errEntry));

  let timeLabel = $derived(fmtMs(totalElapsedMs));

  $effect(() => {
    if (!anyStreaming) return;
    const startedAt = Date.now() - untrack(() => totalElapsedMs);
    const timer = setInterval(() => (totalElapsedMs = Date.now() - startedAt), 100);
    return () => clearInterval(timer);
  });
  $effect(() => () => clearTimeout(copyTimer));
</script>

<div class="msg">
  <div class="msg-body">
    {#if hasProcess || thinkingPhase}
      <div class="proc">
        <button
          type="button"
          class="proc-head"
          class:live={thinkingPhase}
          title={processOpen ? m.message_hide_details() : m.message_show_details()}
          aria-expanded={processOpen}
          onclick={() => (processOpen = !processOpen)}
        >
          <Brain size={13} />
          {#if thinkingPhase}
            <span class="proc-label">{m.message_model_thinking()}</span>
            <span class="proc-time">{timeLabel || "0.0s"}</span>
          {:else}
            <span class="proc-label">
              {timeLabel ? m.message_thought_for({ time: timeLabel }) : m.message_thinking()}
            </span>
            {#if allTools.length > 0}
              <span class="proc-meta">· {m.message_tools_count({ n: allTools.length })}</span>
            {/if}
            {#if aggUsage.total > 0}
              <span class="proc-meta">· {m.message_tokens_total()} {aggUsage.total}</span>
            {/if}
          {/if}
          <span class="chev" class:rot={processOpen}><ChevronRight size={12} /></span>
        </button>

        {#if processOpen}
          <div class="proc-body">
            {#if thinkingCopyText}
              <div class="think-text">{thinkingCopyText}</div>
            {/if}
            {#each intermediateTextBlocks as b (b.id)}
              <div class="prose prose-sm">{@html renderMarkdown(b.text ?? "")}</div>
            {/each}
            {#each allTools as b (b.id)}
              {@render toolEntry(b)}
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    {#each answerTextBlocks as b (b.id)}
      <div class="prose">{@html renderMarkdown(b.text ?? "")}</div>
    {/each}
    {#if anyStreaming && answerTextBlocks.length > 0}
      <span class="cursor"></span>
    {/if}

    {#if hasError}
      {@render errorBanner()}
    {/if}

    <div class="msg-foot">
      {#if model}
        <span class="foot-model">{model}</span>
      {/if}
      {#if aggUsage.prompt > 0}
        <span class="foot-item">{m.message_tokens_prompt()}: {aggUsage.prompt}</span>
      {/if}
      {#if aggUsage.completion > 0}
        <span class="foot-item">{m.message_tokens_completion()}: {aggUsage.completion}</span>
      {/if}
      {#if aggUsage.total > 0}
        <span class="foot-item">{m.message_tokens_total()}: {aggUsage.total}</span>
      {/if}
      {#if timeLabel}
        <span class="foot-item">{m.message_duration()}: {timeLabel}</span>
      {/if}
      {#if !anyStreaming}
        <div class="foot-actions">
          <button type="button" class="copy-btn" title={m.message_regenerate()} onclick={regenerate}>
            <RefreshCw size={12} />
          </button>
          {@render copyBtn("turn:" + firstMsg.id, answerText)}
        </div>
      {/if}
    </div>
  </div>
</div>

{#snippet errorBanner()}
  <div class="turn-error">
    <div class="turn-error-head">
      <AlertCircle size={13} />
      <span class="turn-error-title">{errEntry ? errorKindLabel(errEntry.kind) : m.message_error()}</span>
    </div>
    {#if errEntry}
      <div class="turn-error-msg">{errEntry.message}</div>
    {/if}
    {#if errEntry?.retryable}
      <button class="retry-btn" onclick={retry}>
        <RefreshCw size={12} />
        {m.common_retry()}
      </button>
    {/if}
  </div>
{/snippet}

{#snippet toolEntry(b: ContentBlock)}
  {@const ap = $pendingApprovals[b.id]}
  {@const ask = $pendingAsk[b.id]}
  {@const pty = $ptyByBlock[b.id]}
  {@const agent = isAgentTool(b.name)}
  {@const open = cardOpen[b.id] ?? Boolean(pty)}
  <div class="tool-row">
    <button
      type="button"
      class="tool-row-head"
      title={open ? m.message_hide_details() : m.message_show_details()}
      aria-expanded={open}
      onclick={() => (cardOpen[b.id] = !open)}
    >
      {#if agent}
        <Bot size={12} />
        <span class="tool-row-label">{m.message_agent_call()}</span>
      {:else if isMcpTool(b.name)}
        <Wrench size={12} />
        <span class="tool-row-label">{m.message_mcp_call()}</span>
      {:else}
        <Wrench size={12} />
        <span class="tool-row-label">{m.message_tool_call()}</span>
      {/if}
      <span class="tool-name">{toolName(b)}</span>
      {#if ap}
        <span class="badge">{m.message_waiting_approval()}</span>
      {:else if ask}
        <span class="badge">{m.message_waiting_input()}</span>
      {:else if pty && pty.code === null}
        <Loader size={12} class="spin" />
        <span class="badge">{m.message_waiting_input()}</span>
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
      {#if pty}
        <div class="pty">
          <div class="pty-cmd">$ {pty.command}</div>
          <pre class="pty-out">{pty.output || "(running…)"}</pre>
          {#if pty.code !== null}
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

    <div class="tool-row-foot">
      <span class="tool-name">{toolName(b)}</span>
      <span class="foot-sep">·</span>
      <span class="foot-status" class:st-err={b.is_error || b.status === "error"}>{toolStatusLabel(b)}</span>
      {@render copyBtn("tool:" + b.id, b.result ?? b.input ?? "")}
    </div>
  </div>
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

  .prose :global(.table-wrap) {
    display: block;
    max-width: 100%;
    overflow-x: auto;
    margin: 0.5em 0;
  }
  .prose :global(table) {
    display: table;
    width: 100%;
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
  .prose-sm {
    font-size: 0.8125rem;
    color: var(--muted-foreground);
    margin: 0.25rem 0.25rem 0.15rem;
  }

  .proc {
    margin: 0 0 0.3rem;
    max-width: 100%;
  }
  .proc-head,
  .tool-row-head {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    padding: 0.15rem 0.3rem;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted-foreground);
    text-align: left;
    cursor: default;
  }
  .proc-head { font-size: 0.78rem; }
  .tool-row-head { font-size: 0.75rem; }
  .proc-head:hover,
  .tool-row-head:hover {
    background: color-mix(in srgb, var(--accent) 50%, transparent);
  }
  .proc-head > :global(svg),
  .tool-row-head > :global(svg) {
    flex-shrink: 0;
  }
  .proc-head.live > :global(svg) {
    animation: brain-pulse 1.6s ease-in-out infinite;
  }
  @keyframes brain-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
  .proc-label,
  .tool-row-label {
    flex-shrink: 0;
  }
  .proc-meta {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .proc-time {
    flex-shrink: 0;
    padding: 0 0.4rem;
    border-radius: 9999px;
    background: var(--muted);
    color: var(--foreground);
    font-size: 0.7rem;
    font-variant-numeric: tabular-nums;
  }
  .proc-head .chev {
    margin-left: auto;
  }
  .tool-row-head .tool-name {
    flex: 1 1 auto;
    min-width: 0;
  }
  .proc-body {
    margin-top: 0.2rem;
    max-height: 320px;
    overflow-y: auto;
  }
  .think-text {
    margin: 0.2rem 0.25rem 0.15rem;
    padding: 0.4rem 0.5rem;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--muted) 55%, transparent);
    color: var(--muted-foreground);
    font-style: italic;
    font-size: 0.8125rem;
    white-space: pre-wrap;
    max-height: 320px;
    overflow-y: auto;
  }
  .tool-row {
    margin: 0.25rem 0 0.15rem;
  }
  .tool-row + .tool-row {
    border-top: 1px solid color-mix(in srgb, var(--border) 50%, transparent);
    padding-top: 0.3rem;
  }
  .tool-row-foot {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 0 0.3rem 0.1rem;
    font-size: 0.7rem;
    color: var(--muted-foreground);
    min-width: 0;
  }
  .tool-row-foot .tool-name {
    flex-shrink: 1;
    max-width: 100%;
  }
  .tool-name {
    font-weight: 600;
    color: var(--foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .working { font-style: italic; color: var(--muted-foreground); flex-shrink: 0; }
  .chev {
    display: inline-flex;
    flex-shrink: 0;
    transition: transform 0.15s ease;
  }
  .chev.rot { transform: rotate(90deg); }

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
  .pty { margin: 0.35rem 0.25rem 0; display: flex; flex-direction: column; gap: 0.3rem; }
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
  .pty-exit { font-size: 0.7rem; color: var(--muted-foreground); }
  :global(.spin) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

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