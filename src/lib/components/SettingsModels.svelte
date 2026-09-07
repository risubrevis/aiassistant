<script lang="ts">
  import { onMount } from "svelte";
  import { config as configStore } from "$lib/stores/config";
  import {
    setDefaultsModel,
    setMaxTurns,
    setRagEnabled,
    ragClearAll,
    ragStatus,
    onRagCleared,
    providersActiveModels,
    providersAllModels,
    onProvidersChanged,
    onConfigReloaded,
    type AppConfig,
    type ModelOption,
    type ModelRef,
  } from "$lib/tauri";
  import { m } from "$lib/i18n";
  import { toast } from "$lib/stores/toasts";
  import Select from "./Select.svelte";

  type SelectItem = { value: string; label: string; disabled?: boolean };

  let primary = $state("");
  let secondary = $state("");
  let embedding = $state("");
  let ragEnabled = $state(true);
  let activeModels = $state<ModelOption[]>([]);
  let allModels = $state<ModelOption[]>([]);
  let dirty = $state(false);
  let ragDirty = $state(false);
  let saving = $state(false);
  let ragSaving = $state(false);
  let totalChunks = $state(0);
  let maxTurns = $state(50);
  let mtDirty = $state(false);
  let mtSaving = $state(false);

  // Pristine snapshots used by the section Cancel buttons to revert local
  // edits without a refetch. Refreshed from config on every sync.
  let lastRagEnabled = $state(true);
  let lastEmbedding = $state("");
  let lastMaxTurns = $state(50);

  const NOT_SELECTED = "";

  function refValue(mr: ModelRef | null): string {
    return mr && mr.provider && mr.model ? `${mr.provider}::${mr.model}` : "";
  }

  function syncFromConfig(cfg: AppConfig | null) {
    if (!cfg) return;
    primary = refValue(cfg.defaults.main_model);
    secondary = refValue(cfg.defaults.secondary_model);
    embedding = refValue(cfg.defaults.embedding_model);
    ragEnabled = cfg.defaults.rag_enabled;
    maxTurns = cfg.defaults.max_turns ?? 50;
    lastEmbedding = embedding;
    lastRagEnabled = ragEnabled;
    lastMaxTurns = maxTurns;
    dirty = false;
    ragDirty = false;
    mtDirty = false;
  }

  async function loadModels() {
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

  async function loadRagStatus() {
    try {
      const s = await ragStatus(null, null);
      totalChunks = s.total_chunks;
    } catch {
      totalChunks = 0;
    }
  }

  async function clearAllRag() {
    if (!window.confirm(m.rag_clear_confirm())) return;
    ragSaving = true;
    try {
      await ragClearAll();
      await loadRagStatus();
    } catch (e) {
      toast.error(m.rag_cleared_msg(), String(e));
    } finally {
      ragSaving = false;
    }
  }

  onMount(() => {
    const unsubConfig = configStore.subscribe(syncFromConfig);
    void loadModels();
    void loadRagStatus();
    let unlistenRag: (() => void) | undefined;
    let unlistenProviders: (() => void) | undefined;
    let unlistenConfig: (() => void) | undefined;
    void onRagCleared(() => {
      toast.info(m.rag_cleared_msg());
      void loadRagStatus();
    }).then((u) => {
      unlistenRag = u;
    });
    void onProvidersChanged(() => void loadModels()).then((u) => {
      unlistenProviders = u;
    });
    void onConfigReloaded(() => void loadModels()).then((u) => {
      unlistenConfig = u;
    });
    return () => {
      unsubConfig();
      unlistenRag?.();
      unlistenProviders?.();
      unlistenConfig?.();
    };
  });

  const activeItems = $derived<SelectItem[]>(
    activeModels.map((opt) => ({
      value: `${opt.provider_id}::${opt.model_id}`,
      label: `${opt.provider_name} / ${opt.display_name || opt.model_name}`,
    })),
  );

  // Keep the saved selection visible (disabled) when it points to a model on
  // an inactive provider or a disabled/deleted model.
  function withCurrent(items: SelectItem[], current: string): SelectItem[] {
    if (!current || items.some((it) => it.value === current)) return items;
    const opt = allModels.find((o) => `${o.provider_id}::${o.model_id}` === current);
    const label = opt
      ? `${opt.provider_name} / ${opt.display_name || opt.model_name}`
      : "Undefined";
    return [...items, { value: current, label: `${label} (inactive)`, disabled: true }];
  }

  const primaryItems = $derived(withCurrent(activeItems, primary));
  const secondaryOptions = $derived<SelectItem[]>([
    { value: NOT_SELECTED, label: m.settings_models_not_selected() },
    ...withCurrent(activeItems, secondary),
  ]);
  const embeddingOptions = $derived<SelectItem[]>([
    { value: NOT_SELECTED, label: m.settings_models_not_selected() },
    ...withCurrent(activeItems, embedding),
  ]);

  function parseValue(v: string): { provider: string; model: string } | null {
    if (!v) return null;
    const idx = v.indexOf("::");
    if (idx < 0) return null;
    return { provider: v.slice(0, idx), model: v.slice(idx + 2) };
  }

  async function save() {
    if (!primary) {
      toast.error(m.settings_models_primary_required());
      return;
    }
    // Capture all selections before any await: each setDefaultsModel emits
    // `config:reloaded`, which triggers syncFromConfig and overwrites local
    // state from the (not-yet-fully-saved) in-memory config.
    const p = parseValue(primary)!;
    const s = parseValue(secondary);
    saving = true;
    try {
      await setDefaultsModel("main_model", p.provider, p.model);
      await setDefaultsModel("secondary_model", s?.provider ?? null, s?.model ?? null);
      dirty = false;
      toast.success(m.settings_models_saved());
    } catch (err) {
      toast.error(m.settings_models_save_failed(), String(err));
    } finally {
      saving = false;
    }
  }

  async function reset() {
    saving = true;
    try {
      await setDefaultsModel("main_model", null, null);
      await setDefaultsModel("secondary_model", null, null);
      dirty = false;
      toast.success(m.settings_models_reset());
    } catch (err) {
      toast.error(m.settings_models_save_failed(), String(err));
    } finally {
      saving = false;
    }
  }

  function toggleRag() {
    ragEnabled = !ragEnabled;
    ragDirty = true;
  }

  function cancelRag() {
    ragEnabled = lastRagEnabled;
    embedding = lastEmbedding;
    ragDirty = false;
  }

  async function saveRag() {
    // Capture before any await (see save() above for rationale).
    const e = parseValue(embedding);
    const enabled = ragEnabled;
    ragSaving = true;
    try {
      await setRagEnabled(enabled);
      await setDefaultsModel("embedding_model", e?.provider ?? null, e?.model ?? null);
      ragDirty = false;
      toast.success(m.settings_models_saved());
    } catch (err) {
      toast.error(m.settings_models_save_failed(), String(err));
    } finally {
      ragSaving = false;
    }
  }

  function onMaxTurnsInput(e: Event) {
    const v = Number((e.target as HTMLInputElement).value);
    maxTurns = v;
    mtDirty = v !== lastMaxTurns;
  }

  function cancelMaxTurns() {
    maxTurns = lastMaxTurns;
    mtDirty = false;
  }

  async function saveMaxTurns() {
    const v = Math.max(1, Math.min(1000, Math.floor(maxTurns)));
    mtSaving = true;
    try {
      await setMaxTurns(v);
      maxTurns = v;
      lastMaxTurns = v;
      mtDirty = false;
      toast.success(m.settings_models_max_turns_saved());
    } catch (err) {
      toast.error(m.settings_models_max_turns_save_failed(), String(err));
    } finally {
      mtSaving = false;
    }
  }
</script>

<div class="models">
  {#if activeModels.length === 0 && !primary && !secondary && !embedding}
    <div class="empty">{m.settings_models_empty()}</div>
  {:else}
    <section class="block">
      <div class="fields">
        <div class="field">
          <span class="label">{m.settings_models_primary()}</span>
          <Select
            class="w-full"
            value={primary}
            items={primaryItems}
            placeholder={m.settings_models_not_selected()}
            onchange={(v) => {
              primary = v;
              dirty = true;
            }}
          />
        </div>
        <div class="field">
          <span class="label">{m.settings_models_secondary()}</span>
          <Select
            class="w-full"
            value={secondary}
            items={secondaryOptions}
            placeholder={m.settings_models_not_selected()}
            onchange={(v) => {
              secondary = v;
              dirty = true;
            }}
          />
        </div>
      </div>

      <div class="actions">
        <button class="btn primary" onclick={save} disabled={saving || !dirty}>
          {saving ? "…" : m.common_save()}
        </button>
        <button class="btn ghost" onclick={reset} disabled={saving}>
          {m.common_reset()}
        </button>
      </div>
    </section>

    <section class="block rag-section" class:disabled={!ragEnabled}>
      <div class="rag-head">
        <label class="rag-toggle">
          <input type="checkbox" checked={ragEnabled} onchange={toggleRag} />
          <span class="rag-toggle-label">{m.settings_models_rag_section()}</span>
        </label>
        <span class="rag-toggle-enabled">{m.settings_models_rag_enabled()}</span>
      </div>

      <div class="field">
        <span class="label">{m.settings_models_embedding()}</span>
        <Select
          class="w-full"
          value={embedding}
          items={embeddingOptions}
          placeholder={m.settings_models_not_selected()}
          disabled={!ragEnabled}
          title={ragEnabled ? undefined : m.settings_models_rag_disabled_hint()}
          onchange={(v) => {
            embedding = v;
            ragDirty = true;
          }}
        />
      </div>

      {#if !ragEnabled}
        <p class="rag-hint">{m.settings_models_rag_disabled_hint()}</p>
      {/if}

      <div class="rag-clear">
        <span class="rag-clear-info">
          {totalChunks} {m.rag_chunks()}
          <span class="muted"> · {m.rag_clear_all_hint()}</span>
        </span>
        <button class="btn danger" onclick={clearAllRag} disabled={ragSaving}>
          {m.rag_clear_all()}
        </button>
      </div>

      <div class="actions">
        <button class="btn primary" onclick={saveRag} disabled={ragSaving || !ragDirty}>
          {ragSaving ? "…" : m.common_save()}
        </button>
        <button class="btn ghost" onclick={cancelRag} disabled={ragSaving || !ragDirty}>
          {m.common_cancel()}
        </button>
      </div>
    </section>
  {/if}

  <section class="block max-turns-section">
    <div class="field">
      <span class="label">{m.settings_models_max_turns()}</span>
      <input
        class="max-turns-input"
        type="number"
        min="1"
        max="1000"
        step="1"
        value={maxTurns}
        oninput={onMaxTurnsInput}
      />
      <p class="max-turns-hint">{m.settings_models_max_turns_hint()}</p>
    </div>
    <div class="actions">
      <button class="btn primary" onclick={saveMaxTurns} disabled={mtSaving || !mtDirty}>
        {mtSaving ? "…" : m.common_save()}
      </button>
      <button class="btn ghost" onclick={cancelMaxTurns} disabled={mtSaving || !mtDirty}>
        {m.common_cancel()}
      </button>
    </div>
  </section>
</div>

<style>
  .models {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .label {
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.25rem;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.35rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    font-size: 0.8125rem;
    cursor: default;
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .btn.primary {
    background: var(--primary);
    color: var(--primary-foreground);
    border-color: var(--primary);
  }
  .btn.ghost {
    background: transparent;
  }
  .empty {
    color: var(--muted-foreground);
    font-size: 0.8125rem;
    padding: 0.5rem 0;
  }
  .rag-section {
    border-top: 1px solid var(--border);
    padding-top: 0.75rem;
  }
  .max-turns-section {
    border-top: 1px solid var(--border);
    padding-top: 0.75rem;
  }
  .max-turns-input {
    width: 8rem;
    padding: 0.3rem 0.5rem;
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
  }
  .max-turns-hint {
    margin: 0;
    font-size: 0.72rem;
    color: var(--muted-foreground);
    line-height: 1.35;
  }
  .rag-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .rag-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8125rem;
    font-weight: 600;
    cursor: default;
  }
  .rag-toggle input {
    width: 0.95rem;
    height: 0.95rem;
    cursor: default;
  }
  .rag-toggle-enabled {
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }
  .rag-hint {
    margin: 0;
    font-size: 0.72rem;
    color: var(--muted-foreground);
    line-height: 1.35;
  }
  .rag-clear {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding-top: 0.25rem;
    border-top: 1px solid var(--border);
    font-size: 0.75rem;
  }
  .rag-clear-info {
    color: var(--muted-foreground);
    min-width: 0;
  }
  .rag-clear-info .muted {
    opacity: 0.8;
  }
  .btn.danger {
    color: var(--destructive);
    border-color: var(--destructive);
  }
  .btn.danger:hover:not(:disabled) {
    background: var(--destructive);
    color: var(--destructive-foreground);
  }
</style>