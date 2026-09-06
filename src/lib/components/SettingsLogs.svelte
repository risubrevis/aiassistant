<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import * as ipc from "$lib/tauri";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { m } from "$lib/i18n";
  import { RefreshCw, FolderOpen, Trash2, Loader } from "@lucide/svelte";
  import Select from "./Select.svelte";

  const LEVELS = ["trace", "debug", "info", "warn", "error", "off"];
  const levelItems = LEVELS.map((v) => ({ value: v, label: v }));

  let logPath = $state("");
  let content = $state("");
  let loading = $state(false);
  let clearing = $state(false);
  let error = $state<string | null>(null);
  let logEl: HTMLPreElement | undefined = $state();

  let consoleLevel = $state("info");
  let fileLevel = $state("debug");
  let applying = $state(false);
  let applied = $state(false);
  let redactSecrets = $state(true);
  let appliedTimer: ReturnType<typeof setTimeout> | undefined;

  async function refresh() {
    loading = true;
    error = null;
    try {
      content = (await ipc.logsRead(500)) ?? "";
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    } catch (e) {
      content = "";
      error = String(e);
      console.error("logsRead failed", e);
    } finally {
      loading = false;
    }
  }

  async function openLog() {
    if (!logPath) return;
    try {
      await openPath(logPath);
    } catch (e) {
      console.error("openPath failed", e);
    }
  }

  async function clearLog() {
    clearing = true;
    try {
      await ipc.logsClear();
      await refresh();
    } catch (e) {
      console.error("logsClear failed", e);
    } finally {
      clearing = false;
    }
  }

  async function applyLevels() {
    applying = true;
    try {
      await ipc.setLogLevel(consoleLevel, fileLevel);
      applied = true;
      clearTimeout(appliedTimer);
      appliedTimer = setTimeout(() => (applied = false), 1500);
    } catch (e) {
      console.error("setLogLevel failed", e);
    } finally {
      applying = false;
    }
  }

  onMount(async () => {
    try {
      const paths = await ipc.appPaths();
      logPath = paths.log_path ?? "";
    } catch (e) {
      console.error("appPaths failed", e);
    }
    try {
      const config = await ipc.configGet();
      consoleLevel = config.logging.level.toLowerCase();
      fileLevel = config.logging.file_level.toLowerCase();
      redactSecrets = config.logging.redact_secrets;
    } catch (e) {
      console.error("configGet failed", e);
    }
    await refresh();
  });

  onDestroy(() => clearTimeout(appliedTimer));
</script>

<div class="logs">
  <div class="row">
    <span class="title">{m.settings_logs_title()}</span>
    <span class="actions">
      <button class="chip" onclick={() => void openLog()} disabled={!logPath} title={m.settings_logs_open()}>
        <FolderOpen size={12} /> {m.settings_logs_open()}
      </button>
      <button class="chip" onclick={() => void clearLog()} disabled={clearing} title={m.settings_logs_clear_tooltip()}>
        <Trash2 size={12} /> {m.settings_logs_clear()}
      </button>
      <button class="chip" onclick={() => void refresh()} disabled={loading}>
        <span class:spin={loading}><RefreshCw size={12} /></span> {m.settings_logs_refresh()}
      </button>
    </span>
  </div>

  <div class="controls">
    <div class="ctl">
      <span class="label">{m.settings_logs_console_level()}</span>
      <span class="ctl-sel">
        <Select
          value={consoleLevel}
          items={levelItems}
          onchange={(v) => {
            consoleLevel = v;
            void applyLevels();
          }}
        />
      </span>
    </div>
    <div class="ctl">
      <span class="label">{m.settings_logs_file_level()}</span>
      <span class="ctl-sel">
        <Select
          value={fileLevel}
          items={levelItems}
          onchange={(v) => {
            fileLevel = v;
            void applyLevels();
          }}
        />
      </span>
    </div>
    {#if applying}
      <span class="applying"><Loader size={12} /></span>
    {/if}
    {#if applied}
      <span class="hint">{m.settings_logs_applied()}</span>
    {/if}
    <span class="hint" title="Read-only: edit logging.redact_secrets in config.toml">
      Redact secrets: {redactSecrets ? "on" : "off"}
    </span>
  </div>

  {#if logPath}
    <div class="path" title={logPath}>{logPath}</div>
  {/if}

  {#if error}
    <div class="err">{error}</div>
  {/if}

  {#if content}
    <pre bind:this={logEl}>{content}</pre>
  {:else if !loading && !error}
    <div class="empty">{m.settings_logs_empty()}</div>
  {/if}
</div>

<style>
  .logs {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .title {
    font-weight: 500;
    font-size: 0.8125rem;
  }
  .actions {
    display: flex;
    gap: 0.35rem;
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
  .controls {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.4rem 1rem;
  }
  .ctl {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
  }
  .label {
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .ctl-sel {
    display: inline-flex;
  }
  .applying {
    display: inline-flex;
    color: var(--muted-foreground);
    animation: spin-anim 1s linear infinite;
  }
  .hint {
    font-size: 0.6875rem;
    color: var(--muted-foreground);
  }
  .path {
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    color: var(--muted-foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0.3rem 0.5rem;
    border: 1px dashed var(--border);
    border-radius: var(--radius-sm);
  }
  .logs pre {
    margin: 0;
    padding: 0.5rem 0.625rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--muted);
    color: var(--foreground);
    font-family: var(--font-mono);
    font-size: 0.7rem;
    line-height: 1.45;
    max-height: 300px;
    overflow-y: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .empty {
    padding: 1rem;
    color: var(--muted-foreground);
    font-size: 0.8125rem;
  }
  .err {
    color: var(--destructive);
    font-size: 0.75rem;
  }
  .spin {
    animation: spin-anim 1s linear infinite;
  }
  @keyframes spin-anim {
    to {
      transform: rotate(360deg);
    }
  }
</style>