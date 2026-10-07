<script lang="ts">
  import { X, Eye, FileText, Wrench, Copy } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { previewFirstRequest, type FirstRequestPreview } from "$lib/tauri";
  import { toast } from "$lib/stores/toasts";

  let {
    open,
    onclose,
  }: {
    open: boolean;
    onclose: () => void;
  } = $props();

  let preview = $state<FirstRequestPreview | null>(null);
  let loading = $state(false);
  let error = $state("");
  let tab = $state<"prompt" | "tools">("prompt");
  let dialogEl = $state<HTMLDivElement>();

  async function load() {
    loading = true;
    error = "";
    preview = null;
    try {
      preview = await previewFirstRequest();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // Fetch every time the modal opens so the preview reflects current settings.
  $effect(() => {
    if (open) void load();
  });

  $effect(() => {
    if (open) dialogEl?.focus();
  });

  async function copyPrompt() {
    if (!preview) return;
    try {
      await navigator.clipboard.writeText(preview.system_prompt);
      toast.success(m.message_copied());
    } catch {
      /* ignore */
    }
  }

  function fmtTokens(n: number): string {
    return n.toLocaleString("en-US");
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (open && e.key === "Escape") onclose();
  }}
/>

{#if open}
  <div
    class="overlay"
    onclick={onclose}
    onkeydown={() => {}}
    role="presentation"
  >
    <div
      class="dialog"
      tabindex="-1"
      bind:this={dialogEl}
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
      role="dialog"
    >
      <header class="head">
        <span class="title"><Eye size={15} /> {m.settings_prompts_preview_title()}</span>
        <button class="close" title={m.common_close()} onclick={onclose}><X size={16} /></button>
      </header>

      <div class="body">
        <p class="subtitle">{m.settings_prompts_preview_subtitle()}</p>

        {#if loading}
          <div class="state">{m.settings_prompts_preview_loading()}</div>
        {:else if error}
          <div class="state err">{error}</div>
        {:else if preview}
          {#if preview.system_prompt.length === 0}
            <div class="state">{m.settings_prompts_preview_empty()}</div>
          {/if}

          <div class="stats">
            <div class="stat">
              <span class="stat-label">{m.settings_prompts_preview_tokens_system()}</span>
              <span class="stat-val">{fmtTokens(preview.system_tokens)}</span>
            </div>
            <div class="stat">
              <span class="stat-label">{m.settings_prompts_preview_tokens_tools()}</span>
              <span class="stat-val">{fmtTokens(preview.tools_tokens)}</span>
            </div>
            <div class="stat total">
              <span class="stat-label">{m.settings_prompts_preview_tokens_total()}</span>
              <span class="stat-val">{fmtTokens(preview.total_tokens)}</span>
            </div>
            <span class="mode-badge">{preview.mode}</span>
          </div>
          <p class="estimate">{m.settings_prompts_preview_estimate()}</p>

          <div class="tabs">
            <button
              class="tab"
              class:active={tab === "prompt"}
              onclick={() => (tab = "prompt")}
            >
              <FileText size={13} /> {m.settings_prompts_preview_system()}
            </button>
            <button
              class="tab"
              class:active={tab === "tools"}
              onclick={() => (tab = "tools")}
            >
              <Wrench size={13} />
              {m.settings_prompts_preview_tools()} ({preview.tools.length})
            </button>
          </div>

          {#if tab === "prompt"}
            <div class="pane">
              {#if preview.system_prompt}
                <button class="copy-btn" onclick={() => void copyPrompt()} title={m.message_copy()}>
                  <Copy size={12} />
                </button>
                <pre class="prompt-text">{preview.system_prompt}</pre>
              {:else}
                <div class="state">{m.settings_prompts_preview_empty()}</div>
              {/if}
            </div>
          {:else}
            <div class="pane tools-pane">
              {#if preview.tools.length === 0}
                <div class="state">{m.settings_prompts_preview_empty()}</div>
              {:else}
                <ul class="tool-list">
                  {#each preview.tools as t (t.name)}
                    <li class="tool">
                      <code class="tool-name">{t.name}</code>
                      <span class="tool-desc">{t.description}</span>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
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
    width: 720px;
    max-width: 92vw;
    max-height: 86vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
    outline: none;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
    font-size: 0.875rem;
    font-weight: 500;
  }
  .title {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .close {
    display: inline-flex;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .close:hover {
    background: var(--accent);
  }
  .body {
    padding: 0.875rem 1rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  .subtitle {
    margin: 0;
    font-size: 0.75rem;
    color: var(--muted-foreground);
    line-height: 1.4;
  }
  .state {
    color: var(--muted-foreground);
    font-size: 0.8125rem;
    padding: 0.5rem 0;
  }
  .state.err {
    color: var(--destructive);
  }
  .stats {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .stat {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    font-size: 0.75rem;
  }
  .stat-label {
    color: var(--muted-foreground);
  }
  .stat-val {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .stat.total {
    border-color: var(--primary);
    color: var(--primary);
  }
  .mode-badge {
    margin-left: auto;
    text-transform: capitalize;
    font-size: 0.7rem;
    color: var(--muted-foreground);
    border: 1px solid var(--border);
    border-radius: 9999px;
    padding: 0.1rem 0.55rem;
  }
  .estimate {
    margin: 0;
    font-size: 0.7rem;
    color: var(--muted-foreground);
    line-height: 1.4;
  }
  .tabs {
    display: flex;
    gap: 0.25rem;
    border-bottom: 1px solid var(--border);
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.35rem 0.7rem;
    border: none;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--muted-foreground);
    font-size: 0.78rem;
    cursor: default;
  }
  .tab:hover {
    color: var(--foreground);
  }
  .tab.active {
    color: var(--foreground);
    border-bottom-color: var(--primary);
  }
  .pane {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .prompt-text {
    margin: 0;
    flex: 1;
    max-height: 48vh;
    overflow: auto;
    padding: 0.6rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card);
    color: var(--foreground);
    font-family: var(--font-mono);
    font-size: 0.75rem;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .copy-btn {
    position: absolute;
    top: 0.4rem;
    right: 0.5rem;
    z-index: 2;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--muted-foreground);
    cursor: default;
  }
  .copy-btn:hover {
    background: var(--accent);
    color: var(--foreground);
  }
  .tools-pane {
    overflow: hidden;
  }
  .tool-list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 48vh;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .tool {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.45rem 0.6rem;
    border-bottom: 1px solid var(--border);
  }
  .tool:last-child {
    border-bottom: none;
  }
  .tool-name {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--foreground);
  }
  .tool-desc {
    font-size: 0.72rem;
    color: var(--muted-foreground);
    line-height: 1.4;
  }
</style>