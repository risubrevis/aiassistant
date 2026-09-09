<script lang="ts">
  import { onMount } from "svelte";
  import {
    Plus,
    Trash2,
    Pencil,
    X,
    ArrowUp,
    ArrowDown,
    RefreshCw,
    Check,
    Brain,
  } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { toast } from "$lib/stores/toasts";
  import {
    providersList,
    providersCreate,
    providersUpdate,
    providersDelete,
    providersReorder,
    providersSetActive,
    providerModelsList,
    providerModelsFetch,
    providerModelsSave,
    providerStatus,
    onProvidersChanged,
    type ProviderRow,
    type ProviderInput,
    type ModelInfo,
  } from "$lib/tauri";
  import Select from "./Select.svelte";

  const kindItems: { value: string; label: string }[] = [
    { value: "custom", label: "custom" },
    { value: "openai", label: "openai" },
    { value: "anthropic", label: "anthropic" },
    { value: "ollama", label: "ollama" },
    { value: "lm-studio-desktop", label: "LM Studio (Desktop)" },
    { value: "lm-studio-server", label: "LM Studio (Server)" },
  ];

  const kindDefaults: Record<string, { name: string; base_url: string; timeout: number }> = {
    openai: { name: "OpenAI", base_url: "https://api.openai.com/v1", timeout: 30_000 },
    anthropic: { name: "Anthropic", base_url: "https://api.anthropic.com", timeout: 30_000 },
    ollama: { name: "Ollama (local)", base_url: "http://localhost:11434/v1", timeout: 30_000 },
    "lm-studio-desktop": { name: "LM Studio (Desktop)", base_url: "http://localhost:1234/v1", timeout: 120_000 },
    "lm-studio-server": { name: "LM Studio (Server)", base_url: "http://localhost:1234/v1", timeout: 120_000 },
  };

  function onKindChange(value: string) {
    formKind = value;
    const d = kindDefaults[value];
    if (!d) return;
    formName = d.name;
    formBaseUrl = d.base_url;
    formTimeout = d.timeout;
    modalError = "";
    testInfo = "";
  }

  type Row = {
    name: string;
    display_name: string;
    enabled: boolean;
    alias: string;
    capabilities: string[];
    context_window: number;
  };

  let providers = $state<ProviderRow[]>([]);
  let statuses = $state<Record<string, string>>({});
  let error = $state("");

  let modalOpen = $state(false);
  let editingId = $state<string | null>(null);
  let deleteId = $state<string | null>(null);
  let modalError = $state("");
  let testInfo = $state("");

  let formKind = $state("openai");
  let formName = $state("");
  let formBaseUrl = $state("");
  let formApiKey = $state("");
  let formTimeout = $state(30000);
  let formActive = $state(true);
  let editingApiKeyRef = $state("");
  let editingHeaders = $state<Record<string, string>>({});

  let testing = $state(false);
  let saving = $state(false);

  let modelsModalOpen = $state(false);
  let modelsProviderId = $state<string | null>(null);
  let modelsProviderName = $state("");
  let modelRows = $state<Row[]>([]);
  let modelsSaving = $state(false);
  let modelsError = $state("");
  let modelsTesting = $state(false);
  let modelsTestInfo = $state("");

  let canSaveProvider = $derived(formName.trim().length > 0 && formBaseUrl.trim().length > 0);
  let modalTitle = $derived(
    editingId === null ? m.settings_providers_add() : m.settings_providers_edit(),
  );

  onMount(() => {
    void load();
    let unlisten: (() => void) | undefined;
    void onProvidersChanged(() => void load()).then((u) => (unlisten = u));
    return () => unlisten?.();
  });

  async function load() {
    try {
      providers = await providersList();
      error = "";
      await refreshStatuses();
    } catch (e) {
      error = String(e);
    }
  }

  async function refreshStatuses() {
    const next: Record<string, string> = {};
    for (const p of providers) {
      if (!p.is_active) {
        next[p.id] = "inactive";
        continue;
      }
      try {
        const st = await providerStatus(p.id);
        next[p.id] = st.state;
      } catch {
        next[p.id] = "unreachable";
      }
    }
    statuses = next;
  }

  function dotClass(p: ProviderRow): string {
    return statuses[p.id] ?? "unknown";
  }

  function statusTitle(p: ProviderRow): string {
    const st = statuses[p.id];
    if (st === "active") return "Active";
    if (st === "inactive") return "Inactive";
    if (st === "unreachable") return "Unreachable";
    return "Checking…";
  }

  function formInput(): ProviderInput {
    return {
      name: formName.trim(),
      kind: formKind,
      base_url: formBaseUrl.trim(),
      api_key_ref: editingApiKeyRef,
      extra_headers: editingHeaders,
      timeout_ms: formTimeout,
      is_active: formActive,
    };
  }

  function openAdd() {
    editingId = null;
    formKind = "custom";
    formName = "";
    formBaseUrl = "";
    formApiKey = "";
    formTimeout = 30000;
    formActive = true;
    editingApiKeyRef = "";
    editingHeaders = {};
    modalError = "";
    testInfo = "";
    modalOpen = true;
  }

  function openEdit(p: ProviderRow) {
    editingId = p.id;
    formKind = p.kind;
    formName = p.name;
    formBaseUrl = p.base_url;
    formApiKey = "";
    formTimeout = p.timeout_ms;
    formActive = p.is_active;
    editingApiKeyRef = p.api_key_ref;
    editingHeaders = p.extra_headers ?? {};
    modalError = "";
    testInfo = "";
    modalOpen = true;
  }

  async function testAndFetch() {
    if (!canSaveProvider) {
      modalError = "Name and Base URL are required";
      return;
    }
    testing = true;
    modalError = "";
    testInfo = "";
    try {
      let id = editingId;
      if (!id) {
        // Persist first so the backend can reach the endpoint with the current key.
        const created = await providersCreate(formInput(), formApiKey || undefined);
        id = created.id;
        editingId = id;
        editingApiKeyRef = created.api_key_ref;
        formApiKey = "";
      }
      const fetched = await providerModelsFetch(id);
      const existing = await providerModelsList(id);
      const byName = new Map(existing.map((pm) => [pm.name, pm]));
      const beforeNames = new Set(existing.map((pm) => pm.name));
      const fetchedNames = new Set(fetched.map((mm: ModelInfo) => mm.name));
      const added = [...fetchedNames].filter((n) => !beforeNames.has(n)).length;
      const removed = [...beforeNames].filter((n) => !fetchedNames.has(n)).length;
      const merged = fetched.map((mm: ModelInfo) => {
        const ex = byName.get(mm.name);
        return {
          name: mm.name,
          display_name: ex?.display_name || mm.name,
          enabled: ex?.enabled ?? true,
          alias: ex?.alias ?? "",
          capabilities: ex?.capabilities ?? [],
          context_window: mm.context_window ?? ex?.context_window ?? 0,
        };
      });
      await providerModelsSave(
        id,
        merged.map((r) => ({
          name: r.name,
          display_name: r.display_name.trim() || r.name,
          enabled: r.enabled,
          alias: r.alias,
          capabilities: r.capabilities,
          context_window: r.context_window,
        })),
      );
      const summary = `OK — ${merged.length} model(s)${
        added || removed ? ` (+${added} new, −${removed} removed)` : ""
      }`;
      testInfo = summary;
      toast.success(m.settings_providers_fetch_done(), summary);
      void load();
    } catch (e) {
      modalError = String(e);
    } finally {
      testing = false;
    }
  }

  async function saveProvider() {
    if (!canSaveProvider) {
      modalError = "Name and Base URL are required";
      return;
    }
    saving = true;
    modalError = "";
    try {
      let id = editingId;
      if (!id) {
        const created = await providersCreate(formInput(), formApiKey || undefined);
        id = created.id;
      } else {
        await providersUpdate(id, formInput(), formApiKey || undefined);
      }
      modalOpen = false;
      await load();
    } catch (e) {
      modalError = String(e);
    } finally {
      saving = false;
    }
  }

  async function toggleActive(p: ProviderRow, active: boolean) {
    try {
      await providersSetActive(p.id, active);
    } catch (e) {
      error = String(e);
    }
    await load();
  }

  async function move(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= providers.length) return;
    const next = [...providers];
    [next[i], next[j]] = [next[j], next[i]];
    providers = next;
    try {
      await providersReorder(next.map((p) => p.id));
      error = "";
    } catch (e) {
      error = String(e);
    }
    await load();
  }

  function openDelete(p: ProviderRow) {
    deleteId = p.id;
    modalError = "";
  }

  async function confirmDelete() {
    if (!deleteId) return;
    try {
      await providersDelete(deleteId);
      deleteId = null;
      await load();
    } catch (e) {
      modalError = String(e);
    }
  }

  function closeModals() {
    modalOpen = false;
    modelsModalOpen = false;
    deleteId = null;
  }

  function openModels(p: ProviderRow) {
    modelsProviderId = p.id;
    modelsProviderName = p.name;
    modelRows = [];
    modelsError = "";
    modelsTestInfo = "";
    modelsModalOpen = true;
    void loadModelRows(p.id);
  }

  async function loadModelRows(id: string) {
    try {
      const list = await providerModelsList(id);
      modelRows = list.map((pm) => ({
        name: pm.name,
        display_name: pm.display_name || pm.name,
        enabled: pm.enabled,
        alias: pm.alias,
        capabilities: pm.capabilities ?? [],
        context_window: pm.context_window ?? 0,
      }));
    } catch (e) {
      modelsError = String(e);
    }
  }

  async function saveModels() {
    if (!modelsProviderId) return;
    modelsSaving = true;
    modelsError = "";
    try {
      await providerModelsSave(
        modelsProviderId,
        modelRows.map((r) => ({
          name: r.name,
          display_name: r.display_name.trim() || r.name,
          enabled: r.enabled,
          alias: r.alias,
          capabilities: r.capabilities,
          context_window: r.context_window,
        })),
      );
      modelsModalOpen = false;
      toast.success(m.settings_providers_models_saved());
      void load();
    } catch (e) {
      modelsError = String(e);
      toast.error(m.settings_providers_models_save_failed(), String(e));
    } finally {
      modelsSaving = false;
    }
  }

  async function fetchModels() {
    if (!modelsProviderId) return;
    modelsTesting = true;
    modelsError = "";
    modelsTestInfo = "";
    try {
      const fetched = await providerModelsFetch(modelsProviderId);
      const byName = new Map(modelRows.map((r) => [r.name, r]));
      const beforeNames = new Set(modelRows.map((r) => r.name));
      const fetchedNames = new Set(fetched.map((mm: ModelInfo) => mm.name));
      const added = [...fetchedNames].filter((n) => !beforeNames.has(n)).length;
      const removed = [...beforeNames].filter((n) => !fetchedNames.has(n)).length;
      modelRows = fetched.map((mm: ModelInfo) => {
        const ex = byName.get(mm.name);
        return {
          name: mm.name,
          display_name: ex?.display_name || mm.name,
          enabled: ex?.enabled ?? true,
          alias: ex?.alias ?? "",
          capabilities: ex?.capabilities ?? [],
          context_window: mm.context_window ?? ex?.context_window ?? 0,
        };
      });
      const summary = `OK — ${modelRows.length} model(s)${
        added || removed ? ` (+${added} new, −${removed} removed)` : ""
      }`;
      modelsTestInfo = summary;
      toast.success(m.settings_providers_fetch_done(), summary);
    } catch (e) {
      modelsError = String(e);
    } finally {
      modelsTesting = false;
    }
  }

  function closeModelsModal() {
    modelsModalOpen = false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    closeModals();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="prompts">
  <section class="section">
    <div class="sec-head">
      <span class="sec-title">{m.settings_tab_providers()}</span>
      <span class="sec-actions">
        <button class="icon-btn" title="Refresh" onclick={() => void load()}>
          <RefreshCw size={13} />
        </button>
        <button class="btn" onclick={openAdd}>
          <Plus size={13} /> {m.settings_providers_add()}
        </button>
      </span>
    </div>
    {#if providers.length === 0}
      <div class="empty">{m.settings_providers_empty()}</div>
    {:else}
      <div class="rules">
        {#each providers as p, i (p.id)}
          <div class="rule-row">
            <span class="dot {dotClass(p)}" title={statusTitle(p)}></span>
            <div class="rule-main">
              <span class="rule-title">{p.name}</span>
              <span class="rule-preview" title={p.base_url}>{p.base_url}</span>
            </div>
            <input
              class="toggle"
              type="checkbox"
              title="Active"
              checked={p.is_active}
              onchange={(e) => void toggleActive(p, e.currentTarget.checked)}
            />
            <button class="icon-btn" disabled={i === 0} onclick={() => void move(i, -1)}>
              <ArrowUp size={13} />
            </button>
            <button
              class="icon-btn"
              disabled={i === providers.length - 1}
              onclick={() => void move(i, 1)}
            >
              <ArrowDown size={13} />
            </button>
            <button
              class="icon-btn"
              title={m.settings_providers_models_title()}
              onclick={() => openModels(p)}
            >
              <Brain size={13} />
            </button>
            <button class="icon-btn" title={m.common_edit()} onclick={() => openEdit(p)}>
              <Pencil size={13} />
            </button>
            <button class="icon-btn danger" title={m.common_delete()} onclick={() => openDelete(p)}>
              <Trash2 size={13} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
    {#if error}<div class="err">{error}</div>{/if}
  </section>
</div>

{#if modalOpen}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog wide"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">
        <span class="head-title">{modalTitle}</span>
        <button class="x" title={m.common_close()} onclick={closeModals}><X size={14} /></button>
      </header>
      <div class="body">
        <div class="field">
          <span class="lbl">Kind</span>
          <Select value={formKind} items={kindItems} onchange={onKindChange} />
        </div>
        <label class="field">
          <span class="lbl">Name</span>
          <input bind:value={formName} placeholder="Ollama (local)" spellcheck="false" />
        </label>
        <label class="field">
          <span class="lbl">Base URL</span>
          <input
            bind:value={formBaseUrl}
            placeholder="http://localhost:11434/v1"
            spellcheck="false"
          />
        </label>
        <label class="field">
          <span class="lbl">API key (stored in keyring)</span>
          <input
            type="password"
            bind:value={formApiKey}
            placeholder={editingApiKeyRef ? "••••••  (enter to update)" : "optional"}
            autocomplete="off"
          />
        </label>
        <div class="grid2">
          <label class="field">
            <span class="lbl">Timeout (ms)</span>
            <input type="number" bind:value={formTimeout} />
          </label>
          <label class="field check">
            <span class="lbl">Active</span>
            <input class="toggle" type="checkbox" bind:checked={formActive} />
          </label>
        </div>

        <button class="btn" onclick={() => void testAndFetch()} disabled={testing}>
          <RefreshCw size={12} /> {testing ? "Testing…" : "Test and fetch available models"}
        </button>
        {#if testInfo}<div class="ok"><Check size={12} /> {testInfo}</div>{/if}

        {#if modalError}<div class="err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={closeModals}>{m.common_cancel()}</button>
        <button
          class="btn primary"
          disabled={!canSaveProvider || saving}
          onclick={() => void saveProvider()}
        >
          {saving ? "Saving…" : m.common_save()}
        </button>
      </footer>
    </div>
  </div>
{/if}

{#if modelsModalOpen}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog wide"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">
        <span class="head-title"
          >{m.settings_providers_models_title()} — {modelsProviderName}</span
        >
        <button class="x" title={m.common_close()} onclick={closeModelsModal}><X size={14} /></button>
      </header>
      <div class="body">
        <button class="btn" onclick={() => void fetchModels()} disabled={modelsTesting}>
          <RefreshCw size={12} /> {modelsTesting ? "Testing…" : "Test and fetch available models"}
        </button>
        {#if modelsTestInfo}<div class="ok"><Check size={12} /> {modelsTestInfo}</div>{/if}
        {#if modelRows.length}
          <div class="table">
            <div class="th">
              <span>API name</span>
              <span>Display name</span>
              <span class="c">Visible</span>
            </div>
            {#each modelRows as r (r.name)}
              <div class="tr">
                <span class="api">{r.name}</span>
                <input bind:value={r.display_name} />
                <input class="c" type="checkbox" bind:checked={r.enabled} />
              </div>
            {/each}
          </div>
        {:else}
          <div class="empty">{m.settings_providers_empty()}</div>
        {/if}
        {#if modelsError}<div class="err">{modelsError}</div>{/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={closeModelsModal}>{m.common_cancel()}</button>
        <button
          class="btn primary"
          disabled={!modelRows.length || modelsSaving}
          onclick={() => void saveModels()}
        >
          {modelsSaving ? "Saving…" : m.common_save()}
        </button>
      </footer>
    </div>
  </div>
{/if}

{#if deleteId !== null}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog small"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <div class="body">
        <div class="confirm">{m.settings_providers_delete_confirm()}</div>
        {#if modalError}<div class="err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={closeModals}>{m.common_cancel()}</button>
        <button class="btn danger" onclick={() => void confirmDelete()}>
          {m.common_delete()}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .prompts {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }
  .sec-title {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .sec-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    column-gap: 0.75rem;
  }
  .sec-actions {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .toggle {
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    accent-color: var(--primary);
    cursor: default;
  }
  .status {
    min-height: 0.95rem;
    font-size: 0.7rem;
  }
  .err {
    font-size: 0.72rem;
    color: var(--destructive);
    overflow-wrap: anywhere;
  }
  .ok {
    color: hsl(140 60% 40%);
    font-size: 0.75rem;
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }
  input {
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-sans);
    outline: none;
  }
  input:focus {
    border-color: var(--ring);
  }
  .rules {
    display: grid;
    grid-template-columns: 1fr;
    gap: 0.4rem;
  }
  .rule-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 9999px;
    flex-shrink: 0;
    background: var(--muted-foreground);
    opacity: 0.5;
  }
  .dot.active {
    background: hsl(140 55% 45%);
    opacity: 1;
  }
  .dot.inactive {
    background: hsl(38 90% 50%);
    opacity: 1;
  }
  .dot.unreachable {
    background: var(--destructive);
    opacity: 1;
  }
  .rule-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .rule-title {
    font-size: 0.8125rem;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rule-preview {
    font-size: 0.72rem;
    color: var(--muted-foreground);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty {
    color: var(--muted-foreground);
    font-size: 0.78rem;
    padding: 0.25rem 0;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.28rem 0.7rem;
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
  .btn.primary {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover {
    filter: brightness(1.08);
  }
  .btn.primary:disabled,
  .btn.primary:disabled:hover {
    opacity: 0.5;
    filter: none;
    background: var(--primary);
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .btn.danger {
    background: var(--destructive);
    border-color: var(--destructive);
    color: var(--destructive-foreground);
  }
  .btn.danger:hover {
    filter: brightness(0.92);
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 1.5rem;
    width: 1.5rem;
    flex-shrink: 0;
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
  .icon-btn.danger:hover {
    color: var(--destructive);
  }
  .icon-btn:disabled {
    opacity: 0.4;
  }
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
  }
  .dialog.wide {
    width: 640px;
    max-width: 92vw;
  }
  .dialog.small {
    width: 360px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
  }
  .head-title {
    font-size: 0.875rem;
    font-weight: 500;
  }
  .x {
    display: inline-flex;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .x:hover {
    background: var(--accent);
  }
  .body {
    flex: 1;
    min-height: 0;
    padding: 0.875rem 1rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .confirm {
    font-size: 0.8125rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
  }
  .field.check {
    flex-direction: row;
    align-items: center;
    gap: 0.5rem;
  }
  .lbl {
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem;
    align-items: end;
  }
  .table {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    max-height: 300px;
    overflow-y: auto;
  }
  .th,
  .tr {
    display: grid;
    grid-template-columns: 1.4fr 1.4fr 60px;
    gap: 0.5rem;
    align-items: center;
    padding: 0.3rem 0.5rem;
  }
  .th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--muted);
    font-size: 0.7rem;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .tr {
    border-top: 1px solid var(--border);
    font-size: 0.8125rem;
  }
  .api {
    color: var(--muted-foreground);
    font-family: var(--font-mono);
    font-size: 0.75rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .c {
    text-align: center;
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
</style>