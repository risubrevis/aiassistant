<script lang="ts">
  import { onMount } from "svelte";
  import {
    providersActiveModels,
    providersAllModels,
    providerModelStatus,
    onProvidersChanged,
    type ModelOption,
    type ModelStatus,
  } from "$lib/tauri";
  import { setChatModel } from "$lib/stores/chat";
  import { m } from "$lib/i18n";
  import { ChevronDown, Activity } from "@lucide/svelte";
  import ModelStatusModal from "./ModelStatusModal.svelte";

  let {
    chatId,
    providerId,
    modelId,
  }: { chatId: string; providerId: string | null; modelId: string | null } = $props();

  let activeModels = $state<ModelOption[]>([]);
  let allModels = $state<ModelOption[]>([]);
  let open = $state(false);
  let triggerEl = $state<HTMLButtonElement | null>(null);
  let menuEl = $state<HTMLDivElement | null>(null);
  let menuTop = $state("");
  let menuLeft = $state("");
  let menuMinWidth = $state("");
  let status = $state<ModelStatus | null>(null);
  let statusModalOpen = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | undefined;

  const STATUS_POLL_MS = 10_000;

  async function loadOptions() {
    try {
      activeModels = await providersActiveModels();
    } catch {
      activeModels = [];
    }
    try {
      allModels = await providersAllModels();
    } catch {
      allModels = [];
    }
  }

  onMount(() => {
    void loadOptions();
    let unlisten: (() => void) | undefined;
    void onProvidersChanged(() => void loadOptions()).then((u) => (unlisten = u));
    return () => unlisten?.();
  });

  // Current selection resolved against active providers first, then all
  // providers (covers models on inactive providers / disabled models).
  const currentActive = $derived(
    providerId && modelId
      ? (activeModels.find(
          (o) => o.provider_id === providerId && o.model_id === modelId,
        ) ?? null)
      : null,
  );
  const currentAny = $derived(
    providerId && modelId
      ? (allModels.find(
          (o) => o.provider_id === providerId && o.model_id === modelId,
        ) ?? null)
      : null,
  );

  type Group = { name: string; items: ModelOption[] };
  const groups = $derived.by(() => {
    const out: Group[] = [];
    for (const opt of activeModels) {
      let g = out.find((x) => x.name === opt.provider_name);
      if (!g) {
        g = { name: opt.provider_name, items: [] };
        out.push(g);
      }
      g.items.push(opt);
    }
    return out;
  });

  const currentLabel = $derived.by(() => {
    const opt = currentActive ?? currentAny;
    if (opt) return `${opt.provider_name} / ${opt.display_name || opt.model_name}`;
    return "Select model";
  });

  let statusReq = 0;

  async function refreshStatus(provId: string, modelId: string) {
    if (!provId || !modelId) {
      status = null;
      return;
    }
    const req = ++statusReq;
    try {
      const res = await providerModelStatus(provId, modelId);
      if (req === statusReq) status = res;
    } catch {
      if (req === statusReq) status = { state: "unknown", detail: null };
    }
  }

  $effect(() => {
    const prov = providerId;
    const model = modelId;
    if (!prov || !model) {
      status = null;
      return;
    }
    void refreshStatus(prov, model);
    pollTimer = setInterval(() => void refreshStatus(prov, model), STATUS_POLL_MS);
    return () => clearInterval(pollTimer);
  });

  async function pick(opt: ModelOption) {
    open = false;
    await setChatModel(chatId, opt.provider_id, opt.model_id);
  }

  function toggle() {
    open = !open;
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (!open || e.key !== "Escape") return;
    e.stopPropagation();
    e.preventDefault();
    open = false;
  }

  function positionMenu() {
    if (!open || !triggerEl) return;
    const r = triggerEl.getBoundingClientRect();
    let top = r.bottom + 4;
    let left = r.left;
    if (menuEl) {
      const rect = menuEl.getBoundingClientRect();
      if (top + rect.height > window.innerHeight - 4) {
        top = Math.max(4, r.top - rect.height - 4);
      }
      if (left + rect.width > window.innerWidth - 4) {
        left = Math.max(4, window.innerWidth - rect.width - 4);
      }
    }
    menuTop = `${top}px`;
    menuLeft = `${left}px`;
    menuMinWidth = `${r.width}px`;
  }

  $effect(() => {
    if (!open) return;
    positionMenu();
    requestAnimationFrame(positionMenu);
  });
