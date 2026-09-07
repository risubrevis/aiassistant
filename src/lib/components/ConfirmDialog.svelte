<script lang="ts">
  import { LoaderCircle } from "@lucide/svelte";
  import { m } from "$lib/i18n";

  let {
    open,
    message,
    confirmLabel,
    cancelLabel,
    loading = false,
    loadingLabel,
    onconfirm,
    oncancel,
  }: {
    open: boolean;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    loading?: boolean;
    loadingLabel?: string;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();

  const confirm = $derived(confirmLabel ?? m.common_delete());
  const cancel = $derived(cancelLabel ?? m.common_cancel());
  const loadingText = $derived(loadingLabel ?? m.common_deleting());

  function onKeydown(e: KeyboardEvent) {
    if (loading) return;
    if (e.key === "Escape") oncancel();
  }
</script>

{#if open}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog small"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <div class="body">
        <div class="confirm">{message}</div>
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={oncancel} disabled={loading}>{cancel}</button>
        <button class="btn danger" onclick={onconfirm} disabled={loading}>
          {#if loading}
            <LoaderCircle size={12} class="spin" /> {loadingText}
          {:else}
            {confirm}
          {/if}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .dialog {
    width: 480px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.28);
  }
  .dialog.small {
    width: 360px;
  }
  .body {
    padding: 0.875rem 1rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .confirm {
    font-size: 0.8125rem;
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
    opacity: 0.6;
    cursor: default;
  }
  .btn.danger {
    background: var(--destructive);
    border-color: var(--destructive);
    color: var(--destructive-foreground);
  }
  .btn.danger:hover {
    filter: brightness(0.92);
    background: var(--destructive);
  }
  .btn.danger:disabled {
    filter: none;
  }
  :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>