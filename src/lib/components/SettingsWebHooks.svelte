<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Plus, Trash2, Pencil, X, ArrowUp, ArrowDown } from "@lucide/svelte";
  import { m as msg } from "$lib/i18n";
  import {
    webHooksList,
    webHooksCreate,
    webHooksUpdate,
    webHooksDelete,
    webHooksReorder,
    webHooksSetActive,
    webHooksSetSecret,
    webHooksClearSecret,
    webHooksHasSecret,
    webHooksTest,
    onWebHooksChanged,
    type WebHookView,
    type WebHookInput,
    type WebHookTestResult,
  } from "$lib/tauri";
  import Select from "./Select.svelte";

  const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
  const AUTH_TYPES = ["none", "bearer", "basic", "api_key_header", "api_key_query"];
  const TEST_PAYLOAD = '{"test": true}';
  const HEADERS_HINT = '{"Content-Type":"application/json"}';
  const PLACEHOLDERS = "{{secret}}, {{variables.KEY}}, {{payload}}";

  let hooks = $state<WebHookView[]>([]);
  let error = $state<string | null>(null);
  let saved = $state(false);

  let modalOpen = $state(false);
  let editId = $state<string | null>(null);
  let formTitle = $state("");
  let formName = $state("");
  let formDescription = $state("");
  let formMethod = $state("GET");
  let formUrl = $state("");
  let formHeaders = $state("");
  let formBodyTemplate = $state("");
  let formAuthType = $state("none");
  let formAuthUsername = $state("");
  let formAuthHeaderName = $state("");
  let formAuthParamName = $state("");
  let formSecret = $state("");
  let formTimeoutMs = $state(30000);
  let formActive = $state(true);
  let hasSecret = $state(false);
  let modalError = $state<string | null>(null);

  let testState = $state<"idle" | "testing" | "ok" | "fail">("idle");
  let testSummary = $state("");
  let testBody = $state("");

  let deleteId = $state<string | null>(null);

  let savedTimer: ReturnType<typeof setTimeout> | undefined;
  let unlistenHooks: (() => void) | undefined;

  onMount(() => {
    void fetchList();
    onWebHooksChanged(() => void fetchList()).then((u) => (unlistenHooks = () => u()));
  });
  onDestroy(() => {
    unlistenHooks?.();
    clearTimeout(savedTimer);
  });

  function flashSaved() {
    saved = true;
    clearTimeout(savedTimer);
    savedTimer = setTimeout(() => (saved = false), 1500);
  }

  async function fetchList() {
    try {
      hooks = await webHooksList();
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleActive(h: WebHookView, active: boolean) {
    try {
      await webHooksSetActive(h.id, active);
      error = null;
      flashSaved();
    } catch (e) {
      error = String(e);
    }
    await fetchList();
  }

  async function move(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= hooks.length) return;
    const next = [...hooks];
    [next[i], next[j]] = [next[j], next[i]];
    hooks = next;
    try {
      await webHooksReorder(next.map((h) => h.id));
      error = null;
      flashSaved();
    } catch (e) {
      error = String(e);
    }
    await fetchList();
  }

  function authLabel(t: string): string {
    if (t === "bearer") return msg.settings_web_hooks_auth_bearer();
    if (t === "basic") return msg.settings_web_hooks_auth_basic();
    if (t === "api_key_header") return msg.settings_web_hooks_auth_api_key_header();
    if (t === "api_key_query") return msg.settings_web_hooks_auth_api_key_query();
    return msg.settings_web_hooks_auth_none();
  }

  function resetModal() {
    modalError = null;
    testState = "idle";
    testSummary = "";
    testBody = "";
  }

  function clearTest() {
    testState = "idle";
    testSummary = "";
    testBody = "";
  }

  function openAdd() {
    editId = null;
    formTitle = "";
    formName = "";
    formDescription = "";
    formMethod = "GET";
    formUrl = "";
    formHeaders = "";
    formBodyTemplate = "";
    formAuthType = "none";
    formAuthUsername = "";
    formAuthHeaderName = "";
    formAuthParamName = "";
    formSecret = "";
    formTimeoutMs = 30000;
    formActive = true;
    hasSecret = false;
    resetModal();
    modalOpen = true;
  }

  function openEdit(h: WebHookView) {
    editId = h.id;
    formTitle = h.title;
    formName = h.name;
    formDescription = h.description;
    formMethod = h.method;
    formUrl = h.url;
    formHeaders = h.headers;
    formBodyTemplate = h.body_template;
    formAuthType = h.auth_type;
    formAuthUsername = h.auth_username;
    formAuthHeaderName = h.auth_header_name;
    formAuthParamName = h.auth_param_name;
    formSecret = "";
    formTimeoutMs = h.timeout_ms;
    formActive = h.is_active;
    hasSecret = h.has_secret;
    resetModal();
    modalOpen = true;
    void webHooksHasSecret(h.id)
      .then((v) => (hasSecret = v))
      .catch(() => (hasSecret = h.has_secret));
  }

  function closeModal() {
    modalOpen = false;
  }

  async function setSecret() {
    if (editId === null || !formSecret.trim()) return;
    try {
      await webHooksSetSecret(editId, formSecret);
      formSecret = "";
      hasSecret = true;
      modalError = null;
    } catch (e) {
      modalError = String(e);
    }
  }

  async function clearSecret() {
    if (editId === null) return;
    try {
      await webHooksClearSecret(editId);
      formSecret = "";
      hasSecret = false;
      modalError = null;
    } catch (e) {
      modalError = String(e);
    }
  }

  async function runTest() {
    if (editId === null) return;
    modalError = null;
    testState = "testing";
    testSummary = "";
    testBody = "";
    try {
      const r: WebHookTestResult = await webHooksTest(editId, TEST_PAYLOAD);
      testState = r.ok ? "ok" : "fail";
      const parts: string[] = [];
      if (r.status !== null) parts.push(String(r.status));
      if (r.detail) parts.push(r.detail);
      parts.push(`${r.elapsed_ms} ms`);
      testSummary = parts.join(" · ");
      testBody = r.body.slice(0, 600);
    } catch (e) {
      testState = "fail";
      testSummary = String(e);
    }
  }

  async function save() {
    const title = formTitle.trim();
    if (!title) {
      modalError = "Title is required";
      return;
    }
    const name = formName.trim();
    if (!name) {
      modalError = "Name is required";
      return;
    }
    const url = formUrl.trim();
    if (!/^https?:\/\//i.test(url)) {
      modalError = "URL must start with http:// or https://";
      return;
    }
    let headers = formHeaders.trim();
    if (headers) {
      try {
        const parsed: unknown = JSON.parse(headers);
        if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
          throw new Error("not an object");
        }
      } catch {
        modalError = `Headers must be a valid JSON object, e.g. ${HEADERS_HINT}`;
        return;
      }
    } else {
      headers = "{}";
    }
    const input: WebHookInput = {
      title,
      name,
      description: formDescription,
      method: formMethod,
      url,
      headers,
      body_template: formBodyTemplate,
      auth_type: formAuthType,
      auth_username: formAuthUsername.trim(),
      auth_header_name: formAuthHeaderName.trim(),
      auth_param_name: formAuthParamName.trim(),
      timeout_ms: formTimeoutMs > 0 ? Math.floor(formTimeoutMs) : 30000,
      is_active: formActive,
    };
    modalError = null;
    try {
      const secret = formSecret.trim();
      if (editId !== null) {
        await webHooksUpdate(editId, input);
        if (secret) await webHooksSetSecret(editId, secret);
      } else {
        await webHooksCreate(input);
        if (secret) {
          const list = await webHooksList();
          const created = list.find((h) => h.name === name);
          if (created) await webHooksSetSecret(created.id, secret);
        }
      }
      modalOpen = false;
      error = null;
      flashSaved();
      await fetchList();
    } catch (e) {
      modalError = String(e);
    }
  }

  async function confirmDelete() {
    if (deleteId === null) return;
    const id = deleteId;
    deleteId = null;
    try {
      await webHooksDelete(id);
      error = null;
      flashSaved();
      await fetchList();
    } catch (e) {
      error = String(e);
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (modalOpen) closeModal();
      if (deleteId !== null) deleteId = null;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="hooks">
  <div class="row">
    <span class="hint">{msg.settings_web_hooks_title()}</span>
    <button class="btn" onclick={openAdd}>
      <Plus size={13} /> {msg.settings_web_hooks_add()}
    </button>
  </div>
  <div class="hint-text">{msg.settings_web_hooks_hint({ placeholder: PLACEHOLDERS })}</div>

  <div class="list">
    {#each hooks as h, i (h.id)}
      <div class="hook">
        <div class="hook-main">
          <span class="name">{h.title}</span>
          <span class="sub">
            <code class="method">{h.method}</code>
            {h.name} · <span class="url" title={h.url}>{h.url}</span>
          </span>
        </div>
        <input
          class="toggle"
          type="checkbox"
          title="Active"
          checked={h.is_active}
          onchange={(e) => void toggleActive(h, e.currentTarget.checked)}
        />
        <button class="icon-btn" disabled={i === 0} title="Move up" onclick={() => void move(i, -1)}>
          <ArrowUp size={13} />
        </button>
        <button
          class="icon-btn"
          disabled={i === hooks.length - 1}
          title="Move down"
          onclick={() => void move(i, 1)}
        >
          <ArrowDown size={13} />
        </button>
        <button class="icon-btn" title={msg.common_edit()} onclick={() => openEdit(h)}>
          <Pencil size={13} />
        </button>
        <button class="icon-btn danger" title={msg.common_delete()} onclick={() => (deleteId = h.id)}>
          <Trash2 size={13} />
        </button>
      </div>
    {/each}
    {#if hooks.length === 0}
      <div class="empty">{msg.settings_web_hooks_empty()}</div>
    {/if}
  </div>

  <div class="status">
    {#if saved}<span class="saved">{msg.settings_web_hooks_saved()}</span>
    {:else if error}<span class="err">{error}</span>{/if}
  </div>
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
        <span class="head-title">
          {editId !== null ? msg.settings_web_hooks_edit() : msg.settings_web_hooks_add()}
        </span>
        <button class="x" title={msg.common_close()} onclick={closeModal}><X size={16} /></button>
      </header>
      <div class="body">
        <label class="field">
          <span class="lbl">{msg.settings_web_hooks_field_title()}</span>
          <input bind:value={formTitle} spellcheck="false" />
        </label>
        <label class="field">
          <span class="lbl">{msg.settings_web_hooks_field_name()}</span>
          <input bind:value={formName} spellcheck="false" />
          <span class="field-hint">used by the model in web_hook_run</span>
        </label>
        <label class="field">
          <span class="lbl">{msg.settings_web_hooks_field_description()}</span>
          <textarea class="ta" bind:value={formDescription} spellcheck="false"></textarea>
          <span class="field-hint">{msg.settings_web_hooks_description_hint()}</span>
        </label>
        <div class="field">
          <span class="lbl">{msg.settings_web_hooks_field_method()}</span>
          <Select
            class="w-full"
            value={formMethod}
            items={METHODS.map((mm) => ({ value: mm, label: mm }))}
            onchange={(v) => {
              formMethod = v;
              clearTest();
            }}
          />
        </div>
        <label class="field">
          <span class="lbl">{msg.settings_web_hooks_field_url()}</span>
          <input bind:value={formUrl} spellcheck="false" oninput={clearTest} />
          <span class="field-hint">{msg.settings_web_hooks_url_hint({ placeholder: PLACEHOLDERS })}</span>
        </label>
        <label class="field">
          <span class="lbl">{msg.settings_web_hooks_field_headers()}</span>
          <textarea class="ta mono" bind:value={formHeaders} oninput={clearTest} spellcheck="false"></textarea>
          <span class="field-hint"><code>{HEADERS_HINT}</code></span>
        </label>
        <label class="field">
          <span class="lbl">{msg.settings_web_hooks_field_body_template()}</span>
          <textarea class="ta mono tall" bind:value={formBodyTemplate} oninput={clearTest} spellcheck="false"></textarea>
          <span class="field-hint">{msg.settings_web_hooks_body_template_hint({ placeholder: PLACEHOLDERS })}</span>
        </label>
        <div class="field">
          <span class="lbl">{msg.settings_web_hooks_field_auth_type()}</span>
          <Select
            class="w-full"
            value={formAuthType}
            items={AUTH_TYPES.map((t) => ({ value: t, label: authLabel(t) }))}
            onchange={(v) => {
              formAuthType = v;
              clearTest();
            }}
          />
        </div>
        {#if formAuthType === "basic"}
          <label class="field">
            <span class="lbl">{msg.settings_web_hooks_field_auth_username()}</span>
            <input bind:value={formAuthUsername} spellcheck="false" />
          </label>
        {:else if formAuthType === "api_key_header"}
          <label class="field">
            <span class="lbl">{msg.settings_web_hooks_field_auth_header_name()}</span>
            <input bind:value={formAuthHeaderName} spellcheck="false" />
          </label>
        {:else if formAuthType === "api_key_query"}
          <label class="field">
            <span class="lbl">{msg.settings_web_hooks_field_auth_param_name()}</span>
            <input bind:value={formAuthParamName} spellcheck="false" />
          </label>
        {/if}
        {#if formAuthType !== "none"}
          <label class="field">
            <span class="lbl">
              {msg.settings_web_hooks_field_secret()}
              {#if editId !== null}
                · {hasSecret
                  ? msg.settings_web_hooks_secret_set()
                  : msg.settings_web_hooks_secret_not_set()}
              {/if}
            </span>
            <div class="secret-row">
              <input type="password" bind:value={formSecret} spellcheck="false" />
              {#if editId !== null}
                <button class="btn" disabled={!formSecret.trim()} onclick={() => void setSecret()}>
                  Set
                </button>
                <button class="btn" disabled={!hasSecret} onclick={() => void clearSecret()}>
                  Clear
                </button>
              {/if}
            </div>
          </label>
        {/if}
        <label class="field">
          <span class="lbl">{msg.settings_web_hooks_field_timeout()}</span>
          <input class="timeout" type="number" bind:value={formTimeoutMs} min="1" step="1000" />
        </label>
        <label class="field check">
          <span class="lbl">Active</span>
          <input class="toggle" type="checkbox" bind:checked={formActive} />
        </label>
        {#if testState === "testing"}
          <div class="res">{msg.settings_web_hooks_testing()}</div>
        {:else if testState === "ok"}
          <div class="res ok">{msg.settings_web_hooks_test_ok()} · {testSummary}</div>
          {#if testBody}<pre class="test-out">{testBody}</pre>{/if}
        {:else if testState === "fail"}
          <div class="res err">{msg.settings_web_hooks_test_fail()} · {testSummary}</div>
          {#if testBody}<pre class="test-out">{testBody}</pre>{/if}
        {/if}
        {#if modalError}<div class="res err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        {#if editId !== null}
          <button class="btn" onclick={() => void runTest()} disabled={testState === "testing"}>
            {msg.settings_web_hooks_test()}
          </button>
        {/if}
        <div class="spacer"></div>
        <button class="btn ghost" onclick={closeModal}>{msg.common_cancel()}</button>
        <button class="btn primary" onclick={() => void save()}>{msg.common_save()}</button>
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
        <div class="confirm">{msg.settings_web_hooks_delete_confirm()}</div>
        <code class="confirm-id">{hooks.find((h) => h.id === deleteId)?.title ?? deleteId}</code>
      </div>
      <footer class="foot">
        <div class="spacer"></div>
        <button class="btn ghost" onclick={() => (deleteId = null)}>{msg.common_cancel()}</button>
        <button class="btn danger" onclick={() => void confirmDelete()}>{msg.common_delete()}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .hooks { display: flex; flex-direction: column; gap: 0.75rem; }
  .row { display: flex; align-items: center; justify-content: space-between; }
  .hint { font-weight: 500; font-size: 0.8125rem; }
  .hint-text { font-size: 0.7rem; color: var(--muted-foreground); margin-top: -0.5rem; }
  .list { display: flex; flex-direction: column; gap: 0.25rem; }
  .hook {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .hook-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .name { font-size: 0.8125rem; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sub { font-size: 0.6875rem; color: var(--muted-foreground); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .method {
    display: inline-block;
    font-family: var(--font-mono);
    font-size: 0.625rem;
    line-height: 1.2;
    padding: 0 0.3rem;
    margin-right: 0.25rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
  }
  .url { overflow: hidden; text-overflow: ellipsis; }
  .toggle {
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    accent-color: var(--primary);
    cursor: default;
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
  .icon-btn:hover { background: var(--accent); color: var(--accent-foreground); }
  .icon-btn.danger:hover { color: var(--destructive); }
  .icon-btn:disabled { opacity: 0.4; }
  .empty { padding: 1rem; color: var(--muted-foreground); font-size: 0.8125rem; }
  .status { min-height: 0.95rem; font-size: 0.7rem; }
  .saved { color: hsl(140 60% 40%); }
  .err { color: var(--destructive); font-size: 0.72rem; overflow-wrap: anywhere; }

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
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
  .dialog.small { width: 360px; }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
  }
  .head-title { font-size: 0.875rem; font-weight: 500; }
  .x {
    display: inline-flex;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .x:hover { background: var(--accent); }
  .body { padding: 0.875rem 1rem; overflow-y: auto; min-height: 0; display: flex; flex-direction: column; gap: 0.6rem; }
  .foot {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 0.875rem;
    border-top: 1px solid var(--border);
  }
  .spacer { flex: 1; }
  .field { display: flex; flex-direction: column; gap: 0.25rem; }
  .field.check { flex-direction: row; align-items: center; gap: 0.5rem; }
  .lbl { font-size: 0.72rem; color: var(--muted-foreground); }
  .field-hint { font-size: 0.65rem; color: var(--muted-foreground); overflow-wrap: anywhere; }
  .field-hint code { font-family: var(--font-mono); }
  input {
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-sans);
  }
  input:focus { border-color: var(--ring); }
  .timeout { width: 160px; }
  .secret-row { display: flex; align-items: center; gap: 0.4rem; }
  .secret-row input { flex: 1; min-width: 0; }
  textarea.ta {
    width: 100%;
    height: 72px;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-family: var(--font-sans);
    font-size: 0.75rem;
    line-height: 1.4;
    resize: vertical;
  }
  textarea.ta.mono { font-family: var(--font-mono); }
  textarea.ta.tall { height: 110px; }
  textarea.ta:focus { border-color: var(--ring); }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.3rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.75rem;
    cursor: default;
  }
  .btn:hover { background: var(--accent); }
  .btn:disabled { opacity: 0.5; }
  .btn.primary { background: var(--primary); color: var(--primary-foreground); border-color: var(--primary); }
  .btn.ghost { background: transparent; }
  .btn.danger { color: var(--destructive); border-color: var(--destructive); }
  .btn.danger:hover { background: var(--destructive); color: var(--destructive-foreground); }
  .res { font-size: 0.75rem; color: var(--muted-foreground); }
  .res.ok { color: hsl(140 60% 40%); }
  .res.err { color: var(--destructive); overflow-wrap: anywhere; }
  .test-out {
    margin: 0;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--muted-foreground);
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    line-height: 1.4;
    max-height: 180px;
    overflow-y: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .confirm { font-size: 0.8125rem; }
  .confirm-id { font-family: var(--font-mono); font-size: 0.75rem; color: var(--muted-foreground); }
</style>