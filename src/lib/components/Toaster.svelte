<script lang="ts">
  import { toasts, dismissToast, type ToastKind } from "$lib/stores/toasts";
  import { X, Info, Check, AlertCircle, AlertTriangle } from "@lucide/svelte";
  import { fly } from "svelte/transition";

  const icons: Record<ToastKind, typeof Info> = {
    info: Info,
    success: Check,
    error: AlertCircle,
    warning: AlertTriangle,
  };
</script>

{#if $toasts.length > 0}
  <div class="toaster" aria-live="polite">
    {#each $toasts as t (t.id)}
      {@const Icon = icons[t.kind]}
      <div class="toast {t.kind}" role="status" transition:fly={{ y: -8, duration: 150 }}>
        <span class="icon"><Icon size={14} /></span>
        <div class="body">
          <div class="title">{t.title}</div>
          {#if t.message}
            <div class="msg">{t.message}</div>
          {/if}
        </div>
        <button class="close" aria-label="Close" onclick={() => dismissToast(t.id)}>
          <X size={12} />
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toaster {
    position: fixed;
    top: 0.75rem;
    right: 0.75rem;
    z-index: 100;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    width: 320px;
    max-width: calc(100vw - 1.5rem);
  }
  .toast {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.625rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card, var(--background));
    color: var(--foreground);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.25);
  }
  .icon {
    display: inline-flex;
    margin-top: 0.125rem;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .title {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .msg {
    margin-top: 0.125rem;
    font-size: 0.75rem;
    color: var(--muted-foreground);
    overflow-wrap: anywhere;
  }
  .close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 1.25rem;
    width: 1.25rem;
    flex-shrink: 0;
    border: none;
    background: transparent;
    color: var(--muted-foreground);
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .close:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .toast.info {
    border-left: 3px solid #3b82f6;
  }
  .toast.info .icon {
    color: #3b82f6;
  }
  .toast.success {
    border-left: 3px solid #22c55e;
  }
  .toast.success .icon {
    color: #22c55e;
  }
  .toast.error {
    border-left: 3px solid var(--destructive);
  }
  .toast.error .icon {
    color: var(--destructive);
  }
  .toast.warning {
    border-left: 3px solid #f59e0b;
  }
  .toast.warning .icon {
    color: #f59e0b;
  }
</style>