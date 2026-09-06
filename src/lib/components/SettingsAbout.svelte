<script lang="ts">
  import { onMount } from "svelte";
  import { check, type Update, type DownloadEvent } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { RefreshCw, Download, Loader, ExternalLink, Check } from "@lucide/svelte";
  import * as ipc from "$lib/tauri";
  import { m } from "$lib/i18n";

  const FALLBACK_VERSION = "1.0.0";
  const RELEASES_URL = "https://github.com/risubrevis/aiassistant/releases/latest";
  const MIB = 1048576;

  let paths = $state<ipc.AppPaths | null>(null);
  let currentVersion = $state(FALLBACK_VERSION);
  let installSupported = $state(false);
  let checking = $state(false);
  let update = $state<Update | null>(null);
  let upToDate = $state(false);
  let commits = $state<ipc.CommitEntry[]>([]);
  let commitError = $state(false);
  let downloading = $state(false);
  let progress = $state(0);
  let contentLength = $state<number | null>(null);
  let downloaded = $state(0);
  let installing = $state(false);
  let error = $state<string | null>(null);

  function formatDate(date?: string): string {
    if (!date) return "";
    const ms = Date.parse(date);
    return Number.isNaN(ms) ? date : new Date(ms).toLocaleDateString();
  }

  async function checkForUpdates() {
    checking = true;
    error = null;
    update = null;
    upToDate = false;
    commits = [];
    commitError = false;
    try {
      const u = await check();
      if (u) {
        update = u;
        try {
          commits = await ipc.updateChangelog(currentVersion, u.version);
        } catch (e) {
          console.error("updateChangelog failed", e);
          commits = [];
          commitError = true;
        }
      } else {
        upToDate = true;
      }
    } catch (e) {
      error = String(e);
    } finally {
      checking = false;
    }
  }

  async function downloadAndInstall() {
    if (!update) return;
    downloading = true;
    progress = 0;
    downloaded = 0;
    contentLength = null;
    installing = false;
    error = null;
    try {
      await update.downloadAndInstall((e: DownloadEvent) => {
        if (e.event === "Started") {
          contentLength = e.data.contentLength ?? null;
        } else if (e.event === "Progress") {
          downloaded += e.data.chunkLength;
          if (contentLength && contentLength > 0) {
            progress = Math.min(100, Math.round((downloaded / contentLength) * 100));
          }
        } else if (e.event === "Finished") {
          progress = 100;
        }
      });
      // Reached only on macOS/Linux; on Windows the installer exits the app.
      installing = true;
      await relaunch();
    } catch (e) {
      error = String(e);
    } finally {
      downloading = false;
    }
  }

  onMount(async () => {
    try {
      paths = await ipc.appPaths();
    } catch (e) {
      console.error("appPaths failed", e);
    }
    try {
      currentVersion = await getVersion();
    } catch (e) {
      console.error("getVersion failed", e);
      currentVersion = FALLBACK_VERSION;
    }
    try {
      installSupported = await ipc.updateInstallSupported();
    } catch (e) {
      console.error("updateInstallSupported failed", e);
    }
  });
</script>

