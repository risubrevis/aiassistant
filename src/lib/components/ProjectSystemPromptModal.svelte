<script lang="ts">
  import { X } from "@lucide/svelte";
  import * as ipc from "$lib/tauri";
  import type { Project } from "$lib/tauri";
  import { projectSystemPromptOpen, projectSystemPromptId, projects } from "$lib/stores/project";
  import { m } from "$lib/i18n";
  import RichTextEditor from "./RichTextEditor.svelte";
  import { toast } from "$lib/stores/toasts";

  let project = $state<Project | null>(null);
  let includeGlobal = $state(false);
  let systemPromptText = $state("");
  let savedText = $state("");

  let open = $derived($projectSystemPromptOpen);
  let projectId = $derived($projectSystemPromptId);
  let dirty = $derived(systemPromptText !== savedText);

  $effect(() => {
    if (open && projectId) void load(projectId);
  });

  async function load(id: string) {
    try {
      const p = await ipc.projectGet(id);
      project = p;
      includeGlobal = p?.include_global_system_prompt === 1;
      systemPromptText = p?.system_prompt ?? "";
      savedText = systemPromptText;
    } catch (e) {
      console.error("projectSystemPrompt load failed", e);
    }
  }

  async function toggleInclude(checked: boolean) {
    if (!projectId) return;
    try {
      await ipc.projectSetIncludeGlobalSystemPrompt(projectId, checked);
      projects.update((list) =>
        list.map((x) =>
          x.id === projectId ? { ...x, include_global_system_prompt: checked ? 1 : 0 } : x,
        ),
      );
    } catch (e) {
      console.error("projectSetIncludeGlobalSystemPrompt failed", e);
      includeGlobal = !checked;
    }
  }

  async function save() {
    if (!projectId) return;
    try {
      await ipc.projectSetSystemPrompt(projectId, systemPromptText);
      savedText = systemPromptText;
      projects.update((list) =>
        list.map((x) => (x.id === projectId ? { ...x, system_prompt: systemPromptText } : x)),
      );
      toast.success(m.settings_prompts_saved());
      close();
    } catch (e) {
      console.error("projectSetSystemPrompt failed", e);
    }
  }

  function close() {
    projectSystemPromptOpen.set(false);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
</script>

{#if open}
  <div class="overlay" onkeydown={onKeydown} role="presentation">
    <div class="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
      <header class="head">
        <span class="title">{m.project_system_prompt_title()}</span>
        <button class="close" title={m.common_close()} onclick={close}><X size={16} /></button>
      </header>

      <div class="body">
        <div class="include-row">
          <input
            class="toggle"
            type="checkbox"
            bind:checked={includeGlobal}
            onchange={(e) => void toggleInclude(e.currentTarget.checked)}
          />
          <span>{m.project_system_prompt_include_global()}</span>
        </div>
        <div class="hint">{m.project_system_prompt_hint()}</div>

        <div class="field">
          <span class="lbl">{m.project_system_prompt()}</span>
          <RichTextEditor bind:value={systemPromptText} minHeight="220px" />
        </div>
      </div>

      <footer class="foot">
        <div class="spacer"></div>
        <button class="ghost" onclick={close}>{m.common_close()}</button>
        <button class="primary" disabled={!dirty} onclick={() => void save()}>
          {m.common_save()}
        </button>
      </footer>
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
    width: 560px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
    font-size: 0.875rem;
    font-weight: 500;
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
    gap: 0.6rem;
  }
  .include-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8125rem;
  }
  .hint {
    font-size: 0.7rem;
    color: var(--muted-foreground);
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
  .toggle {
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    accent-color: var(--primary);
    cursor: default;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 0.875rem;
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
  .foot button {
    padding: 0.3rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    font-size: 0.8125rem;
    cursor: default;
  }
  .foot button:disabled {
    opacity: 0.6;
  }
  .ghost {
    background: var(--background);
    color: var(--foreground);
  }
  .primary {
    background: var(--primary);
    color: var(--primary-foreground);
    border-color: var(--primary);
  }
</style>