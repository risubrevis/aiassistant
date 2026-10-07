<script lang="ts">
  import { onMount } from "svelte";
  import { config as configStore } from "$lib/stores/config";
  import {
    setDefaultsModel,
    setMaxTurns,
    setRagEnabled,
    setVisionModelEnabled,
    setSecondaryRoutingEnabled,
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
  import { Check } from "@lucide/svelte";
  import Select from "./Select.svelte";

  type SelectItem = { value: string; label: string; disabled?: boolean };

  // Form state — local edits, committed to the backend by save().
  let primary = $state("");
  let secondary = $state("");
  let summarization = $state("");
  let routingEnabled = $state(true);
  let embedding = $state("");
  let vision = $state("");
  let ragEnabled = $state(true);
  let visionEnabled = $state(false);
  let activeModels = $state<ModelOption[]>([]);
  let allModels = $state<ModelOption[]>([]);
  let totalChunks = $state(0);
  let maxTurns = $state(50);
  let mtUnlimited = $state(false);
  let mtRestore = $state(50);

  // Pristine snapshots — last values confirmed by the backend config.
  // Used by cancel() to revert local edits and by the derived `dirty` flag.
  let lastPrimary = $state("");
  let lastSecondary = $state("");
  let lastRoutingEnabled = $state(true);
  let lastSummarization = $state("");
  let lastEmbedding = $state("");
  let lastRagEnabled = $state(true);
  let lastVisionEnabled = $state(false);
  let lastVision = $state("");
  let lastMaxTurns = $state(50);

  let saving = $state(false);
  let ragClearing = $state(false);
  let justSaved = $state(false);
  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  const NOT_SELECTED = "";

  function refValue(mr: ModelRef | null): string {
    return mr && mr.provider && mr.model ? `${mr.provider}::${mr.model}` : "";
  }

  function syncFromConfig(cfg: AppConfig | null) {
    if (!cfg) return;
    primary = refValue(cfg.defaults.main_model);
    secondary = refValue(cfg.defaults.secondary_model);
    routingEnabled = cfg.defaults.secondary_routing_enabled;
    summarization = refValue(cfg.defaults.summarization_model);
    embedding = refValue(cfg.defaults.embedding_model);
    vision = refValue(cfg.defaults.vision_model);
    ragEnabled = cfg.defaults.rag_enabled;
    visionEnabled = cfg.defaults.vision_model_enabled;
    maxTurns = cfg.defaults.max_turns ?? 50;
    mtUnlimited = maxTurns === 0;
    mtRestore = maxTurns === 0 ? 50 : maxTurns;
    lastPrimary = primary;
    lastSecondary = secondary;
    lastRoutingEnabled = routingEnabled;
    lastSummarization = summarization;
    lastEmbedding = embedding;
    lastRagEnabled = ragEnabled;
    lastVisionEnabled = visionEnabled;
    lastVision = vision;
    lastMaxTurns = maxTurns;
  }

  // A single dirty flag for the whole page: any local field diverging from
  // its pristine snapshot makes the form editable.
  const dirty = $derived(
    primary !== lastPrimary ||
      secondary !== lastSecondary ||
      routingEnabled !== lastRoutingEnabled ||
      summarization !== lastSummarization ||
      embedding !== lastEmbedding ||
      ragEnabled !== lastRagEnabled ||
      vision !== lastVision ||
      visionEnabled !== lastVisionEnabled ||
      maxTurns !== lastMaxTurns,
  );

  // When there are no models at all and nothing is selected, the model cards
  // are hidden (only the max-turns card and footer remain editable).
  const modelsEmpty = $derived(
    activeModels.length === 0 && !primary && !secondary && !embedding,
  );

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
    ragClearing = true;
    try {
      await ragClearAll();
      await loadRagStatus();
    } catch (e) {
      toast.error(m.rag_cleared_msg(), String(e));
    } finally {
      ragClearing = false;
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
      if (savedTimer) clearTimeout(savedTimer);
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
  const summarizationOptions = $derived<SelectItem[]>([
    { value: NOT_SELECTED, label: m.settings_models_not_selected() },
    ...withCurrent(activeItems, summarization),
  ]);
  const embeddingOptions = $derived<SelectItem[]>([
    { value: NOT_SELECTED, label: m.settings_models_not_selected() },
    ...withCurrent(activeItems, embedding),
  ]);
  const visionOptions = $derived<SelectItem[]>([
    { value: NOT_SELECTED, label: m.settings_models_not_selected() },
    ...withCurrent(activeItems, vision),
  ]);

  function parseValue(v: string): { provider: string; model: string } | null {
    if (!v) return null;
    const idx = v.indexOf("::");
    if (idx < 0) return null;
    return { provider: v.slice(0, idx), model: v.slice(idx + 2) };
  }

  // Save every field in one pass. Each backend setter emits `config:reloaded`,
  // which triggers syncFromConfig and overwrites local state from the in-memory
  // config mid-flight, so all values are captured into consts before any await.
  async function save() {
    if (!modelsEmpty && !primary) {
      toast.error(m.settings_models_primary_required());
      return;
    }
    const p = parseValue(primary);
    const s = parseValue(secondary);
    const summ = parseValue(summarization);
    const e = parseValue(embedding);
    const v = parseValue(vision);
    const routing = routingEnabled;
    const ragOn = ragEnabled;
    const visionOn = visionEnabled;
    const rawTurns = Math.floor(maxTurns);
    const turns = rawTurns === 0 ? 0 : Math.max(1, Math.min(1000, rawTurns));
    saving = true;
    try {
      await setDefaultsModel("main_model", p?.provider ?? null, p?.model ?? null);
      await setDefaultsModel("secondary_model", s?.provider ?? null, s?.model ?? null);
      await setSecondaryRoutingEnabled(routing);
      await setDefaultsModel("summarization_model", summ?.provider ?? null, summ?.model ?? null);
      await setRagEnabled(ragOn);
      await setDefaultsModel("embedding_model", e?.provider ?? null, e?.model ?? null);
      await setVisionModelEnabled(visionOn);
      await setDefaultsModel("vision_model", v?.provider ?? null, v?.model ?? null);
      await setMaxTurns(turns);
      toast.success(m.settings_models_saved());
      justSaved = true;
      if (savedTimer) clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (justSaved = false), 1800);
    } catch (err) {
      toast.error(m.settings_models_save_failed(), String(err));
    } finally {
      saving = false;
    }
  }

  // Revert all local edits back to the last config-confirmed snapshots.
  function cancel() {
    primary = lastPrimary;
    secondary = lastSecondary;
    routingEnabled = lastRoutingEnabled;
    summarization = lastSummarization;
    embedding = lastEmbedding;
    ragEnabled = lastRagEnabled;
    vision = lastVision;
    visionEnabled = lastVisionEnabled;
    maxTurns = lastMaxTurns;
    mtUnlimited = maxTurns === 0;
    mtRestore = maxTurns === 0 ? 50 : maxTurns;
  }

  function toggleMaxTurnsUnlimited(e: Event) {
    const checked = (e.target as HTMLInputElement).checked;
    mtUnlimited = checked;
    if (checked) {
      if (maxTurns !== 0) mtRestore = maxTurns;
      maxTurns = 0;
    } else {
      maxTurns = mtRestore;
    }
  }

  function onMaxTurnsInput(e: Event) {
    maxTurns = Number((e.target as HTMLInputElement).value);
  }
</script>

<div class="models">
  {#if modelsEmpty}
    <div class="empty">{m.settings_models_empty()}</div>
  {:else}
    <section class="card">
      <div class="card-head">
        <span class="card-heading">{m.settings_models_defaults_heading()}</span>
      </div>
      <div class="fields">
        <div class="field">
          <span class="label">{m.settings_models_primary()}</span>
          <Select
            class="w-full"
            value={primary}
            items={primaryItems}
            placeholder={m.settings_models_not_selected()}
            disabled={saving}
            onchange={(v) => (primary = v)}
          />
        </div>
        <div class="field">
          <span class="label">{m.settings_models_secondary()}</span>
          <Select
            class="w-full"
            value={secondary}
            items={secondaryOptions}
            placeholder={m.settings_models_not_selected()}
            disabled={saving}
            onchange={(v) => (secondary = v)}
          />
          <p class="hint">{m.settings_models_secondary_hint()}</p>
        </div>
      </div>

      {#if secondary}
        <label class="toggle">
          <input
            type="checkbox"
            checked={routingEnabled}
            disabled={saving}
            onchange={(e) => (routingEnabled = e.currentTarget.checked)}
          />
          <span class="toggle-label">{m.settings_models_routing()}</span>
        </label>
        <p class="hint">{m.settings_models_routing_hint()}</p>
      {/if}
    </section>

    <section class="card">
      <div class="card-head">
        <span class="card-heading">{m.settings_models_summarization()}</span>
      </div>
      <Select
        class="w-full"
        value={summarization}
        items={summarizationOptions}
        placeholder={m.settings_models_not_selected()}
        disabled={saving}
        onchange={(v) => (summarization = v)}
      />
      <p class="hint">{m.settings_models_summarization_hint()}</p>
    </section>

    <section class="card" class:disabled={!ragEnabled}>
      <div class="card-head">
        <label class="toggle">
          <input type="checkbox" checked={ragEnabled} disabled={saving} onchange={(e) => (ragEnabled = e.currentTarget.checked)} />
          <span class="toggle-label">{m.settings_models_rag_section()}</span>
        </label>
        <span class="toggle-state">{ragEnabled ? m.settings_models_rag_enabled() : ""}</span>
      </div>

      <div class="field">
        <span class="label">{m.settings_models_embedding()}</span>
        <Select
          class="w-full"
          value={embedding}
          items={embeddingOptions}
          placeholder={m.settings_models_not_selected()}
          disabled={saving || !ragEnabled}
          title={ragEnabled ? undefined : m.settings_models_rag_disabled_hint()}
          onchange={(v) => (embedding = v)}
        />
      </div>

      {#if !ragEnabled}
        <p class="hint">{m.settings_models_rag_disabled_hint()}</p>
      {/if}

      <div class="rag-clear">
        <span class="rag-clear-info">
          {totalChunks} {m.rag_chunks()}
          <span class="muted"> · {m.rag_clear_all_hint()}</span>
        </span>
        <button class="btn danger" onclick={clearAllRag} disabled={ragClearing}>
          {ragClearing ? "…" : m.rag_clear_all()}
        </button>
      </div>
    </section>

    <section class="card" class:disabled={!visionEnabled}>
      <div class="card-head">
        <label class="toggle">
          <input type="checkbox" checked={visionEnabled} disabled={saving} onchange={(e) => (visionEnabled = e.currentTarget.checked)} />
          <span class="toggle-label">{m.settings_models_vision_section()}</span>
        </label>
        <span class="toggle-state">{visionEnabled ? m.settings_models_vision_enabled() : ""}</span>
      </div>

      <div class="field">
        <span class="label">{m.settings_models_vision_model()}</span>
        <Select
          class="w-full"
          value={vision}
          items={visionOptions}
          placeholder={m.settings_models_not_selected()}
          disabled={saving || !visionEnabled}
          title={visionEnabled ? undefined : m.settings_models_vision_disabled_hint()}
          onchange={(v) => (vision = v)}
        />
      </div>

      {#if !visionEnabled}
        <p class="hint">{m.settings_models_vision_disabled_hint()}</p>
      {/if}
    </section>
  {/if}

  <section class="card">
    <div class="card-head">
      <span class="card-heading">{m.settings_models_max_turns()}</span>
    </div>
    <div class="max-turns-row">
      <input
        class="max-turns-input"
        type="number"
        min="1"
        max="1000"
        step="1"
        value={maxTurns}
        disabled={saving || mtUnlimited}
        oninput={onMaxTurnsInput}
      />
      <label class="max-turns-unlimited">
        <input
          type="checkbox"
          checked={mtUnlimited}
          disabled={saving}
          onchange={toggleMaxTurnsUnlimited}
        />
        <span>{m.settings_models_max_turns_unlimited()}</span>
      </label>
    </div>
    <p class="hint">{m.settings_models_max_turns_hint()}</p>
  </section>

  <div class="footer">
    {#if justSaved}<span class="saved-check" title={m.settings_models_saved()}><Check size={12} /></span>{/if}
    <button class="btn" onclick={cancel} disabled={!dirty || saving}>
      {m.common_cancel()}
    </button>
    <button class="btn primary" onclick={() => void save()} disabled={!dirty || saving}>
      {saving ? "…" : m.common_save()}
    </button>
  </div>
</div>

<style>
  .models {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    min-height: 100%;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 0.75rem 0.875rem;
  }
  .card.disabled {
    opacity: 0.65;
  }
  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .card-heading {
    font-size: 0.8125rem;
    font-weight: 600;
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
  .hint {
    margin: 0;
    font-size: 0.72rem;
    color: var(--muted-foreground);
    line-height: 1.35;
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8125rem;
    font-weight: 600;
    cursor: default;
  }
  .toggle input {
    width: 0.95rem;
    height: 0.95rem;
    cursor: default;
  }
  .toggle-state {
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }
  .max-turns-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
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
  .max-turns-input:disabled {
    opacity: 0.5;
  }
  .max-turns-unlimited {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8125rem;
    cursor: default;
  }
  .max-turns-unlimited input {
    width: 0.95rem;
    height: 0.95rem;
    cursor: default;
  }
  .rag-clear {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding-top: 0.4rem;
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
  .empty {
    color: var(--muted-foreground);
    font-size: 0.8125rem;
    padding: 0.5rem 0;
  }
  .footer {
    position: sticky;
    bottom: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: auto;
    padding: 0.6rem 0 0.25rem;
    background: var(--background);
    border-top: 1px solid var(--border);
  }
  .saved-check {
    margin-right: auto;
    display: inline-flex;
    color: hsl(142 71% 45%);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.35rem 0.85rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    cursor: default;
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .btn:hover:not(:disabled) {
    background: var(--accent);
  }
  .btn.primary {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover:not(:disabled) {
    opacity: 0.9;
    background: var(--primary);
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