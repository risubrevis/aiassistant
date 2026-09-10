<script lang="ts">
  import { get } from "svelte/store";
  import { ptyInput, type Chat } from "$lib/tauri";
  import {
    termAgentEntriesByChat,
    exportTerminalToFile,
    type AgentCmdEntry,
  } from "$lib/stores/terminal";
  import { ptyByBlock, chatRightPanel, setChatRightPanel } from "$lib/stores/chat";
  import { toast } from "$lib/stores/toasts";
  import { DEFAULT_RIGHT, clampRight } from "$lib/stores/layout";
  import { m } from "$lib/i18n";
  import { TerminalSquare, Download, X, Loader, Send } from "@lucide/svelte";

  let { chat }: { chat: Chat } = $props();

  let rp = $derived(chatRightPanel(chat, DEFAULT_RIGHT));
  let width = $state(DEFAULT_RIGHT);

  $effect(() => {
    void chat.id;
    void chat.settings;
    width = clampRight(chatRightPanel(chat, DEFAULT_RIGHT).width);
  });

  let entries = $derived($termAgentEntriesByChat[chat.id] ?? []);

  let exporting = $state(false);
  let ptyText: Record<string, string> = $state({});

  let bottomEl: HTMLDivElement | undefined = $state();
  let historyEl: HTMLDivElement | undefined = $state();
  let stick = $state(true);

  // Keep the newest card in view: on open, on new entries and on streaming output.
  // Only auto-scrolls while the user has not scrolled up ("stick to bottom").
  $effect(() => {
    void entries.length;
    const last = entries[entries.length - 1];
    void last?.output;
    void last?.code;
    if (stick) bottomEl?.scrollIntoView();
  });

  function sendCardInput(entry: AgentCmdEntry) {
    const text = ptyText[entry.block_id] ?? "";
    if (!text.trim()) return;
    const sid = get(ptyByBlock)[entry.block_id]?.session_id;
    if (!sid) return;
    ptyInput(sid, text + "\n")
      .then(() => {
        ptyText[entry.block_id] = "";
      })
      .catch((err) => {
        toast.error(String(err));
      });
  }

  async function doExport() {
    if (exporting) return;
    exporting = true;
    try {
      await exportTerminalToFile(chat.id, chat.title);
    } catch {
      // toast is handled in the store
    }
    exporting = false;
  }

  let asideEl: HTMLElement | undefined = $state();
  let resizing = $state(false);

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onResizeMove(e: PointerEvent) {
    if (!resizing || !asideEl) return;
    const rect = asideEl.getBoundingClientRect();
    width = clampRight(rect.right - e.clientX);
  }
  function onResizeUp(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch {}
    void setChatRightPanel(chat.id, rp.open, rp.mode, width);
  }
</script>

<aside class="terminal-panel" bind:this={asideEl} style="width:{width}px">
  <div
    class="resize-handle"
    class:active={resizing}
    role="separator"
    aria-orientation="vertical"
    tabindex="-1"
    onpointerdown={startResize}
    onpointermove={onResizeMove}
    onpointerup={onResizeUp}
    onpointercancel={onResizeUp}
  ></div>

  <div class="term-head">
    <span class="term-title"><TerminalSquare size={14} />{m.terminal_title()}</span>
    <div class="term-actions">
      <button
        class="icon-btn"
        title={exporting ? m.terminal_exporting() : m.terminal_export()}
        disabled={exporting}
        onclick={() => void doExport()}
      >
        <Download size={14} />
      </button>
      <button
        class="icon-btn"
        title={m.common_close()}
        onclick={() => { void setChatRightPanel(chat.id, false, rp.mode, width); }}
      >
        <X size={14} />
      </button>
    </div>
  </div>

  <div
    class="term-history"
    bind:this={historyEl}
    onscroll={(e) => {
      const el = e.currentTarget;
      stick = el.scrollTop + el.clientHeight >= el.scrollHeight - 32;
    }}
  >
    {#each entries as entry (entry.block_id)}
      {@const sid = $ptyByBlock[entry.block_id]?.session_id}
      <div class="term-card" class:live={entry.code === null}>
        <div class="term-cmd">$ {entry.command}</div>
        {#if entry.output}
          <pre class="term-out">{entry.output}</pre>
        {/if}
        {#if entry.code !== null}
          <span class="term-exit" class:err={entry.is_error || entry.code !== 0}>{m.terminal_exit_code({ code: entry.code })}</span>
        {:else}
          <div class="term-live">
            <Loader size={12} class="term-spin" />
            {#if entry.needsInput}
              <span class="term-badge">{m.terminal_needs_input()}</span>
            {/if}
          </div>
          <div class="pty-input-row">
            <input
              class="pty-input"
              type="text"
              value={ptyText[entry.block_id] ?? ""}
              disabled={!sid}
              placeholder={m.terminal_input_placeholder()}
              oninput={(e) => { ptyText[entry.block_id] = e.currentTarget.value; }}
              onkeydown={(e) => {
                if (e.key === "Enter") sendCardInput(entry);
              }}
            />
            <button type="button" class="pty-send" disabled={!sid} onclick={() => sendCardInput(entry)}>
              <Send size={12} />
            </button>
          </div>
        {/if}
      </div>
    {/each}
    <div bind:this={bottomEl}></div>
  </div>
</aside>

<style>
  .terminal-panel {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    height: 100%;
    border-left: 1px solid var(--border);
    background: var(--background);
    position: relative;
    min-height: 0;
  }
  .term-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--header-height);
    padding: 0 0.625rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .term-title {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .term-actions {
    display: inline-flex;
    align-items: center;
    gap: 0.15rem;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    border-radius: var(--radius-sm);
    cursor: default;
    padding: 0.15rem;
  }
  .icon-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .icon-btn:disabled {
    opacity: 0.5;
  }
  .term-history {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.5rem 0.75rem;
  }
  .term-card {
    margin-bottom: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .term-card.live .term-out {
    max-height: none;
  }
  .term-cmd {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--muted-foreground);
    overflow-wrap: anywhere;
  }
  .term-out {
    margin: 0;
    padding: 0.4rem;
    background: var(--muted);
    color: var(--foreground);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 240px;
    overflow-y: auto;
  }
  .term-exit {
    font-size: 0.7rem;
    color: var(--muted-foreground);
  }
  .term-exit.err {
    color: var(--destructive);
  }
  .term-live {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.7rem;
    color: var(--muted-foreground);
  }
  .term-live :global(.term-spin) {
    animation: term-rotate 1s linear infinite;
  }
  @keyframes term-rotate {
    to {
      transform: rotate(360deg);
    }
  }
  .term-badge {
    padding: 0.05rem 0.35rem;
    border-radius: 9999px;
    background: hsl(45 95% 55% / 0.18);
    color: hsl(45 95% 45%);
    font-size: 0.65rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .pty-input-row { display: flex; gap: 0.3rem; }
  .pty-input {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    padding: 0.25rem 0.4rem;
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--foreground);
  }
  .pty-input:disabled {
    opacity: 0.5;
  }
  .pty-send {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.25rem 0.45rem;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--muted-foreground);
    cursor: default;
  }
  .pty-send:hover:not(:disabled) {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .pty-send:disabled {
    opacity: 0.5;
  }
  .resize-handle {
    position: absolute;
    top: 0;
    left: -3px;
    bottom: 0;
    width: 7px;
    z-index: 10;
    cursor: col-resize;
    background: transparent;
  }
  .resize-handle:hover,
  .resize-handle.active {
    background-color: var(--ring);
    opacity: 0.35;
  }
</style>