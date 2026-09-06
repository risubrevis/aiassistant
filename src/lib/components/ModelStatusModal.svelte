<script lang="ts">
  import { onMount } from "svelte";
  import { X } from "@lucide/svelte";
  import {
    providersAllModels,
    onProvidersChanged,
    type ModelOption,
    type ModelStatus,
  } from "$lib/tauri";
  import { m } from "$lib/i18n";

  let {
    open,
    providerId,
    model,
    status,
    onclose,
  }: {
    open: boolean;
    providerId: string | null;
    model: string | null;
    status: ModelStatus | null;
    onclose: () => void;
  } = $props();

  let allModels = $state<ModelOption[]>([]);

  async function loadAllModels() {
    try {
      allModels = await providersAllModels();
    } catch {
      allModels = [];
    }
  }

  onMount(() => {
    void loadAllModels();
    let unlisten: (() => void) | undefined;
    void onProvidersChanged(() => void loadAllModels()).then((u) => (unlisten = u));
    return () => unlisten?.();
  });

  const opt = $derived(
    allModels.find((o) => o.provider_id === providerId && o.model_id === model) ?? null,
  );
  const modelLabel = $derived(
    opt ? opt.display_name || opt.model_name : "Undefined",
  );
  const providerLabel = $derived(opt?.provider_name ?? "Undefined");

  const STATE_LABEL: Record<string, () => string> = {
    unknown: () => m.model_status_unknown(),
    loaded: () => m.model_status_loaded(),
    error: () => m.model_status_error(),
    cloud: () => m.model_status_cloud(),
  };

  let dialogEl = $state<HTMLDivElement>();

  $effect(() => {
    if (open) dialogEl?.focus();
  });

  let statusState = $derived(status?.state ?? "unknown");
  let stateLabel = $derived(STATE_LABEL[statusState]?.() ?? statusState);
  let entries = $derived(Object.entries(status?.detail ?? {}));

  function humanKey(key: string): string {
    return key
      .split("_")
      .filter(Boolean)
      .map((w) => w[0].toUpperCase() + w.slice(1))
      .join(" ");
  }

  function formatBytes(n: number): string {
    if (!Number.isFinite(n)) return String(n);
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = n;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${i === 0 ? v : v.toFixed(1)} ${units[i]}`;
  }

  function isByteKey(key: string): boolean {
    return key === "size" || key === "size_vram" || key.endsWith("_bytes") || key.includes("bytes");
  }

  function formatValue(key: string, value: unknown): string {
    if (value === null || value === undefined) return "—";
    if (typeof value === "boolean") return value ? "yes" : "no";
    if (typeof value === "number") {
      return isByteKey(key) ? formatBytes(value) : value.toLocaleString("en-US");
    }
    if (Array.isArray(value)) return value.map((v) => String(v)).join(", ") || "—";
    if (typeof value === "object") return JSON.stringify(value);
    return String(value);
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (open && e.key === "Escape") onclose();
  }}
/>

{#if open}
  <div class="overlay" onkeydown={() => {}} role="presentation">
    <div
      class="dialog"
      tabindex="-1"
      bind:this={dialogEl}
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
      role="dialog"
    >
      <header class="head">
        <span class="title">{m.model_status_title()} — {modelLabel}</span>
        <button class="close" title={m.common_close()} onclick={onclose}><X size={16} /></button>
      </header>

      <div class="body">
        <div class="row">
          <span class="badge {statusState}">{stateLabel}</span>
          <span class="provider" title={providerId ?? ""}>{providerLabel}</span>
        </div>

        {#if entries.length}
          <div class="detail">
            {#each entries as [key, value] (key)}
              <div class="kv">
                <span class="k">{humanKey(key)}</span>
                <span class="v">{formatValue(key, value)}</span>
              </div>
            {/each}
          </div>
        {:else}
          <div class="empty">{m.model_status_no_detail()}</div>
        {/if}
      </div>
    </div>
  </div>
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
    width: 480px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
    outline: none;
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
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    gap: 0.6rem;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    padding: 0.15rem 0.65rem;
    border-radius: 9999px;
    font-size: 0.75rem;
    font-weight: 600;
    color: white;
    flex-shrink: 0;
  }
  .badge.unknown {
    background: var(--muted-foreground);
  }
  .badge.loaded {
    background: hsl(140 50% 45%);
  }
  .badge.error {
    background: var(--destructive);
  }
  .badge.cloud {
    background: hsl(210 90% 50%);
  }
  .provider {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.78rem;
    color: var(--muted-foreground);
  }
  .detail {
    display: flex;
    flex-direction: column;
  }
  .kv {
    display: flex;
    gap: 0.75rem;
    padding: 0.3rem 0;
    border-bottom: 1px solid var(--border);
    font-size: 0.8125rem;
  }
  .kv:last-child {
    border-bottom: none;
  }
  .k {
    flex: 0 0 40%;
    color: var(--muted-foreground);
  }
  .v {
    flex: 1;
    font-family: var(--font-mono);
    word-break: break-word;
  }
  .empty {
    color: var(--muted-foreground);
    font-size: 0.8125rem;
  }
</style>