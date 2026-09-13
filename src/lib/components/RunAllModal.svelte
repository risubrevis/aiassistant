<script lang="ts">
  import { m } from "$lib/i18n";
  import { Play, X } from "@lucide/svelte";
  import type { BatchMode } from "$lib/stores/projectTasksBatch";

  let {
    open,
    todoCount,
    onrun,
    onclose,
  }: {
    open: boolean;
    todoCount: number;
    onrun: (mode: BatchMode) => void;
    onclose: () => void;
  } = $props();

  let oneChat = $state(false);
  let simultaneous = $state(false);

  $effect(() => {
    if (open) {
      oneChat = false;
      simultaneous = false;
    }
  });

  let simultaneousDisabled = $derived(oneChat);

  function toggleOneChat() {
    oneChat = !oneChat;
    if (oneChat) simultaneous = false;
  }

  function toggleSimultaneous() {
    if (simultaneousDisabled) return;
    simultaneous = !simultaneous;
  }

  function start() {
    const mode: BatchMode = oneChat ? "one_chat" : simultaneous ? "simultaneous" : "sequential";
    onrun(mode);
  }

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="overlay" role="presentation" onclick={onclose} onkeydown={onKeydown}>
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">
        <Play size={15} />
        <span class="name">{m.board_run_all_title()}</span>
        <button class="x" title={m.common_close()} onclick={onclose}><X size={15} /></button>
      </header>
      <div class="body">
        <p class="hint">{m.board_run_all_hint()}</p>
        <p class="count">{m.board_run_all_count({ n: todoCount })}</p>
        <div class="field">
          <div class="check">
            <input type="checkbox" checked={oneChat} onchange={toggleOneChat} />
            <span>{m.board_run_all_one_chat()}</span>
          </div>
        </div>
        <div class="field">
          <div class="check" class:disabled={simultaneousDisabled}>
            <input
              type="checkbox"
              checked={simultaneous}
              disabled={simultaneousDisabled}
              onchange={toggleSimultaneous}
            />
            <span>{m.board_run_all_simultaneous()}</span>
          </div>
        </div>
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={onclose}>{m.ptask_cancel()}</button>
        <button class="btn primary" onclick={start} disabled={todoCount === 0}>
          <Play size={13} />
          {m.board_run_all_start()}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .dialog {
    width: 440px;
    max-width: 92vw;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.28);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
  }
  .head :global(svg) {
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .name {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    margin-left: auto;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
    padding: 0.15rem;
    flex-shrink: 0;
  }
  .x:hover {
    background-color: var(--accent);
    color: var(--accent-foreground);
  }
  .body {
    padding: 0.75rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .hint {
    margin: 0;
    font-size: 0.78rem;
    color: var(--muted-foreground);
  }
  .count {
    margin: 0;
    font-size: 0.78rem;
    font-weight: 600;
  }
  .field {
    display: flex;
    flex-direction: column;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8rem;
    padding: 0.15rem 0;
    cursor: pointer;
  }
  .check input {
    cursor: pointer;
  }
  .check.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .check.disabled input {
    cursor: not-allowed;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.55rem 0.875rem;
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.78rem;
    cursor: default;
  }
  .btn:hover {
    background: var(--accent);
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .btn.primary {
    background: hsl(239 84% 67%);
    border-color: hsl(239 84% 67%);
    color: white;
  }
  .btn.primary:hover {
    filter: brightness(0.92);
    background: hsl(239 84% 67%);
  }
  .btn.primary:disabled {
    filter: none;
  }
</style>