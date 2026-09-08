<script lang="ts">
  import { X, Plus, Trash2, Pencil, ArrowUp, ArrowDown } from "@lucide/svelte";
  import * as ipc from "$lib/tauri";
  import type { Project, ProjectRule } from "$lib/tauri";
  import { projectRulesOpen, projectRulesId } from "$lib/stores/project";
  import { m } from "$lib/i18n";
  import RichTextEditor from "./RichTextEditor.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";

  let project = $state<Project | null>(null);
  let includeGlobal = $state(false);
  let rules = $state<ProjectRule[]>([]);

  let ruleModalOpen = $state(false);
  let ruleModalMode = $state<"create" | "edit">("create");
  let ruleModalId = $state<string | null>(null);
  let formTitle = $state("");
  let formText = $state("");

  let deleteTarget = $state<ProjectRule | null>(null);

  let open = $derived($projectRulesOpen);
  let projectId = $derived($projectRulesId);

  $effect(() => {
    if (open && projectId) void load(projectId);
  });

  async function load(id: string) {
    try {
      const p = await ipc.projectGet(id);
      project = p;
      includeGlobal = p?.include_global_rules === 1;
      rules = await ipc.projectRulesList(id);
    } catch (e) {
      console.error("projectRules load failed", e);
    }
  }

  async function toggleInclude(checked: boolean) {
    if (!projectId) return;
    try {
      await ipc.projectSetIncludeGlobalRules(projectId, checked);
    } catch (e) {
      console.error("projectSetIncludeGlobalRules failed", e);
    }
  }

  async function toggleActive(r: ProjectRule, active: boolean) {
    try {
      await ipc.projectRuleSetActive(r.id, active);
    } catch (e) {
      console.error("projectRuleSetActive failed", e);
    }
    if (projectId) await load(projectId);
  }

  async function move(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= rules.length || !projectId) return;
    const next = [...rules];
    [next[i], next[j]] = [next[j], next[i]];
    rules = next;
    try {
      await ipc.projectRuleReorder(projectId, next.map((r) => r.id));
    } catch (e) {
      console.error("projectRuleReorder failed", e);
    }
    await load(projectId);
  }

  function openCreate() {
    ruleModalMode = "create";
    ruleModalId = null;
    formTitle = "";
    formText = "";
    ruleModalOpen = true;
  }

  function openEdit(r: ProjectRule) {
    ruleModalMode = "edit";
    ruleModalId = r.id;
    formTitle = r.title;
    formText = r.text;
    ruleModalOpen = true;
  }

  function closeRuleModal() {
    ruleModalOpen = false;
  }

  async function saveRuleModal() {
    if (!projectId) return;
    const title = formTitle.trim();
    const text = formText.trim();
    if (!text) return;
    try {
      if (ruleModalMode === "create") {
        await ipc.projectRuleCreate(projectId, title, text);
      } else if (ruleModalId) {
        await ipc.projectRuleUpdate(ruleModalId, title, text);
      }
      ruleModalOpen = false;
      await load(projectId);
    } catch (e) {
      console.error("projectRule save failed", e);
    }
  }

  async function confirmDelete() {
    const r = deleteTarget;
    if (!r || !projectId) return;
    try {
      await ipc.projectRuleDelete(r.id);
      deleteTarget = null;
      await load(projectId);
    } catch (e) {
      console.error("projectRuleDelete failed", e);
    }
  }

  function close() {
    projectRulesOpen.set(false);
  }

  function onKeydown(e: KeyboardEvent) {
    if (ruleModalOpen) {
      closeRuleModal();
      return;
    }
    if (e.key === "Escape") close();
  }

  function onRuleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      closeRuleModal();
    }
  }
</script>

