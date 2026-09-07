<script lang="ts">
  import { onMount } from "svelte";
  import { Plus, Trash2, Pencil, X, ArrowUp, ArrowDown } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { config } from "$lib/stores/config";
  import { toast } from "$lib/stores/toasts";
  import {
    configGet,
    setWebSearch,
    webSearchProvidersList,
    webSearchProvidersSave,
    webSearchProviderSetKey,
    webSearchProviderClearKey,
    webSearchProviderHasKey,
    type WebSearchConfig,
    type WebSearchProvider,
    type WebSearchProviderKind,
  } from "$lib/tauri";
  import Select from "./Select.svelte";

  let providers = $state<WebSearchProvider[]>([]);
  let saved = $state(false);
  let error = $state("");

  let modalOpen = $state(false);
  let editingIndex = $state<number | null>(null);
  let deleteIndex = $state<number | null>(null);
  let modalError = $state("");
  let formTitle = $state("");
  let formUrl = $state("");
  let formKind = $state<WebSearchProviderKind>("scrape");
  let formMethod = $state("GET");
  let formAuthScheme = $state("none");
  let formAuthHeader = $state("");
  let formBodyTemplate = $state("");
  let formResultsPath = $state("");
  let formTitleField = $state("");
  let formUrlField = $state("");
  let formSnippetField = $state("");
  let formApiKey = $state("");
  let formHasKey = $state(false);
  let keyFlash = $state("");

  let savedTimer: ReturnType<typeof setTimeout> | undefined;
  let keyTimer: ReturnType<typeof setTimeout> | undefined;

  const DEFAULT_UA =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36";

  const PRESET_ENDPOINTS: Partial<Record<WebSearchProviderKind, string>> = {
    brave_api: "https://api.search.brave.com/res/v1/web/search?q={query}&count={count}",
    tavily_api: "https://api.tavily.com/search",
    serper_api: "https://google.serper.dev/search",
    exa_api: "https://api.exa.ai/search",
  };

  const BLANK_API_FIELDS = {
    api_method: "",
    auth_scheme: "",
    auth_header: "",
    body_template: "",
    results_path: "",
    title_field: "",
    url_field: "",
    snippet_field: "",
  };

  const kindItems = [
    { value: "scrape", label: m.settings_web_search_kind_scrape() },
    { value: "brave_api", label: m.settings_web_search_kind_brave_api() },
    { value: "tavily_api", label: m.settings_web_search_kind_tavily_api() },
    { value: "serper_api", label: m.settings_web_search_kind_serper_api() },
    { value: "exa_api", label: m.settings_web_search_kind_exa_api() },
    { value: "custom_api", label: m.settings_web_search_kind_custom_api() },
  ];

  const methodItems = [
    { value: "GET", label: "GET" },
    { value: "POST", label: "POST" },
  ];

  const authItems = [
    { value: "none", label: m.settings_web_search_auth_none() },
    { value: "header", label: m.settings_web_search_auth_header() },
    { value: "bearer", label: m.settings_web_search_auth_bearer() },
  ];

  const isPresetKind = (kind: WebSearchProviderKind): boolean =>
    kind === "brave_api" || kind === "tavily_api" || kind === "serper_api" || kind === "exa_api";

  function wsDefaults(): WebSearchConfig {
    return {
      user_agent: DEFAULT_UA,
      accept_language: "en-US,en;q=0.9",
      extra_headers: "",
      timeout_ms: 20000,
    };
  }

  let wsForm = $state<WebSearchConfig>(wsDefaults());
  let wsSaved = $state<WebSearchConfig>(wsDefaults());
  let wsSaving = $state(false);
  let wsSavedFlash = $state(false);
  let wsError = $state("");

  let wsSavedTimer: ReturnType<typeof setTimeout> | undefined;

  let wsDirty = $derived(
    wsForm.user_agent !== wsSaved.user_agent ||
      wsForm.accept_language !== wsSaved.accept_language ||
      wsForm.extra_headers !== wsSaved.extra_headers ||
      wsForm.timeout_ms !== wsSaved.timeout_ms,
  );

  let canSaveProvider = $derived(
    formTitle.trim().length > 0 &&
      (isPresetKind(formKind) ||
        (formUrl.includes("{query}") && /^https?:\/\//i.test(formUrl.trim()))),
  );
  let modalTitle = $derived(
    editingIndex === null
      ? m.settings_web_search_add()
      : m.settings_web_search_edit(),
  );

  onMount(() => {
    void load();
    return () => {
      clearTimeout(savedTimer);
      clearTimeout(wsSavedTimer);
      clearTimeout(keyTimer);
    };
  });

  async function reloadProviders() {
    try {
      providers = await webSearchProvidersList();
    } catch (e) {
      error = String(e);
    }
  }

  async function load() {
    await reloadProviders();
    try {
      const cfg = $config ?? (await configGet());
      const ws = cfg.web_search ?? wsDefaults();
      wsForm = { ...ws };
      wsSaved = { ...ws };
    } catch (e) {
      wsError = String(e);
    }
  }

  async function saveWs() {
    wsSaving = true;
    try {
      await setWebSearch(wsForm);
      wsSaved = { ...wsForm };
      wsError = "";
      wsSavedFlash = true;
      clearTimeout(wsSavedTimer);
      wsSavedTimer = setTimeout(() => (wsSavedFlash = false), 1500);
      toast.success(m.settings_web_search_saved());
    } catch (e) {
      wsError = String(e);
      toast.error(m.settings_web_search_save_failed(), String(e));
    } finally {
      wsSaving = false;
    }
  }

  function cancelWs() {
    wsForm = { ...wsSaved };
    wsError = "";
  }

  function resetWsDefaults() {
    wsForm = wsDefaults();
  }

  // Replace-all save; reload afterwards to pick up fresh server-side ids.
  async function saveList(list: WebSearchProvider[]): Promise<string | null> {
    try {
      await webSearchProvidersSave(
        list.map((p) => ({
          id: p.id,
          title: p.title,
          url: p.url,
          enabled: p.enabled,
          kind: p.kind,
          api_method: p.api_method,
          auth_scheme: p.auth_scheme,
          auth_header: p.auth_header,
          body_template: p.body_template,
          results_path: p.results_path,
          title_field: p.title_field,
          url_field: p.url_field,
          snippet_field: p.snippet_field,
        })),
      );
      providers = await webSearchProvidersList();
      error = "";
      saved = true;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (saved = false), 1500);
      return null;
    } catch (e) {
      saved = false;
      return String(e);
    }
  }

  async function save() {
    error = (await saveList(providers)) ?? "";
  }

  function moveUp(i: number) {
    if (i <= 0) return;
    [providers[i - 1], providers[i]] = [providers[i], providers[i - 1]];
    void save();
  }

  function moveDown(i: number) {
    if (i >= providers.length - 1) return;
    [providers[i + 1], providers[i]] = [providers[i], providers[i + 1]];
    void save();
  }

  function resetApiForm() {
    formMethod = "GET";
    formAuthScheme = "none";
    formAuthHeader = "";
    formBodyTemplate = "";
    formResultsPath = "";
    formTitleField = "";
    formUrlField = "";
    formSnippetField = "";
  }

  function onKindChange(kind: string) {
    const k = kind as WebSearchProviderKind;
    formKind = k;
    if (isPresetKind(k)) {
      formUrl = PRESET_ENDPOINTS[k] ?? "";
      resetApiForm();
    }
  }

  function openAdd() {
    editingIndex = null;
    formTitle = "";
    formKind = "scrape";
    formUrl = "";
    resetApiForm();
    formApiKey = "";
    formHasKey = false;
    keyFlash = "";
    modalError = "";
    modalOpen = true;
  }

  function openEdit(index: number) {
    const p = providers[index];
    editingIndex = index;
    formTitle = p.title;
    formKind = p.kind;
    formUrl = p.url;
    formMethod = p.api_method === "POST" ? "POST" : "GET";
    formAuthScheme =
      p.auth_scheme === "header" || p.auth_scheme === "bearer" ? p.auth_scheme : "none";
    formAuthHeader = p.auth_header;
    formBodyTemplate = p.body_template;
    formResultsPath = p.results_path;
    formTitleField = p.title_field;
    formUrlField = p.url_field;
    formSnippetField = p.snippet_field;
    formApiKey = "";
    formHasKey = p.has_key;
    keyFlash = "";
    modalError = "";
    modalOpen = true;
    void webSearchProviderHasKey(p.id)
      .then((v) => (formHasKey = v))
      .catch(() => (formHasKey = p.has_key));
  }

  function openDelete(index: number) {
    deleteIndex = index;
    modalError = "";
  }

  function closeModals() {
    modalOpen = false;
    deleteIndex = null;
  }

  function editingId(): string {
    return editingIndex === null ? "" : (providers[editingIndex]?.id ?? "");
  }

  async function saveProviderKey() {
    const id = editingId();
    if (!id || !formApiKey.trim()) return;
    try {
      await webSearchProviderSetKey(id, formApiKey.trim());
      formApiKey = "";
      formHasKey = true;
      keyFlash = "";
      modalError = "";
      void reloadProviders();
    } catch (e) {
      modalError = String(e);
    }
  }

  async function clearProviderKey() {
    const id = editingId();
    if (!id) return;
    try {
      await webSearchProviderClearKey(id);
      formApiKey = "";
      formHasKey = false;
      modalError = "";
      keyFlash = m.settings_web_search_api_key_cleared();
      clearTimeout(keyTimer);
      keyTimer = setTimeout(() => (keyFlash = ""), 1500);
      void reloadProviders();
    } catch (e) {
      modalError = String(e);
    }
  }

  async function saveProvider() {
    const title = formTitle.trim();
    if (!title || !canSaveProvider) return;
    const url = isPresetKind(formKind) ? (PRESET_ENDPOINTS[formKind] ?? "") : formUrl.trim();
    const custom =
      formKind === "custom_api"
        ? {
            api_method: formMethod,
            auth_scheme: formAuthScheme,
            auth_header: formAuthScheme === "header" ? formAuthHeader.trim() : "",
            body_template: formBodyTemplate,
            results_path: formResultsPath.trim(),
            title_field: formTitleField.trim(),
            url_field: formUrlField.trim(),
            snippet_field: formSnippetField.trim(),
          }
        : BLANK_API_FIELDS;
    const updated = [...providers];
    if (editingIndex === null) {
      updated.push({
        id: "",
        title,
        url,
        enabled: true,
        position: updated.length,
        kind: formKind,
        ...custom,
        has_key: false,
      });
    } else {
      updated[editingIndex] = {
        ...providers[editingIndex],
        title,
        url,
        kind: formKind,
        ...custom,
      };
    }
    const err = await saveList(updated);
    if (err) {
      modalError = err;
    } else {
      modalError = "";
      modalOpen = false;
    }
  }

  async function confirmDelete() {
    if (deleteIndex === null) return;
    const updated = providers.filter((_, i) => i !== deleteIndex);
    const err = await saveList(updated);
    if (err) {
      modalError = err;
    } else {
      modalError = "";
      deleteIndex = null;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    closeModals();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="prompts">
  <section class="section">
    <span class="sec-title">{m.settings_web_search_request_title()}</span>
    <div class="hint">{m.settings_web_search_request_hint()}</div>

    <label class="field">
      <span class="lbl">{m.settings_web_search_field_user_agent()}</span>
      <input
        class="mono"
        bind:value={wsForm.user_agent}
        spellcheck="false"
        autocomplete="off"
      />
    </label>

    <label class="field">
      <span class="lbl">{m.settings_web_search_field_accept_language()}</span>
      <input bind:value={wsForm.accept_language} spellcheck="false" autocomplete="off" />
    </label>

    <label class="field">
      <span class="lbl">{m.settings_web_search_field_extra_headers()}</span>
      <textarea class="mono" bind:value={wsForm.extra_headers} rows="3" spellcheck="false"></textarea>
      <span class="hint">{m.settings_web_search_field_extra_headers_hint()}</span>
    </label>

    <label class="field">
      <span class="lbl">{m.settings_web_search_field_timeout()}</span>
      <input
        type="number"
        min="1000"
        value={wsForm.timeout_ms}
        oninput={(e) => (wsForm.timeout_ms = Number(e.currentTarget.value))}
      />
    </label>

    <div class="actions">
      <button class="btn primary" disabled={!wsDirty || wsSaving} onclick={() => void saveWs()}>
        {m.common_save()}
      </button>
      <button class="btn" disabled={!wsDirty || wsSaving} onclick={cancelWs}>
        {m.common_cancel()}
      </button>
      <button class="btn" disabled={wsSaving} onclick={resetWsDefaults}>
        {m.settings_web_search_reset_defaults()}
      </button>
    </div>

    <div class="status">
      {#if wsSavedFlash}<span class="saved">{m.settings_web_search_saved()}</span>
      {:else if wsError}<span class="err">{wsError}</span>{/if}
    </div>
  </section>

  <section class="section">
    <div class="sec-head">
      <span class="sec-title">{m.settings_web_search_title()}</span>
      <button class="btn" onclick={openAdd}>
        <Plus size={13} /> {m.settings_web_search_add()}
      </button>
    </div>
    <div class="hint">{m.settings_web_search_hint({ query: "{query}" })}</div>
    {#if providers.length === 0}
      <div class="empty">{m.settings_web_search_empty()}</div>
    {:else}
      <div class="rules">
        {#each providers as provider, i (i)}
          <div class="rule-row">
            <input
              class="toggle"
              type="checkbox"
              checked={provider.enabled}
              onchange={(e) => {
                providers[i].enabled = e.currentTarget.checked;
                void save();
              }}
            />
            <div class="rule-main">
              <span class="rule-title">
                {provider.title}
                {#if provider.kind === "scrape"}
                  <span class="badge">Scrape</span>
                {:else}
                  <span class="badge">API</span>
                  {#if provider.has_key}<span class="badge">Key</span>{/if}
                {/if}
              </span>
              <span class="rule-preview" title={provider.url}>{provider.url}</span>
            </div>
            <button class="icon-btn" disabled={i === 0} onclick={() => moveUp(i)}>
              <ArrowUp size={13} />
            </button>
            <button
              class="icon-btn"
              disabled={i === providers.length - 1}
              onclick={() => moveDown(i)}
            >
              <ArrowDown size={13} />
            </button>
            <button class="icon-btn" title={m.common_edit()} onclick={() => openEdit(i)}>
              <Pencil size={13} />
            </button>
            <button class="icon-btn danger" title={m.common_delete()} onclick={() => openDelete(i)}>
              <Trash2 size={13} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
    <div class="status">
      {#if saved}<span class="saved">{m.settings_web_search_saved()}</span>
      {:else if error}<span class="err">{error}</span>{/if}
    </div>
  </section>
</div>

{#if modalOpen}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog"
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
          <span class="lbl">{m.settings_web_search_kind()}</span>
          <Select class="w-full" value={formKind} items={kindItems} onchange={onKindChange} />
          {#if formKind !== "scrape"}
            <span class="hint">{m.settings_web_search_api_hint()}</span>
          {/if}
        </div>
        <label class="field">
          <span class="lbl">{m.settings_web_search_field_title()}</span>
          <input bind:value={formTitle} />
        </label>
        {#if isPresetKind(formKind)}
          <label class="field">
            <span class="lbl">{m.settings_web_search_api_endpoint()}</span>
            <input class="mono" value={formUrl} readonly />
          </label>
        {:else}
          <label class="field">
            <span class="lbl">{m.settings_web_search_field_url({ query: "{query}" })}</span>
            <input bind:value={formUrl} spellcheck="false" />
          </label>
        {/if}
        {#if formKind === "custom_api"}
          <div class="field">
            <span class="lbl">{m.settings_web_search_field_method()}</span>
            <Select
              class="w-full"
              value={formMethod}
              items={methodItems}
              onchange={(v) => (formMethod = v)}
            />
          </div>
          <div class="field">
            <span class="lbl">{m.settings_web_search_field_auth_scheme()}</span>
            <Select
              class="w-full"
              value={formAuthScheme}
              items={authItems}
              onchange={(v) => (formAuthScheme = v)}
            />
          </div>
          {#if formAuthScheme === "header"}
            <label class="field">
              <span class="lbl">{m.settings_web_search_field_auth_header()}</span>
              <input bind:value={formAuthHeader} placeholder="X-API-KEY" spellcheck="false" />
            </label>
          {/if}
          <label class="field">
            <span class="lbl">{m.settings_web_search_field_body()}</span>
            <textarea
              class="mono"
              rows="3"
              bind:value={formBodyTemplate}
              spellcheck="false"
            ></textarea>
            <span class="hint">
              {m.settings_web_search_field_body_hint({ query: "{query}", count: "{count}" })}
            </span>
          </label>
          <label class="field">
            <span class="lbl">{m.settings_web_search_field_results_path()}</span>
            <input class="mono" bind:value={formResultsPath} placeholder="results" spellcheck="false" />
            <span class="hint">{m.settings_web_search_field_results_path_hint()}</span>
          </label>
          <label class="field">
            <span class="lbl">{m.settings_web_search_field_title_field()}</span>
            <input class="mono" bind:value={formTitleField} spellcheck="false" />
          </label>
          <label class="field">
            <span class="lbl">{m.settings_web_search_field_url_field()}</span>
            <input class="mono" bind:value={formUrlField} spellcheck="false" />
          </label>
          <label class="field">
            <span class="lbl">{m.settings_web_search_field_snippet_field()}</span>
            <input class="mono" bind:value={formSnippetField} spellcheck="false" />
          </label>
        {/if}
        {#if formKind !== "scrape"}
          <div class="field">
            <span class="lbl">
              {m.settings_web_search_api_key()}
              {#if editingIndex !== null && formHasKey}
                ·<span class="badge">{m.settings_web_search_api_key_saved()}</span>
              {/if}
            </span>
            {#if editingIndex === null}
              <span class="hint">{m.settings_web_search_api_key_save_first()}</span>
            {:else}
              <div class="key-row">
                <input
                  type="password"
                  bind:value={formApiKey}
                  placeholder={m.settings_web_search_api_key_placeholder()}
                  spellcheck="false"
                  autocomplete="off"
                />
                <button
                  class="btn"
                  disabled={!formApiKey.trim()}
                  onclick={() => void saveProviderKey()}
                >
                  {m.settings_web_search_api_key_save()}
                </button>
                <button class="btn" disabled={!formHasKey} onclick={() => void clearProviderKey()}>
                  {m.settings_web_search_api_key_clear()}
                </button>
              </div>
              {#if keyFlash}<span class="hint">{keyFlash}</span>{/if}
            {/if}
            <span class="hint">{m.settings_web_search_api_key_hint()}</span>
            {#if formKind === "brave_api"}
              <span class="hint">{m.settings_web_search_preset_brave_hint()}</span>
            {:else if formKind === "tavily_api"}
              <span class="hint">{m.settings_web_search_preset_tavily_hint()}</span>
            {:else if formKind === "serper_api"}
              <span class="hint">{m.settings_web_search_preset_serper_hint()}</span>
            {:else if formKind === "exa_api"}
              <span class="hint">{m.settings_web_search_preset_exa_hint()}</span>
            {/if}
          </div>
        {/if}
        {#if modalError}<div class="err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <span class="spacer"></span>
        <button class="btn" onclick={closeModals}>{m.common_cancel()}</button>
        <button class="btn primary" disabled={!canSaveProvider} onclick={() => void saveProvider()}>
          {m.common_save()}
        </button>
      </footer>
    </div>
  </div>
{/if}

{#if deleteIndex !== null}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog small"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <div class="body">
        <div class="confirm">{m.settings_web_search_delete_confirm()}</div>
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
  .toggle {
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    accent-color: var(--primary);
    cursor: default;
  }
  .hint {
    font-size: 0.7rem;
    color: var(--muted-foreground);
  }
  .status {
    min-height: 0.95rem;
    font-size: 0.7rem;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .saved {
    color: hsl(140 60% 40%);
  }
  .err {
    font-size: 0.72rem;
    color: var(--destructive);
    overflow-wrap: anywhere;
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
  textarea {
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-mono);
    outline: none;
    resize: vertical;
  }
  textarea:focus {
    border-color: var(--ring);
  }
  .mono {
    font-family: var(--font-mono);
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
  .badge {
    display: inline-block;
    margin-left: 0.35rem;
    padding: 0 0.3rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 0.65rem;
    font-weight: 400;
    color: var(--muted-foreground);
    vertical-align: middle;
  }
  .key-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .key-row input {
    flex: 1;
    min-width: 0;
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
  .btn:disabled,
  .btn:disabled:hover {
    opacity: 0.5;
    cursor: default;
    background: var(--background);
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
  .lbl {
    font-size: 0.72rem;
    color: var(--muted-foreground);
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