</script>

<svelte:window
  onkeydowncapture={onWindowKeydown}
  onscroll={() => open && (open = false)}
  onresize={() => open && (open = false)}
/>

<div class="model-selector">
  <div class="ms-row">
    <button class="ms-trigger" bind:this={triggerEl} onclick={toggle}>
      <span class="ms-label" class:inactive={!currentActive && !!currentAny}>{currentLabel}</span>
      <ChevronDown size={14} />
    </button>
    <button
      class="ms-status {status?.state ?? 'unknown'}"
      title={`Model status: ${status?.state ?? "unknown"}`}
      onclick={() => (statusModalOpen = true)}
    >
      <Activity size={13} />
    </button>
  </div>

  {#if open}
    <button class="ms-backdrop" aria-label={m.common_close()} onclick={() => (open = false)}></button>
    <div class="ms-menu" bind:this={menuEl} style:top={menuTop} style:left={menuLeft} style:min-width={menuMinWidth}>
      {#each groups as g (g.name)}
        <div class="ms-group">{g.name}</div>
        {#each g.items as opt (opt.model_id)}
          <button
            class="ms-item"
            class:active={currentActive === opt}
            onclick={() => pick(opt)}
          >
            {opt.display_name || opt.model_name}
          </button>
        {/each}
      {/each}
      {#if currentAny && !currentActive}
        <!-- Current model on an inactive provider or a disabled model: visible but not selectable. -->
        <div class="ms-group">{currentAny.provider_name}</div>
        <button class="ms-item" disabled>
          {currentAny.display_name || currentAny.model_name}
        </button>
      {/if}
      {#if activeModels.length === 0 && !currentAny}
        <div class="ms-empty">No models — configure providers in Settings</div>
      {/if}
    </div>
  {/if}
</div>

<ModelStatusModal
  open={statusModalOpen}
  providerId={providerId}
  model={modelId}
  status={status}
  onclose={() => (statusModalOpen = false)}
/>

<style>
  .model-selector {
    position: relative;
  }
  .ms-row {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    min-width: 0;
  }
  .ms-trigger {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    max-width: 240px;
    height: 1.75rem;
    padding: 0 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    cursor: default;
  }
  .ms-trigger:hover {
    background: var(--accent);
  }
  .ms-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ms-label.inactive {
    color: var(--muted-foreground);
  }
  .ms-status {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.75rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .ms-status:hover {
    background: var(--accent);
  }
  .ms-status.loaded {
    color: hsl(140 50% 45%);
  }
  .ms-status.error {
    color: var(--destructive);
  }
  .ms-status.cloud {
    color: hsl(210 90% 50%);
  }
  .ms-backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: transparent;
    border: none;
    padding: 0;
    cursor: default;
  }
  .ms-menu {
    position: fixed;
    width: max-content;
    max-width: 360px;
    max-height: 320px;
    overflow-y: auto;
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--popover);
    color: var(--popover-foreground);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.18);
    padding: 0.25rem;
    z-index: 30;
  }
  .ms-group {
    padding: 0.25rem 0.5rem;
    font-size: 0.6875rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted-foreground);
  }
  .ms-item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 0.3rem 0.5rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    font-size: 0.8125rem;
    white-space: nowrap;
    cursor: default;
  }
  .ms-item:hover:not(:disabled) {
    background: var(--accent);
  }
  .ms-item.active {
    background: var(--secondary);
    color: var(--secondary-foreground);
  }
  .ms-item:disabled {
    opacity: 0.5;
  }
  .ms-empty {
    padding: 0.5rem;
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
</style>