<script lang="ts">
  import { Globe, FolderOpen, Trash2, X, Loader2 } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import {
    mcpWebUiSave,
    mcpWebUiDelete,
    mcpWebUiDetectFavicon,
  } from "$lib/tauri";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";

  let { serverId, existingUrl = "", existingIcon = "", onClose } = $props<{
    serverId: string;
    existingUrl: string;
    existingIcon: string;
    onClose: () => void;
  }>();

  // svelte-ignore state_referenced_locally (form draft is captured once; parent remounts the modal on open)
  let url = $state(existingUrl);
  // svelte-ignore state_referenced_locally
  let favicon = $state(existingIcon);
  let faviconNote = $state<string | null>(null);
  let detecting = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let confirmDelete = $state(false);

  function close() {
    onClose();
  }

  async function autoDetect() {
    faviconNote = null;
    if (!url.trim()) {
      faviconNote = m.mcp_webui_favicon_not_found();
      return;
    }
    detecting = true;
    try {
      const r = await mcpWebUiDetectFavicon(url.trim(), favicon.trim() || null);
      if (r.ok && r.url) favicon = r.url;
      else faviconNote = m.mcp_webui_favicon_not_found();
    } catch {
      faviconNote = m.mcp_webui_favicon_not_found();
    }
    detecting = false;
  }

  async function manualPick() {
    faviconNote = null;
    const p = await openDialog({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico"] }],
    });
    if (typeof p === "string") favicon = p;
  }

  async function save() {
    error = null;
    if (!url.trim()) {
      error = m.mcp_webui_url_required();
      return;
    }
    saving = true;
    try {
      await mcpWebUiSave(serverId, url.trim(), favicon.trim() || null);
      onClose();
    } catch (e) {
      error = String(e);
    }
    saving = false;
  }

  async function doDelete() {
    confirmDelete = false;
    try {
      await mcpWebUiDelete(serverId);
      onClose();
    } catch (e) {
      error = String(e);
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
</script>

<div class="overlay" onkeydown={onKeydown} role="presentation">
  <div class="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
    <header class="head">
      <span class="head-title">{m.mcp_webui()}</span>
      <button class="x" title={m.common_close()} onclick={close}><X size={16} /></button>
    </header>
    <div class="body">
      <label class="field">
        <span class="lbl">{m.mcp_webui_url()}</span>
        <input bind:value={url} placeholder="https://… or http://127.0.0.1:port/" />
      </label>
      <div class="field">
        <div class="favicon-row">
          <span class="lbl">{m.mcp_webui_favicon()}</span>
          <div class="favicon-actions">
            <button class="mini" title={m.mcp_webui_auto_detect()} onclick={autoDetect} disabled={detecting}>
              {#if detecting}<span class="spin"><Loader2 size={13} /></span>{:else}<Globe size={13} />{/if}
            </button>
            <button class="mini" title={m.mcp_webui_manual()} onclick={manualPick}>
              <FolderOpen size={13} />
            </button>
          </div>
        </div>
        <input bind:value={favicon} placeholder={m.mcp_webui_favicon_placeholder()} />
        {#if faviconNote}<div class="note">{faviconNote}</div>{/if}
      </div>
      {#if error}<div class="err">{error}</div>{/if}
    </div>
    <footer class="foot">
      <button class="btn danger" onclick={() => (confirmDelete = true)} disabled={!existingUrl}>
        <Trash2 size={13} /> {m.common_delete()}
      </button>
      <div class="spacer"></div>
      <button class="btn ghost" onclick={close}>{m.common_cancel()}</button>
      <button class="btn primary" onclick={save} disabled={saving}>{m.common_save()}</button>
    </footer>
  </div>
</div>

{#if confirmDelete}
  <div class="overlay sub" onkeydown={onKeydown} role="presentation">
    <div class="dialog small" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
      <div class="body">
        <div class="confirm">{m.mcp_webui_delete_confirm()}</div>
      </div>
      <footer class="foot">
        <div class="spacer"></div>
        <button class="btn ghost" onclick={() => (confirmDelete = false)}>{m.common_cancel()}</button>
        <button class="btn danger" onclick={doDelete}>{m.common_delete()}</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .overlay { position: fixed; inset: 0; z-index: 60; display: flex; align-items: center; justify-content: center; background-color: rgb(0 0 0 / 0.4); }
  .overlay.sub { z-index: 70; }
  .dialog { width: 480px; max-width: 90vw; max-height: 90vh; overflow: auto; display: flex; flex-direction: column; background: var(--background); border: 1px solid var(--border); border-radius: var(--radius-md); }
  .dialog.small { width: 360px; }
  .head { display: flex; align-items: center; justify-content: space-between; padding: 0.625rem 0.875rem; border-bottom: 1px solid var(--border); }
  .head-title { font-size: 0.875rem; font-weight: 500; }
  .x { display: inline-flex; color: var(--muted-foreground); background: transparent; border: none; border-radius: var(--radius-sm); cursor: default; }
  .x:hover { background: var(--accent); }
  .body { padding: 0.875rem 1rem; display: flex; flex-direction: column; gap: 0.6rem; }
  .foot { display: flex; align-items: center; gap: 0.5rem; padding: 0.625rem 0.875rem; border-top: 1px solid var(--border); }
  .spacer { flex: 1; }
  .field { display: flex; flex-direction: column; gap: 0.25rem; }
  .lbl { font-size: 0.72rem; color: var(--muted-foreground); }
  .favicon-row { display: flex; align-items: center; justify-content: space-between; }
  .favicon-actions { display: flex; gap: 0.3rem; }
  input { padding: 0.3rem 0.5rem; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--background); color: var(--foreground); font-size: 0.8125rem; }
  .mini { display: inline-flex; align-items: center; justify-content: center; height: 1.5rem; width: 1.5rem; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--background); color: var(--muted-foreground); }
  .mini:hover { background: var(--accent); color: var(--foreground); }
  .mini:disabled { opacity: 0.5; }
  .btn { display: inline-flex; align-items: center; gap: 0.25rem; padding: 0.3rem 0.6rem; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--background); color: var(--foreground); font-size: 0.75rem; }
  .btn:hover { background: var(--accent); }
  .btn:disabled { opacity: 0.5; }
  .btn.primary { background: var(--primary); color: var(--primary-foreground); border-color: var(--primary); }
  .btn.ghost { background: transparent; }
  .btn.danger { color: var(--destructive); border-color: var(--destructive); }
  .btn.danger:hover { background: var(--destructive); color: var(--destructive-foreground); }
  .note { font-size: 0.72rem; color: var(--destructive); }
  .err { font-size: 0.75rem; color: var(--destructive); }
  .confirm { font-size: 0.8125rem; }
  .spin { display: inline-flex; animation: mwu-spin 0.8s linear infinite; }
  @keyframes mwu-spin { to { transform: rotate(360deg); } }
</style>