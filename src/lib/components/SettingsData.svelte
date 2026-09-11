<script lang="ts">
  import { onMount } from "svelte";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { RefreshCw, LoaderCircle, Trash2 } from "@lucide/svelte";
  import * as ipc from "$lib/tauri";
  import { toast } from "$lib/stores/toasts";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { m } from "$lib/i18n";

  let paths = $state<ipc.AppPaths | null>(null);
  let clearing = $state(false);
  let resetOpen = $state(false);
  let resetting = $state(false);

  async function clearCache() {
    clearing = true;
    try {
      const removed = await ipc.clearCache();
      if (removed > 0) {
        toast.success(m.settings_data_clear_cache_done());
      } else {
        toast.info(m.settings_data_clear_cache_empty());
      }
    } catch (e) {
      toast.error(String(e));
    } finally {
      clearing = false;
    }
  }

  async function doReset() {
    resetting = true;
    try {
      await ipc.resetApp();
      await relaunch();
    } catch (e) {
      toast.error(String(e));
      resetting = false;
      resetOpen = false;
    }
  }

  onMount(async () => {
    try {
      paths = await ipc.appPaths();
    } catch (e) {
      console.error("appPaths failed", e);
    }
  });
</script>

<div class="data">
  <div class="section-heading">{m.settings_about_data_storage()}</div>
  <div class="rows">
    <div class="row">
      <span class="label">{m.settings_about_config_path()}</span>
      <span class="value mono" title={paths?.config_path ?? ""}>{paths?.config_path ?? "—"}</span>
    </div>
    <div class="row">
      <span class="label">{m.settings_about_data_dir()}</span>
      <span class="value mono" title={paths?.data_dir ?? ""}>{paths?.data_dir ?? "—"}</span>
    </div>
    <div class="row">
      <span class="label">{m.settings_about_log_path()}</span>
      <span class="value mono" title={paths?.log_path ?? ""}>{paths?.log_path ?? "—"}</span>
    </div>
  </div>

  <div class="section-heading">{m.settings_data_clear_cache()}</div>
  <div class="action-row">
    <span class="hint">{m.settings_data_clear_cache_hint()}</span>
    <button class="chip" onclick={() => void clearCache()} disabled={clearing}>
      {#if clearing}
        <span class="spin"><LoaderCircle size={12} /></span>
      {:else}
        <RefreshCw size={12} />
      {/if}
      {m.settings_data_clear_cache()}
    </button>
  </div>

  <div class="section-heading">{m.settings_data_reset_title()}</div>
  <div class="action-row">
    <span class="hint">{m.settings_data_reset_hint()}</span>
    <button class="chip danger" onclick={() => (resetOpen = true)} disabled={resetting}>
      <Trash2 size={12} /> {m.settings_data_reset_button()}
    </button>
  </div>

  <ConfirmDialog
    open={resetOpen}
    message={m.settings_data_reset_confirm()}
    confirmLabel={m.settings_data_reset_confirm_label()}
    variant="danger"
    loading={resetting}
    loadingLabel={m.settings_data_resetting()}
    oncancel={() => (resetOpen = false)}
    onconfirm={() => void doReset()}
  />
</div>

<style>
  .data {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .section-heading {
    font-size: 0.8125rem;
    font-weight: 600;
    margin-top: 0.25rem;
  }
  .rows {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    padding: 0.4rem 0.625rem;
    font-size: 0.75rem;
  }
  .row + .row {
    border-top: 1px solid var(--border);
  }
  .label {
    width: 130px;
    flex-shrink: 0;
    color: var(--muted-foreground);
  }
  .value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 0.6875rem;
  }

  .action-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 0.625rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .hint {
    flex: 1;
    min-width: 0;
    font-size: 0.6875rem;
    color: var(--muted-foreground);
    line-height: 1.45;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.25rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.75rem;
    cursor: default;
  }
  .chip:disabled {
    opacity: 0.5;
  }
  .chip:hover:not(:disabled) {
    background: var(--accent);
  }
  .chip.danger {
    background: var(--destructive);
    border-color: var(--destructive);
    color: var(--destructive-foreground);
  }
  .chip.danger:hover:not(:disabled) {
    filter: brightness(0.92);
    background: var(--destructive);
  }
  .spin {
    display: inline-flex;
    animation: spin-anim 1s linear infinite;
  }
  @keyframes spin-anim {
    to {
      transform: rotate(360deg);
    }
  }
</style>