<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    mcpList,
    mcpRefresh,
    mcpTestDef,
    mcpCreate,
    mcpUpdate,
    mcpDelete,
    mcpReorder,
    mcpSetActive,
    onMcpChanged,
    mcpOAuthStart,
    mcpOAuthRefresh,
    mcpOAuthRevoke,
    type McpServerInfo,
    type McpServerInput,
    type McpBody,
    type McpOAuthStatus,
  } from "$lib/tauri";
  import { m as msg } from "$lib/i18n";
  import { Plus, RefreshCw, Trash2, Pencil, Wrench, X, Globe, ArrowUp, ArrowDown, LogIn, LogOut, Loader } from "@lucide/svelte";
  import McpWebUiModal from "./McpWebUiModal.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";

  const DEF_TEMPLATE = `{
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"],
  "env": {}
}`;

  let servers = $state<McpServerInfo[]>([]);
  let error = $state<string | null>(null);
  let expandedId = $state<string | null>(null);

  let modalOpen = $state(false);
  let editId = $state<string | null>(null);
  let formTitle = $state("");
  let formName = $state("");
  let formBody = $state("");
  let formActive = $state(true);
  let modalError = $state<string | null>(null);
  let testState = $state<"idle" | "testing" | "ok" | "fail">("idle");
  let testToolCount = $state(0);
  let testInfo = $state("");

  let deleteId = $state<string | null>(null);
  let webUiFor = $state<string | null>(null);

  let oauthLoading = $state<string | null>(null); // server_id being auth'd
  let oauthError = $state<string | null>(null);

  // OAuth confirmation dialog
  let oauthPromptId = $state<string | null>(null); // server_id pending OAuth confirm
  let oauthPromptLoading = $state(false);

  let unlistenMcp: (() => void) | undefined;
  onMount(() => {
    fetchList();
    onMcpChanged(() => fetchList()).then((u) => (unlistenMcp = () => u()));
  });
  onDestroy(() => unlistenMcp?.());

  async function fetchList() {
    try {
      servers = await mcpList();
    } catch (e) {
      error = String(e);
    }
  }

  async function refreshAll() {
    error = null;
    try {
      await mcpRefresh();
    } catch (e) {
      error = String(e);
    }
    await fetchList();
  }

  function dotClass(s: McpServerInfo): string {
    if (s.status === "connected") return "ok";
    if (s.status === "needs_auth") return "warn";
    if (typeof s.status === "object") return "err";
    return "warn";
  }

  function statusTitle(s: McpServerInfo): string {
    if (s.status === "connected") return "connected";
    if (s.status === "connecting") return "connecting";
    if (s.status === "disabled") return "inactive";
    if (s.status === "needs_auth") return "authentication required";
    if (typeof s.status === "object") return s.status.error;
    return "";
  }

  async function signIn(s: McpServerInfo) {
    oauthError = null;
    oauthLoading = s.id;
    try {
      await mcpOAuthStart(s.id);
      await fetchList();
    } catch (e) {
      oauthError = String(e);
    } finally {
      oauthLoading = null;
    }
  }

  async function signOut(s: McpServerInfo) {
    oauthError = null;
    try {
      await mcpOAuthRevoke(s.id);
      await fetchList();
    } catch (e) {
      oauthError = String(e);
    }
  }

  async function refreshToken(s: McpServerInfo) {
    oauthError = null;
    oauthLoading = s.id;
    try {
      await mcpOAuthRefresh(s.id);
      await fetchList();
    } catch (e) {
      oauthError = String(e);
    } finally {
      oauthLoading = null;
    }
  }

  async function toggleActive(s: McpServerInfo, active: boolean) {
    try {
      await mcpSetActive(s.id, active);
    } catch (e) {
      error = String(e);
    }
    await fetchList();
  }

  async function move(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= servers.length) return;
    const next = [...servers];
    [next[i], next[j]] = [next[j], next[i]];
    servers = next;
    try {
      await mcpReorder(next.map((s) => s.id));
      error = null;
    } catch (e) {
      error = String(e);
    }
    await fetchList();
  }

  function toggleTools(id: string) {
    expandedId = expandedId === id ? null : id;
  }

  function parseBody(): { ok: true; body: McpBody } | { ok: false; reason: string } {
    let parsed: unknown;
    try {
      parsed = JSON.parse(formBody);
    } catch {
      return { ok: false, reason: msg.mcp_invalid_json() };
    }
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      return { ok: false, reason: msg.mcp_invalid_json() };
    }
    const body = parsed as McpBody;
    if (typeof body.command !== "string" && typeof body.url !== "string") {
      return { ok: false, reason: msg.mcp_invalid_json() };
    }
    return { ok: true, body };
  }

  function resetModal() {
    modalError = null;
    testState = "idle";
    testToolCount = 0;
    testInfo = "";
  }

  function openAdd() {
    editId = null;
    formTitle = "";
    formName = "";
    formBody = DEF_TEMPLATE;
    formActive = true;
    resetModal();
    modalOpen = true;
  }

  function openEdit(s: McpServerInfo) {
    editId = s.id;
    formTitle = s.title;
    formName = s.name;
    formBody = JSON.stringify(s.body ?? {}, null, 2);
    formActive = s.is_active;
    resetModal();
    modalOpen = true;
  }

  function closeModal() {
    modalOpen = false;
  }

  function clearTest() {
    testState = "idle";
    testToolCount = 0;
    testInfo = "";
  }

  async function runTest() {
    const parsed = parseBody();
    if (!parsed.ok) {
      clearTest();
      modalError = parsed.reason;
      return;
    }
    modalError = null;
    testState = "testing";
    try {
      const r = await mcpTestDef(parsed.body);
      if (r.ok) {
        testState = "ok";
        testToolCount = r.tool_count;
      } else if (r.needs_auth) {
        testState = "fail";
        testInfo = msg.mcp_oauth_test_detected();
        // Offer to save and sign in
        oauthPromptId = "test"; // special marker — means "save first, then OAuth"
      } else {
        testState = "fail";
        testInfo = r.error ?? "";
      }
    } catch (e) {
      testState = "fail";
      testInfo = String(e);
    }
  }

  async function saveServer(): Promise<string | null> {
    const parsed = parseBody();
    if (!parsed.ok) {
      modalError = parsed.reason;
      return null;
    }
    const name = formName.trim();
    if (!name) {
      modalError = msg.mcp_name_required();
      return null;
    }
    const title = formTitle.trim();
    if (!title) {
      modalError = msg.mcp_title_required();
      return null;
    }
    const input: McpServerInput = { title, name, body: parsed.body, is_active: formActive };
    try {
      let serverId: string;
      if (editId !== null) {
        await mcpUpdate(editId, input);
        serverId = editId;
      } else {
        serverId = await mcpCreate(input);
      }
      modalOpen = false;
      await fetchList();
      // Auto-detect: if the server needs OAuth, prompt the user
      const srv = servers.find((s) => s.id === serverId);
      if (srv && srv.status === "needs_auth") {
        oauthPromptId = serverId;
      }
      return serverId;
    } catch (e) {
      modalError = String(e);
      return null;
    }
  }

  async function confirmOAuth() {
    if (oauthPromptId === null) return;
    oauthPromptLoading = true;
    try {
      if (oauthPromptId === "test") {
        // Save the server first, then start OAuth
        const id = await saveServer();
        if (id) {
          await mcpOAuthStart(id);
          await fetchList();
        }
      } else {
        await mcpOAuthStart(oauthPromptId);
        await fetchList();
      }
    } catch (e) {
      oauthError = String(e);
    } finally {
      oauthPromptLoading = false;
      oauthPromptId = null;
    }
  }

  function cancelOAuth() {
    oauthPromptId = null;
  }

  async function confirmDelete() {
    if (deleteId === null) return;
    const id = deleteId;
    deleteId = null;
    try {
      await mcpDelete(id);
      if (expandedId === id) expandedId = null;
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

<div class="mcp">
  <div class="row">
    <span class="hint">{msg.settings_tab_mcp()}</span>
    <div class="btns">
      <button class="icon-btn" title={msg.mcp_refresh()} onclick={() => void refreshAll()}>
        <RefreshCw size={13} />
      </button>
      <button class="btn" onclick={openAdd}><Plus size={13} /> {msg.mcp_add()}</button>
    </div>
  </div>

  <div class="list">
    {#each servers as s, i (s.id)}
      <div class="srv-wrap">
        <div class="srv">
          <span class="dot {dotClass(s)}" title={statusTitle(s)}></span>
          <div class="srv-main">
            <span class="name">{s.title}</span>
            <span class="sub">
              {s.name} · {s.transport} · {s.tool_count} tool(s){#if typeof s.status === "object"} ·
                {s.status.error}{/if}{#if s.status === "needs_auth"} · {msg.mcp_oauth_required()}{/if}{#if
                s.auth_expired} · {msg.mcp_oauth_expired()}{/if}
            </span>
          </div>
          <input
            class="toggle"
            type="checkbox"
            title="Active"
            checked={s.is_active}
            onchange={(e) => void toggleActive(s, e.currentTarget.checked)}
          />
          <button class="icon-btn" disabled={i === 0} title="Move up" onclick={() => void move(i, -1)}>
            <ArrowUp size={13} />
          </button>
          <button
            class="icon-btn"
            disabled={i === servers.length - 1}
            title="Move down"
            onclick={() => void move(i, 1)}
          >
            <ArrowDown size={13} />
          </button>
          <button
            class="icon-btn"
            class:active={!!s.webui_url}
            title={msg.mcp_webui()}
            onclick={() => (webUiFor = s.id)}
          >
            <Globe size={13} />
          </button>
          <button
            class="icon-btn"
            class:active={expandedId === s.id}
            title={msg.mcp_tools()}
            onclick={() => toggleTools(s.id)}
          >
            <Wrench size={13} />
          </button>
          {#if s.status === "needs_auth"}
            <button
              class="icon-btn auth"
              title={oauthLoading === s.id ? msg.mcp_oauth_authenticating() : msg.mcp_oauth_sign_in()}
              disabled={oauthLoading === s.id}
              onclick={() => void signIn(s)}
            >
              {#if oauthLoading === s.id}
                <Loader size={13} class="spin" />
              {:else}
                <LogIn size={13} />
              {/if}
            </button>
          {/if}
          {#if s.oauth_authenticated && s.status === "connected"}
            <button
              class="icon-btn"
              title={msg.mcp_oauth_sign_out()}
              onclick={() => void signOut(s)}
            >
              <LogOut size={13} />
            </button>
          {/if}
          <button class="icon-btn" title={msg.common_edit()} onclick={() => openEdit(s)}>
            <Pencil size={13} />
          </button>
          <button class="icon-btn danger" title={msg.common_delete()} onclick={() => (deleteId = s.id)}>
            <Trash2 size={13} />
          </button>
        </div>
        {#if expandedId === s.id}
          <div class="tools-panel">
            <span class="tools-title">{msg.mcp_tools()}</span>
            {#if s.tools.length === 0}
              <span class="no-tools">{msg.mcp_no_tools()}</span>
            {:else}
              <div class="chips">
                {#each s.tools as t (t)}
                  <code class="chip">{t}</code>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
    {#if servers.length === 0}
      <div class="empty">{msg.mcp_empty()}</div>
    {/if}
  </div>
  {#if error}<div class="err">{error}</div>{/if}
  {#if oauthError}<div class="err">{oauthError}</div>{/if}
</div>

{#if modalOpen}
  <div class="overlay" onkeydown={onKeydown} role="presentation">
    <div class="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
      <header class="head">
        <span class="head-title">{editId !== null ? msg.mcp_edit_title() : msg.mcp_add_title()}</span>
        <button class="x" title={msg.common_close()} onclick={closeModal}><X size={16} /></button>
      </header>
      <div class="body">
        <label class="field">
          <span class="lbl">{msg.mcp_server_title()}</span>
          <input bind:value={formTitle} spellcheck="false" />
        </label>
        <label class="field">
          <span class="lbl">{msg.mcp_server_name()}</span>
          <input bind:value={formName} spellcheck="false" />
        </label>
        <label class="field">
          <span class="lbl">{msg.mcp_server_json()}</span>
          <textarea bind:value={formBody} oninput={clearTest} spellcheck="false"></textarea>
        </label>
        <label class="field check">
          <span class="lbl">Active</span>
          <input class="toggle" type="checkbox" bind:checked={formActive} />
        </label>
        {#if testState === "testing"}
          <div class="res">{msg.mcp_testing()}</div>
        {:else if testState === "ok"}
          <div class="res ok">{msg.mcp_available()} — {testToolCount} tool(s)</div>
        {:else if testState === "fail"}
          <div class="res err">{msg.mcp_unavailable()}{testInfo ? ` — ${testInfo}` : ""}</div>
        {/if}
        {#if modalError}<div class="res err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <button class="btn" onclick={runTest} disabled={testState === "testing"}>{msg.mcp_test()}</button>
        <div class="spacer"></div>
        <button class="btn ghost" onclick={closeModal}>{msg.common_cancel()}</button>
        <button class="btn primary" onclick={() => void saveServer()}>{msg.common_save()}</button>
      </footer>
    </div>
  </div>
{/if}

{#if deleteId !== null}
  <div class="overlay" onkeydown={onKeydown} role="presentation">
    <div class="dialog small" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
      <div class="body">
        <div class="confirm">{msg.mcp_delete_confirm()}</div>
        <code class="confirm-id">{deleteId}</code>
      </div>
      <footer class="foot">
        <div class="spacer"></div>
        <button class="btn ghost" onclick={() => (deleteId = null)}>{msg.common_cancel()}</button>
        <button class="btn danger" onclick={confirmDelete}>{msg.common_delete()}</button>
      </footer>
    </div>
  </div>
{/if}

{#if webUiFor !== null}
  {@const srv = servers.find((x) => x.id === webUiFor)}
  <McpWebUiModal
    serverId={webUiFor}
    existingUrl={srv?.webui_url ?? ""}
    existingIcon={srv?.webui_icon ?? ""}
    onClose={() => (webUiFor = null)}
  />
{/if}

<ConfirmDialog
  open={oauthPromptId !== null}
  message={msg.mcp_oauth_prompt_message()}
  confirmLabel={msg.mcp_oauth_prompt_confirm()}
  variant="primary"
  loading={oauthPromptLoading}
  loadingLabel={msg.mcp_oauth_authenticating()}
  onconfirm={() => void confirmOAuth()}
  oncancel={cancelOAuth}
/>

<style>
  .mcp { display: flex; flex-direction: column; gap: 0.75rem; }
  .row { display: flex; align-items: center; justify-content: space-between; }
  .hint { font-weight: 500; font-size: 0.8125rem; }
  .btns { display: flex; align-items: center; gap: 0.4rem; }
  .list { display: flex; flex-direction: column; gap: 0.25rem; }
  .srv-wrap { display: flex; flex-direction: column; gap: 0.25rem; }
  .srv { display: flex; align-items: center; gap: 0.4rem; padding: 0.4rem 0.5rem; border: 1px solid var(--border); border-radius: var(--radius-md); }
  .srv-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .name { font-size: 0.8125rem; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sub { font-size: 0.6875rem; color: var(--muted-foreground); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .dot { width: 8px; height: 8px; border-radius: 9999px; flex-shrink: 0; background: var(--muted-foreground); }
  .dot.ok { background: hsl(140 60% 40%); }
  .dot.warn { background: hsl(38 92% 50%); }
  .dot.err { background: var(--destructive); }
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
  .icon-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .icon-btn.active {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .icon-btn.danger:hover {
    color: var(--destructive);
  }
  .icon-btn.auth {
    color: hsl(239 84% 67%);
  }
  .icon-btn.auth:hover {
    background: var(--accent);
    color: hsl(239 84% 72%);
  }
  .icon-btn:disabled {
    opacity: 0.4;
  }
  :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .tools-panel { margin-left: 1.5rem; display: flex; flex-direction: column; gap: 0.4rem; padding: 0.5rem 0.6rem; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--accent); }
  .tools-title { font-size: 0.6875rem; color: var(--muted-foreground); }
  .chips { display: flex; flex-wrap: wrap; gap: 0.3rem; }
  .chip { font-family: var(--font-mono); font-size: 0.6875rem; padding: 0.15rem 0.4rem; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--background); color: var(--foreground); }
  .no-tools { font-size: 0.75rem; color: var(--muted-foreground); }
  .empty { padding: 1rem; color: var(--muted-foreground); font-size: 0.8125rem; }
  .err { color: var(--destructive); font-size: 0.75rem; overflow-wrap: anywhere; }

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
    max-height: 90vh;
    overflow: auto;
    display: flex;
    flex-direction: column;
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
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
  .body { padding: 0.875rem 1rem; display: flex; flex-direction: column; gap: 0.6rem; }
  .foot { display: flex; align-items: center; gap: 0.5rem; padding: 0.625rem 0.875rem; border-top: 1px solid var(--border); }
  .spacer { flex: 1; }
  .field { display: flex; flex-direction: column; gap: 0.25rem; }
  .field.check { flex-direction: row; align-items: center; gap: 0.5rem; }
  .lbl { font-size: 0.72rem; color: var(--muted-foreground); }
  input {
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
  }
  textarea {
    width: 100%;
    height: 170px;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    line-height: 1.4;
    padding: 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    resize: vertical;
  }
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
  }
  .btn:hover { background: var(--accent); }
  .btn:disabled { opacity: 0.5; }
  .btn.primary { background: var(--primary); color: var(--primary-foreground); border-color: var(--primary); }
  .btn.ghost { background: transparent; }
  .btn.danger { color: var(--destructive); border-color: var(--destructive); }
  .btn.danger:hover { background: var(--destructive); color: var(--destructive-foreground); }
  .res { font-size: 0.75rem; color: var(--muted-foreground); }
  .res.ok { color: hsl(140 60% 40%); }
  .res.err { color: var(--destructive); }
  .confirm { font-size: 0.8125rem; }
  .confirm-id { font-family: var(--font-mono); font-size: 0.75rem; color: var(--muted-foreground); }
</style>