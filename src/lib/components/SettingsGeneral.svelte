<script lang="ts">
  import { onMount } from "svelte";
  import { check, type Update, type DownloadEvent } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { RefreshCw, Download, Loader, ExternalLink, Check } from "@lucide/svelte";
  import * as ipc from "$lib/tauri";
  import { config } from "$lib/stores/config";
  import { toast } from "$lib/stores/toasts";
  import {
    loadLanguageSetting,
    saveLanguageSetting,
    applyLanguageSetting,
    SUPPORTED_LOCALES,
    type LanguageSetting,
    type Locale,
  } from "$lib/stores/lang";
  import { m } from "$lib/i18n";
  import Select from "./Select.svelte";

  const FALLBACK_VERSION = "1.0.0";
  const MIB = 1048576;
  const DOWNLOAD_URL = "https://agnostic-ai-assistant.com/download/";

  // --- language ---

  const languageOptions: { value: LanguageSetting; label: string }[] = [
    { value: "system", label: m.settings_appearance_language_system() },
    ...(Object.keys(SUPPORTED_LOCALES) as Locale[]).map((code) => ({
      value: code,
      label: SUPPORTED_LOCALES[code],
    })),
  ];

  let langSetting = $state<LanguageSetting>("system");

  async function chooseLanguage(value: string) {
    const setting: LanguageSetting =
      value in SUPPORTED_LOCALES ? (value as Locale) : "system";
    langSetting = setting;
    saveLanguageSetting(setting);
    // paraglide reloads this window; notify the main window to reload too.
    await ipc.emitLangChanged();
    await applyLanguageSetting(setting);
  }

  // --- preferences ---

  type Prefs = {
    sendOnEnter: boolean;
    notificationsEnabled: boolean;
    rememberWindow: boolean;
  };

  let draft = $state<Prefs>({
    sendOnEnter: true,
    notificationsEnabled: true,
    rememberWindow: true,
  });
  let userEdited = $state(false);
  let saving = $state(false);
  let justSaved = $state(false);
  let savedTimer: ReturnType<typeof setTimeout> | null = null;

  const saved = $derived.by(() => {
    const g = $config?.general;
    if (!g) return null;
    return {
      sendOnEnter: g.send_on_enter,
      notificationsEnabled: g.notifications_enabled,
      rememberWindow: g.remember_window_state,
    } satisfies Prefs;
  });

  const dirty = $derived.by(() => {
    const s = saved;
    if (!s) return false;
    return (
      draft.sendOnEnter !== s.sendOnEnter ||
      draft.notificationsEnabled !== s.notificationsEnabled ||
      draft.rememberWindow !== s.rememberWindow
    );
  });

  // Follow the saved config until the user edits, re-sync after Save/Cancel.
  $effect(() => {
    const s = saved;
    if (!s || userEdited) return;
    draft = { ...s };
  });

  function togglePref(key: keyof Prefs) {
    draft[key] = !draft[key];
    userEdited = true;
  }

  function cancelPrefs() {
    if (saved) draft = { ...saved };
    userEdited = false;
  }

  async function savePrefs() {
    if (!dirty || saving) return;
    saving = true;
    try {
      await ipc.setSendOnEnter(draft.sendOnEnter);
      await ipc.setNotificationsEnabled(draft.notificationsEnabled);
      await ipc.setRememberWindowState(draft.rememberWindow);
      userEdited = false;
      justSaved = true;
      if (savedTimer) clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (justSaved = false), 1500);
    } catch (e) {
      toast.error(String(e));
    } finally {
      saving = false;
    }
  }

  // --- updates ---

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

  // Wrapped so the import is referenced in the module body (SSR build drops
  // inline onclick handlers, which would otherwise flag `openUrl` as unused).
  function openExternal(url: string) {
    void openUrl(url);
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
    langSetting = loadLanguageSetting();
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

{#snippet prefRow(key: keyof Prefs, label: string, hint: string)}
  <div class="pref-row">
    <button
      type="button"
      role="switch"
      aria-checked={draft[key]}
      aria-label={label}
      class="switch"
      class:on={draft[key]}
      onclick={() => togglePref(key)}
      disabled={saving}
    >
      <span class="knob"></span>
    </button>
    <div class="pref-text">
      <div class="pref-label">{label}</div>
      <div class="pref-hint">{hint}</div>
    </div>
  </div>
{/snippet}

<div class="general">
  <div class="section">
    <div class="section-heading">{m.settings_appearance_language()}</div>
    <Select
      class="w-full max-w-45"
      value={langSetting}
      items={languageOptions}
      onchange={(v) => void chooseLanguage(v)}
    />
  </div>

  <div class="section">
    <div class="section-heading">{m.settings_general_preferences()}</div>
    <div class="prefs">
      {@render prefRow(
        "sendOnEnter",
        m.settings_general_send_on_enter(),
        m.settings_general_send_on_enter_hint(),
      )}
      {@render prefRow(
        "notificationsEnabled",
        m.settings_general_notifications(),
        m.settings_general_notifications_hint(),
      )}
      {@render prefRow(
        "rememberWindow",
        m.settings_general_remember_window(),
        m.settings_general_remember_window_hint(),
      )}
    </div>
    <div class="prefs-footer">
      {#if justSaved}<span class="saved-check"><Check size={12} /></span>{/if}
      <button class="btn" onclick={cancelPrefs} disabled={!dirty || saving}>
        {m.common_cancel()}
      </button>
      <button class="btn primary" onclick={() => void savePrefs()} disabled={!dirty || saving}>
        {m.common_save()}
      </button>
    </div>
  </div>

  <div class="section">
    <div class="section-heading">{m.settings_about_updates()}</div>
    <div class="updates">
      <div class="updates-top">
        <div class="hint">{m.settings_about_updates_description()}</div>
        <button
          class="chip"
          onclick={() => void checkForUpdates()}
          disabled={checking || downloading || installing}
        >
          <span class:spin={checking}><RefreshCw size={12} /></span>
          {m.settings_about_updates_check()}
        </button>
      </div>

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
            <span class="muted">
              {#if contentLength && contentLength > 0}{progress}% · {/if}{(downloaded / MIB).toFixed(1)} MiB
            </span>
          </div>
          <div class="progress">
            <div
              class="bar"
              class:indeterminate={!(contentLength && contentLength > 0)}
              style="width: {contentLength && contentLength > 0 ? `${progress}%` : '40%'}"
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
            <button class="chip" onclick={() => openExternal(DOWNLOAD_URL)}>
              <ExternalLink size={12} /> {m.settings_about_updates_open_download()}
            </button>
          {/if}
        </div>
        {#if !installSupported}
          <div class="muted note">{m.settings_about_updates_not_supported()}</div>
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .general {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .section-heading {
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .prefs {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }
  .pref-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.55rem 0.625rem;
  }
  .pref-row + .pref-row {
    border-top: 1px solid var(--border);
  }
  .pref-text {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }
  .pref-label {
    font-size: 0.8125rem;
  }
  .pref-hint {
    font-size: 0.6875rem;
    color: var(--muted-foreground);
    line-height: 1.45;
  }
  .switch {
    flex-shrink: 0;
    position: relative;
    width: 36px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 999px;
    background: var(--border);
    cursor: default;
    transition: background 0.15s;
  }
  .switch.on {
    background: var(--primary);
  }
  .switch:disabled {
    opacity: 0.6;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 999px;
    background: var(--background);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
    transition: transform 0.15s;
  }
  .switch.on .knob {
    transform: translateX(16px);
    background: var(--primary-foreground);
  }

  .prefs-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  .saved-check {
    display: inline-flex;
    color: hsl(142 71% 45%);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.78rem;
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
  }

  .updates {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.625rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
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
    background: var(--primary);
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
  }
  .updates-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0.5rem;
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