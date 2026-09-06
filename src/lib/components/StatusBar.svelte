<script lang="ts">
  import { m } from "$lib/i18n";
  import { currentChatId, statusByChat } from "$lib/stores/chat";
  import { agentsPanelOpen, runningCount } from "$lib/stores/agents";
  import { Bot } from "@lucide/svelte";

  $: status = $statusByChat[$currentChatId ?? ""] ?? "idle";
  $: label =
    status === "running"
      ? m.status_running()
      : status === "error"
        ? m.status_error()
        : status === "cancelled"
          ? m.status_cancelled()
          : m.status_idle();
</script>

<footer
  class="flex h-6 shrink-0 items-center justify-between border-t border-border bg-background px-3 text-xs text-muted-foreground"
>
  <span class="flex items-center gap-1.5">
    <span class="dot {status}"></span>
    {label}
    <button
      class="ag-ind"
      class:open={$agentsPanelOpen}
      title={m.agents_indicator()}
      onclick={() => agentsPanelOpen.update((v) => !v)}
    >
      <Bot size={12} />
      {#if $runningCount > 0}
        <span class="ag-badge">{$runningCount}</span>
      {/if}
    </button>
  </span>
  <span class="opacity-60">AI Assistant</span>
</footer>

<style>
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 9999px;
    background: var(--muted-foreground);
    opacity: 0.6;
  }
  .dot.running {
    background: hsl(217 91% 60%);
    opacity: 1;
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dot.error {
    background: var(--destructive);
    opacity: 1;
  }
  .ag-ind {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    margin-left: 0.35rem;
    padding: 0.05rem 0.35rem;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted-foreground);
    opacity: 0.7;
    cursor: default;
  }
  .ag-ind:hover {
    background: var(--accent);
    color: var(--accent-foreground);
    opacity: 1;
  }
  .ag-ind.open {
    background: var(--secondary);
    color: var(--secondary-foreground);
    opacity: 1;
  }
  .ag-badge {
    font-size: 0.6rem;
    line-height: 1;
    padding: 0.08rem 0.32rem;
    border-radius: 9999px;
    background: hsl(217 91% 60%);
    color: white;
  }
  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }
</style>