{#if open}
  <div class="overlay" onkeydown={onKeydown} role="presentation">
    <div class="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
      <header class="head">
        <span class="title">{m.project_rules_title()}</span>
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
          <span>{m.project_rules_include_global()}</span>
        </div>

        <div class="section">
          <div class="sec-head">
            <span>{m.project_rules_title()}</span>
            <button class="mini" onclick={openCreate}><Plus size={12} /> {m.project_rules_add()}</button>
          </div>
          {#if rules.length === 0}
            <div class="empty">{m.project_rules_empty()}</div>
          {/if}
          {#each rules as r, i (r.id)}
            <div class="rule-row">
              <input
                class="toggle"
                type="checkbox"
                checked={r.is_active === 1}
                onchange={(e) => void toggleActive(r, e.currentTarget.checked)}
              />
              <button class="icon-btn" disabled={i === 0} onclick={() => void move(i, -1)}>
                <ArrowUp size={12} />
              </button>
              <button class="icon-btn" disabled={i === rules.length - 1} onclick={() => void move(i, 1)}>
                <ArrowDown size={12} />
              </button>
              <div class="rule-main">
                <span class="rule-title" title={r.title || r.text}>{r.title || r.text}</span>
                {#if r.title}
                  <span class="rule-preview" title={r.text}>{r.text}</span>
                {/if}
              </div>
              <button class="icon-btn" title={m.common_edit()} onclick={() => openEdit(r)}>
                <Pencil size={12} />
              </button>
              <button class="icon-btn danger" title={m.common_delete()} onclick={() => (deleteTarget = r)}>
                <Trash2 size={12} />
              </button>
            </div>
          {/each}
        </div>
      </div>

      <footer class="foot">
        <div class="spacer"></div>
        <button class="ghost" onclick={close}>{m.common_close()}</button>
      </footer>
    </div>
  </div>

  {#if ruleModalOpen}
    <div class="overlay rule-overlay" onkeydown={onRuleKeydown} role="presentation">
      <div class="dialog rule-dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onRuleKeydown} role="dialog">
        <header class="head">
          <span class="title">{ruleModalMode === "create" ? m.project_rules_add() : m.project_rules_edit()}</span>
          <button class="close" title={m.common_close()} onclick={closeRuleModal}><X size={16} /></button>
        </header>
        <div class="body">
          <div class="field">
            <span class="lbl">{m.project_rules_field_title()}</span>
            <input bind:value={formTitle} placeholder={m.project_rules_field_title()} />
          </div>
          <div class="field">
            <span class="lbl">{m.project_rules_field_text()}</span>
            <RichTextEditor bind:value={formText} minHeight="120px" />
          </div>
        </div>
        <footer class="foot">
          <div class="spacer"></div>
          <button class="ghost" onclick={closeRuleModal}>{m.common_cancel()}</button>
          <button class="primary" disabled={!formText.trim()} onclick={() => void saveRuleModal()}>
            {m.common_save()}
          </button>
        </footer>
      </div>
    </div>
  {/if}

  <ConfirmDialog
    open={deleteTarget !== null}
    message={m.project_rules_delete_confirm()}
    onconfirm={() => void confirmDelete()}
    oncancel={() => (deleteTarget = null)}
  />
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
  .section {
    border-top: 1px solid var(--border);
    padding-top: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 0.75rem;
    font-weight: 600;
    margin-bottom: 0.2rem;
  }
  .mini {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.2rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.72rem;
    cursor: default;
  }
  .mini:hover {
    background: var(--accent);
  }
  .rule-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.78rem;
    min-width: 0;
  }
  .rule-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
    overflow: hidden;
  }
  .rule-title {
    flex-shrink: 0;
    max-width: 60%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rule-preview {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted-foreground);
    font-size: 0.7rem;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    padding: 0.2rem;
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
  .empty {
    font-size: 0.78rem;
    color: var(--muted-foreground);
    padding: 0.25rem 0;
  }
  .rule-overlay {
    z-index: 70;
  }
  .rule-dialog {
    width: 420px;
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