<script lang="ts">
  import { m } from "$lib/i18n";
  import {
    approveRequest,
    type ApprovalRequestEvent,
    type AskUserEvent,
    type ChangeInfo,
  } from "$lib/tauri";
  import {
    pendingApprovals,
    pendingByChat,
    currentChatId,
    approvePending,
    rejectPending,
  } from "$lib/stores/chat";
  import { pendingAsk, resolveAsk } from "$lib/stores/project";
  import {
    ShieldAlert,
    CircleHelp,
    FileDiff,
    FilePlus,
    FileEdit,
    FileX,
    ChevronRight,
    Check,
    X,
    Loader,
    Send,
  } from "@lucide/svelte";

  let { chatId }: { chatId: string } = $props();

  type Interaction =
    | { kind: "approval"; key: string; data: ApprovalRequestEvent }
    | { kind: "ask"; key: string; data: AskUserEvent }
    | { kind: "pending"; key: string; data: ChangeInfo[] };

  let resolving = $state<Record<string, boolean>>({});
  let busy = $state<"approve" | "reject" | null>(null);
  let askText = $state("");
  let askSelected = $state<Set<number>>(new Set());
  let expanded = $state<Record<string, boolean>>({});
  let askInput = $state<HTMLInputElement | null>(null);

  let list = $derived.by(() => {
    const approvals: Interaction[] = Object.entries($pendingApprovals)
      .filter(([, a]) => a.chat_id === chatId)
      .map(([key, data]) => ({ kind: "approval", key, data }));
    const asks: Interaction[] = Object.entries($pendingAsk)
      .filter(([, a]) => a.chat_id === chatId)
      .map(([key, data]) => ({ kind: "ask", key, data }));
    const changes = $pendingByChat[chatId];
    const pending: Interaction[] =
      changes && changes.length > 0
        ? [{ kind: "pending", key: "pending:" + chatId, data: changes }]
        : [];
    return [...approvals, ...asks, ...pending];
  });

  let active = $derived(list[0]);
  let activeKey = $derived(active?.key ?? null);

  $effect(() => {
    if (activeKey === null) return;
    askText = "";
    askSelected = new Set();
    expanded = {};
  });

  async function resolveApproval(reqId: string, approved: boolean) {
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

  async function handlePending(action: "approve" | "reject") {
    if (busy) return;
    busy = action;
    try {
      if (action === "approve") await approvePending(chatId);
      else await rejectPending(chatId);
    } catch (e) {
      console.error(e);
    }
    busy = null;
  }

  function pickOption(ask: AskUserEvent, idx: number) {
    if (ask.multi_select) {
      const set = new Set(askSelected);
      if (set.has(idx)) set.delete(idx);
      else set.add(idx);
      askSelected = set;
    } else {
      void resolveAsk(ask.block_id, ask.options[idx]);
    }
  }

  function confirmMulti(ask: AskUserEvent) {
    const chosen = ask.options
      .map((opt, i) => (askSelected.has(i) ? opt : null))
      .filter((x): x is string => x !== null);
    if (chosen.length === 0) return;
    void resolveAsk(ask.block_id, chosen.join(", "));
  }

  function pickCurrentOption(idx: number) {
    const a = active;
    if (!a || a.kind !== "ask") return;
    pickOption(a.data, idx);
  }

  function confirmCurrentMulti() {
    const a = active;
    if (!a || a.kind !== "ask") return;
    confirmMulti(a.data);
  }

  function submitCurrentAsk() {
    const a = active;
    if (!a || a.kind !== "ask") return;
    const t = askText.trim();
    if (!t) return;
    void resolveAsk(a.data.block_id, t);
    askText = "";
  }

  function skipActive() {
    const a = active;
    if (!a) return;
    if (a.kind === "approval") void resolveApproval(a.data.request_id, false);
    else if (a.kind === "ask") void resolveAsk(a.data.block_id, "");
    else if (a.kind === "pending") void handlePending("reject");
  }

  function primaryAction() {
    const a = active;
    if (!a) return;
    if (a.kind === "approval") void resolveApproval(a.data.request_id, true);
    else if (a.kind === "pending") void handlePending("approve");
    else if (a.kind === "ask") {
      if (askText.trim()) submitCurrentAsk();
      else askInput?.focus();
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (!active) return;
    if ($currentChatId !== chatId) return;
    if (e.key === "Escape") {
      // Capture phase: stop the key from reaching the composer textarea so it
      // doesn't send/cancel a message while a permission dialog is open.
      e.stopPropagation();
      e.preventDefault();
      skipActive();
      return;
    }
    if (e.key === "Enter") {
      if (e.defaultPrevented) return;
      const el = document.activeElement;
      if (
        (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) &&
        el.closest(".interaction-overlay")
      ) {
        return;
      }
      e.stopPropagation();
      e.preventDefault();
      primaryAction();
    }
  }

  // Capture-phase listener so Enter/Esc are intercepted before they reach the
  // composer (or anything else) while an interaction is pending.
  $effect(() => {
    const handler = (e: KeyboardEvent) => onKeydown(e);
    window.addEventListener("keydown", handler, { capture: true });
    return () => window.removeEventListener("keydown", handler, { capture: true });
  });
</script>

{#if active}
  <div class="interaction-overlay" role="dialog">
    <div class="head">
      {#if active.kind === "approval"}
        <ShieldAlert size={14} class="ico" />
        <span class="title">{m.interaction_approval_title()}</span>
      {:else if active.kind === "ask"}
        <CircleHelp size={14} class="ico" />
        <span class="title">{m.interaction_ask_title()}</span>
      {:else if active.kind === "pending"}
        <FileDiff size={14} class="ico" />
        <span class="title">{m.interaction_pending_title()}</span>
        <span class="count">{m.interaction_pending_count({ n: active.data.length })}</span>
      {/if}
      {#if list.length > 1}
        <span class="queue">{m.interaction_queue({ i: 1, n: list.length })}</span>
      {/if}
      <button class="x" onclick={skipActive} aria-label={m.interaction_skip()}>
        <X size={12} />
      </button>
    </div>

    <div class="body">
      {#if active.kind === "approval"}
        <div class="tool">{active.data.tool_name}</div>
        {#if active.data.summary}
          <div class="summary">{active.data.summary}</div>
        {/if}
        {#if active.data.path}
          <div class="path">{active.data.path}</div>
        {/if}
        {#if active.data.escape}
          <div class="escape">{m.approval_escape_warning()}</div>
          {#if active.data.symlink_target}
            <div class="path">→ {active.data.symlink_target}</div>
          {/if}
        {/if}
        {#if active.data.destructive_mode}
          <div class="mode">
            {active.data.destructive_mode === "trash" ? m.approval_trash() : m.approval_permanent()}
          </div>
        {/if}
        {#if active.data.preview}
          <pre class="preview">{active.data.preview}</pre>
        {/if}
      {:else if active.kind === "ask"}
        <div class="ask-q">{active.data.question}</div>
        {#if active.data.context}
          <div class="ctx">{active.data.context}</div>
        {/if}
        {#if active.data.options && active.data.options.length > 0}
          <div class="options">
            {#each active.data.options as opt, i}
              <button
                type="button"
                class="opt"
                class:sel={askSelected.has(i)}
                onclick={() => pickCurrentOption(i)}
              >
                {opt}
              </button>
            {/each}
          </div>
        {/if}
        {#if active.data.multi_select && active.data.options && active.data.options.length > 0}
          <div class="confirm">
            <button class="apr yes" onclick={confirmCurrentMulti}>
              {m.ask_user_confirm()}
            </button>
          </div>
        {/if}
      {:else if active.kind === "pending"}
        <div class="files">
          {#each active.data as c (c.path)}
            <div class="file">
              <button
                type="button"
                class="file-head"
                onclick={() => (expanded[c.path] = !expanded[c.path])}
              >
                <ChevronRight size={12} class={expanded[c.path] ? "rot" : ""} />
                {#if c.kind === "created"}
                  <FilePlus size={12} />
                {:else if c.kind === "deleted"}
                  <FileX size={12} />
                {:else}
                  <FileEdit size={12} />
                {/if}
                <span class="fpath">{c.path}</span>
                <span class="kind">{c.kind}</span>
              </button>
              {#if expanded[c.path] && c.diff}
                <pre class="diff">{c.diff}</pre>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <div class="foot">
      {#if active.kind === "approval"}
        <div class="acts">
          <button
            class="apr yes"
            disabled={resolving[active.data.request_id] !== undefined}
            onclick={() => resolveApproval(active.data.request_id, true)}
          >
            {#if resolving[active.data.request_id] === true}
              <Loader size={12} class="spin" />
            {:else}
              <Check size={12} />
            {/if}
            {m.interaction_allow()}
          </button>
          <button
            class="apr no"
            disabled={resolving[active.data.request_id] !== undefined}
            onclick={() => resolveApproval(active.data.request_id, false)}
          >
            {#if resolving[active.data.request_id] === false}
              <Loader size={12} class="spin" />
            {:else}
              <X size={12} />
            {/if}
            {m.interaction_reject()}
          </button>
        </div>
        <span class="hint">{m.interaction_hint_allow()}</span>
      {:else if active.kind === "pending"}
        <div class="acts">
          <button class="apr yes" disabled={busy !== null} onclick={() => handlePending("approve")}>
            {#if busy === "approve"}
              <Loader size={12} class="spin" />
            {:else}
              <Check size={12} />
            {/if}
            {m.interaction_approve_all()}
          </button>
          <button class="apr no" disabled={busy !== null} onclick={() => handlePending("reject")}>
            {#if busy === "reject"}
              <Loader size={12} class="spin" />
            {:else}
              <X size={12} />
            {/if}
            {m.interaction_reject_all()}
          </button>
        </div>
        <span class="hint">{m.interaction_hint_allow()}</span>
      {:else if active.kind === "ask"}
        <div class="acts">
          <input
            class="foot-input"
            bind:this={askInput}
            bind:value={askText}
            placeholder={m.ask_user_placeholder()}
            onkeydown={(e) => {
              if (e.key === "Enter") submitCurrentAsk();
            }}
          />
          <button class="apr yes" onclick={submitCurrentAsk}>
            <Send size={12} /> {m.ask_user_send()}
          </button>
        </div>
        <span class="hint">{m.interaction_hint_send()}</span>
      {/if}
    </div>
  </div>
{/if}

<style>
  .interaction-overlay {
    position: absolute;
    bottom: 100%;
    left: 0;
    right: 0;
    margin-bottom: 0.5rem;
    z-index: 50;
    background: var(--popover);
    color: var(--popover-foreground);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
    max-height: 60vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .head,
  .foot {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.55rem;
  }
  .head { border-bottom: 1px solid var(--border); }
  .foot { border-top: 1px solid var(--border); }
  .head :global(.ico) { color: var(--muted-foreground); flex-shrink: 0; }
  .title { font-size: 0.75rem; font-weight: 600; }
  .count {
    font-size: 0.66rem;
    color: var(--muted-foreground);
    background: var(--muted);
    border-radius: var(--radius-sm);
    padding: 0.05rem 0.35rem;
  }
  .queue { font-size: 0.66rem; color: var(--muted-foreground); }
  .x {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    padding: 0.15rem;
    border: none;
    background: transparent;
    color: var(--muted-foreground);
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .x:hover { color: var(--foreground); background: var(--muted); }
  .body {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 0.5rem 0.55rem;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    font-size: 0.75rem;
  }
  .acts { display: flex; align-items: center; gap: 0.4rem; flex: 1; min-width: 0; }
  .hint {
    margin-left: auto;
    font-size: 0.64rem;
    color: var(--muted-foreground);
    white-space: nowrap;
  }
  .apr {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 0.75rem;
    cursor: default;
    flex-shrink: 0;
  }
  .apr.yes { background: hsl(140 60% 40%); color: white; border-color: hsl(140 60% 40%); }
  .apr.no { background: var(--destructive); color: var(--destructive-foreground); border-color: var(--destructive); }
  .apr:disabled { opacity: 0.55; }
  .foot-input {
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
  .foot-input:focus { border-color: var(--ring); }
  .tool { font-family: var(--font-mono); font-size: 0.72rem; }
  .summary { font-size: 0.75rem; }
  .path {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--muted-foreground);
    word-break: break-all;
  }
  .escape { font-size: 0.72rem; color: var(--destructive); font-weight: 600; }
  .mode {
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--muted-foreground);
  }
  .preview {
    margin: 0;
    padding: 0.4rem;
    background: var(--muted);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 0.7rem;
    white-space: pre-wrap;
    max-height: 180px;
    overflow-y: auto;
  }
  .ask-q { font-size: 0.78rem; }
  .ctx { font-size: 0.72rem; color: var(--muted-foreground); }
  .options { display: flex; flex-wrap: wrap; gap: 0.3rem; }
  .opt {
    padding: 0.2rem 0.5rem;
    font-size: 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--foreground);
    cursor: default;
    transition: background 0.12s, border-color 0.12s;
  }
  .opt:hover { border-color: var(--accent); }
  .opt.sel { background: var(--primary); color: var(--primary-foreground); }
  .confirm { display: flex; justify-content: flex-start; }
  .files { display: flex; flex-direction: column; gap: 0.15rem; }
  .file {
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
  .file-head {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    width: 100%;
    padding: 0.3rem 0.4rem;
    background: transparent;
    border: none;
    text-align: left;
    font-size: 0.72rem;
    color: var(--foreground);
    cursor: default;
  }
  .file-head :global(.rot) { transform: rotate(90deg); }
  .fpath {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
  }
  .kind { font-size: 0.62rem; color: var(--muted-foreground); text-transform: uppercase; }
  .diff {
    margin: 0;
    padding: 0.4rem;
    max-height: 200px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 0.68rem;
    white-space: pre;
    border-top: 1px solid var(--border);
  }
  :global(.spin) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>