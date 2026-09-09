<script lang="ts">
  import { X, Trash2, Brain } from "@lucide/svelte";
  import * as ipc from "$lib/tauri";
  import type { Memory, MemoryUpdateEvent } from "$lib/tauri";
  import { memoryModalOpen, memoryModalMode, memoryModalId } from "$lib/stores/memory";
  import { m } from "$lib/i18n";
  import ConfirmDialog from "./ConfirmDialog.svelte";

  let items = $state<Memory[]>([]);
  let deleteTarget = $state<Memory | null>(null);
  let clearOpen = $state(false);

  let open = $derived($memoryModalOpen);
  let mode = $derived($memoryModalMode);
  let id = $derived($memoryModalId);

  $effect(() => {
    if (open && id) void load(id);
  });

  // Re-register the update listener for the lifetime of the open modal.
  $effect(() => {
    if (!open) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void ipc.onChatMemoryUpdate(onMemoryUpdate).then((u) => {
      if (disposed) u();
      else unlisten = u;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function load(targetId: string) {
    try {
      items =
        mode === "chat" ? await ipc.memoryListChat(targetId) : await ipc.memoryListProject(targetId);
    } catch (e) {
      console.error("memory load failed", e);
    }
  }

  function onMemoryUpdate(_e: MemoryUpdateEvent) {
    if (open && id) void load(id);
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target || !id) return;
    try {
      await ipc.memoryDelete(target.id);
      deleteTarget = null;
      await load(id);
    } catch (e) {
      console.error("memoryDelete failed", e);
    }
  }

  async function confirmClear() {
    if (!id) return;
    try {
      if (mode === "chat") await ipc.memoryClearChat(id);
      else await ipc.memoryClearProject(id);
      clearOpen = false;
      await load(id);
    } catch (e) {
      console.error("memory clear failed", e);
    }
  }

  function close() {
    memoryModalOpen.set(false);
  }

  function onKeydown(e: KeyboardEvent) {
    if (deleteTarget !== null || clearOpen) {
      if (e.key === "Escape") {
        deleteTarget = null;
        clearOpen = false;
      }
      return;
    }
    if (e.key === "Escape") close();
  }
</script>

{#if open}
  <div class="overlay" onkeydown={onKeydown} role="presentation">
    <div class="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
      <header class="head">
        <span class="head-left">
          <Brain size={14} />
          <span>{mode === "chat" ? m.memory_chat_title() : m.memory_project_title()}</span>
        </span>
        <button class="close" title={m.common_close()} onclick={close}><X size={16} /></button>
      </header>

      <div class="body">
        {#if items.length === 0}
          <div class="empty">{m.memory_empty()}</div>
        {:else}
          {#each items as item (item.id)}
            <div class="mem-row">
              <div class="mem-main">
                <div class="mem-content">{item.content}</div>
                <div class="mem-meta">
                  {#if item.category}
                    <span class="tag">{item.category}</span>
                  {/if}
                  <span class="time">{new Date(item.created_at).toLocaleString()}</span>
                </div>
              </div>
              <button class="icon-btn" title={m.common_delete()} onclick={() => (deleteTarget = item)}>
                <Trash2 size={13} />
              </button>
            </div>
          {/each}
        {/if}
      </div>

      <footer class="foot">
        <button class="danger" disabled={items.length === 0} onclick={() => (clearOpen = true)}>
          <Trash2 size={13} /> {m.memory_clear()}
        </button>
        <div class="spacer"></div>
        <button class="ghost" onclick={close}>{m.common_close()}</button>
      </footer>
    </div>
  </div>

  <ConfirmDialog
    open={deleteTarget !== null}
    message={m.memory_delete_confirm()}
    onconfirm={() => void confirmDelete()}
    oncancel={() => (deleteTarget = null)}
  />

  <ConfirmDialog
    open={clearOpen}
    message={mode === "chat" ? m.memory_clear_confirm_chat() : m.memory_clear_confirm_project()}
    onconfirm={() => void confirmClear()}
    oncancel={() => (clearOpen = false)}
  />
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .dialog {
    width: 560px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
    font-size: 0.875rem;
    font-weight: 500;
  }
  .head-left {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--foreground);
  }
  .close {
    display: inline-flex;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .close:hover {
    background: var(--accent);
  }
  .body {
    padding: 0.875rem 1rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .mem-row {
    display: flex;
    align-items: flex-start;
    gap: 0.4rem;
    padding: 0.45rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .mem-main {
    flex: 1;
    min-width: 0;
  }
  .mem-content {
    font-size: 0.8125rem;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .mem-meta {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-top: 0.2rem;
  }
  .tag {
    padding: 0.05rem 0.35rem;
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: var(--accent-foreground);
    font-size: 0.68rem;
  }
  .time {
    font-size: 0.7rem;
    color: var(--muted-foreground);
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    padding: 0.2rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted-foreground);
    cursor: default;
  }
  .icon-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .empty {
    font-size: 0.78rem;
    color: var(--muted-foreground);
    padding: 0.25rem 0;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 0.875rem;
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
  .foot button {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    font-size: 0.8125rem;
    cursor: default;
  }
  .foot button:disabled {
    opacity: 0.6;
  }
  .ghost {
    background: var(--background);
    color: var(--foreground);
  }
  .danger {
    background: var(--destructive);
    color: var(--destructive-foreground);
    border-color: var(--destructive);
  }
  .danger:hover {
    filter: brightness(0.92);
  }
  .danger:disabled {
    filter: none;
  }
</style>