<div class="about">
  <div class="name">{m.app_name()}</div>
  <div class="desc">{m.settings_about_description()}</div>

  <div class="rows">
    <div class="row">
      <span class="label">{m.settings_about_version()}</span>
      <span class="value">{currentVersion}</span>
    </div>
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

  <div class="updates">
    <div class="row">
      <span class="title">{m.settings_about_updates()}</span>
      <button
        class="chip"
        onclick={() => void checkForUpdates()}
        disabled={checking || downloading || installing}
      >
        <span class:spin={checking}><RefreshCw size={12} /></span>
        {m.settings_about_updates_check()}
      </button>
    </div>
    <div class="hint">{m.settings_about_updates_description()}</div>

    {#if checking}
      <div class="status muted">{m.settings_about_updates_checking()}</div>
    {:else if upToDate}
      <div class="status">
        <span class="ok"><Check size={12} /></span> {m.settings_about_updates_up_to_date()}
      </div>
    {:else if update}
      <div class="status">
        <span class="strong">{m.settings_about_updates_available()} v{update.version}</span>
        {#if update.date}
          <span class="muted">{m.settings_about_updates_date()} {formatDate(update.date)}</span>
        {/if}
      </div>
    {/if}

    {#if error}
      <div class="error">{m.settings_about_updates_error()} {error}</div>
    {/if}

    {#if update?.body}
      <div class="notes">
        <div class="heading">{m.settings_about_updates_notes()}</div>
        <p>{update.body}</p>
      </div>
    {/if}

    {#if update}
      <div class="changes">
        <div class="heading">{m.settings_about_updates_changes()}</div>
        {#if commitError || commits.length === 0}
          <div class="muted">{m.settings_about_updates_no_changes()}</div>
        {:else}
          <ul class="commits">
            {#each commits as c (c.sha)}
              <li>
                <span class="sha">{c.sha.slice(0, 7)}</span>
                <span class="msg">{c.message}</span>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}

    {#if downloading}
      <div class="download">
        <div class="dl-label">
          <span>{m.settings_about_updates_downloading()}</span>
          {#if contentLength && contentLength > 0}
            <span class="muted">{progress}%</span>
          {:else}
            <span class="muted">{(downloaded / MIB).toFixed(1)} MiB</span>
          {/if}
        </div>
        <div class="progress">
          <div
            class="bar"
            class:indeterminate={!contentLength}
            style="width: {contentLength ? `${progress}%` : '40%'}"
          ></div>
        </div>
      </div>
    {:else if installing}
      <div class="status">
        <span class="spin"><Loader size={12} /></span> {m.settings_about_updates_installing()}
      </div>
    {/if}

    {#if update && !downloading && !installing}
      <div class="actions">
        {#if installSupported}
          <button class="chip primary" onclick={() => void downloadAndInstall()}>
            <Download size={12} /> {m.settings_about_updates_download_install()}
          </button>
        {:else}
          <button class="chip" onclick={() => void openUrl(RELEASES_URL)}>
            <ExternalLink size={12} /> {m.settings_about_updates_open_releases()}
          </button>
        {/if}
      </div>
      {#if !installSupported}
        <div class="muted note">{m.settings_about_updates_not_supported()}</div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .about {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .name {
    font-size: 0.9375rem;
    font-weight: 600;
  }
  .desc {
    font-size: 0.8125rem;
    color: var(--muted-foreground);
    max-width: 46ch;
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

  .updates {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.625rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .updates .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 0;
    font-size: 0.8125rem;
  }
  .title {
    font-weight: 500;
    font-size: 0.8125rem;
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
  .chip.primary {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .chip.primary:hover:not(:disabled) {
    background: var(--primary);
    opacity: 0.9;
  }
  .hint {
    font-size: 0.6875rem;
    color: var(--muted-foreground);
  }
  .status {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.35rem 0.5rem;
    font-size: 0.8125rem;
  }
  .muted {
    color: var(--muted-foreground);
  }
  .ok,
  .spin {
    display: inline-flex;
  }
  .spin {
    animation: spin-anim 1s linear infinite;
  }
  .strong {
    font-weight: 500;
  }
  .heading {
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--muted-foreground);
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .notes p {
    margin: 0;
    padding: 0.5rem 0.625rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--muted);
    font-size: 0.75rem;
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .changes {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .commits {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.75rem;
    max-height: 180px;
    overflow-y: auto;
  }
  .commits li {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
  }
  .sha {
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    color: var(--muted-foreground);
    flex-shrink: 0;
  }
  .msg {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .download {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .dl-label {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    font-size: 0.6875rem;
  }
  .progress {
    height: 6px;
    background: var(--border);
    border-radius: 999px;
    overflow: hidden;
  }
  .bar {
    height: 100%;
    background: var(--accent, #3b82f6);
    transition: width 0.15s;
  }
  .bar.indeterminate {
    animation: indeterminate 1.2s ease-in-out infinite;
  }
  .error {
    color: var(--destructive);
    font-size: 0.75rem;
  }
  .actions {
    display: flex;
    gap: 0.35rem;
  }
  .note {
    font-size: 0.6875rem;
    max-width: 60ch;
  }
  @keyframes spin-anim {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes indeterminate {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(250%);
    }
  }
</style>