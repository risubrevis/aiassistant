<script lang="ts">
  import { onMount } from "svelte";
  import { toast } from "$lib/stores/toasts";
  import { X, Plus, Trash2, Pencil } from "@lucide/svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import * as ipc from "$lib/tauri";
  import type { Project, ProjectPath, Rule } from "$lib/tauri";
  import { projectSettingsOpen, projectSettingsId, deleteProject, projects } from "$lib/stores/project";
  import { m } from "$lib/i18n";
  import Select from "./Select.svelte";
  import ColorPicker from "./ColorPicker.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";

  let project = $state<Project | null>(null);
  let paths = $state<ProjectPath[]>([]);
  let rules = $state<Rule[]>([]);
  let name = $state("");
  let color = $state("#6366f1");
  let description = $state("");
  let systemPrompt = $state("");
  let crossChat = $state("off");
  let ruleModalOpen = $state(false);
  let ruleModalMode = $state<"create" | "edit">("create");
  let ruleModalId = $state<string | null>(null);
  let ruleModalTitle = $state("");
  let ruleModalBody = $state("");
  let ruleArea = $state<HTMLTextAreaElement | null>(null);
  let ragChunks = $state(0);
  let ragBusy = $state(false);
  let confirmDeleteOpen = $state(false);
  let deleting = $state(false);

  const CROSS_CHAT_MODES = ["off", "summary", "retrieval", "hybrid"] as const;

  function crossChatLabel(mode: (typeof CROSS_CHAT_MODES)[number]) {
    return mode === "off"
      ? m.project_cross_chat_off()
      : mode === "summary"
        ? m.project_cross_chat_summary()
        : mode === "retrieval"
          ? m.project_cross_chat_retrieval()
          : m.project_cross_chat_hybrid();
  }

  $effect(() => {
    ruleArea?.focus();
  });

  let open = $derived($projectSettingsOpen);
  let userRules = $derived(rules.filter((r) => r.added_by !== "auto"));

  $effect(() => {
    const id = $projectSettingsId;
    if (open && id) void load(id);
  });

  async function load(id: string) {
    try {
      project = await ipc.projectGet(id);
      paths = await ipc.projectPathsList(id);
      rules = await ipc.rulesList("project", id);
    } catch (e) {
      console.error("projectGet/paths/rules failed", e);
      return;
    }
    if (project) {
      name = project.name;
      color = project.color || "#6366f1";
      description = project.description;
      systemPrompt = project.system_prompt;
      try {
        const s = project.settings ? JSON.parse(project.settings) : {};
        crossChat = s.cross_chat ?? "off";
      } catch {
        crossChat = "off";
      }
    }
    try {
      const rs = await ipc.ragStatus(id, null);
      ragChunks = rs.project_chunks;
    } catch {
      ragChunks = 0;
    }
  }

  async function save() {
    if (!project) return;
    const settings = JSON.stringify({ cross_chat: crossChat });
    const updated: Project = {
      ...project,
      name,
      color,
      description,
      system_prompt: systemPrompt,
      default_provider_id: null,
      default_model_id: null,
      settings,
    };
    try {
      await ipc.projectUpdate(updated);
      project = updated;
      projects.update((list) => list.map((x) => (x.id === updated.id ? updated : x)));
      close();
    } catch (e) {
      console.error("projectUpdate failed", e);
    }
  }

  function close() {
    projectSettingsOpen.set(false);
  }

  async function addPath() {
    if (!project) return;
    const selected = await openDialog({ directory: true, multiple: true });
    if (!selected) return;
    const selectedPaths = Array.isArray(selected) ? selected : [selected];
    for (const p of selectedPaths) {
      if (!p) continue;
      try {
        await ipc.projectPathAdd(project.id, p, "dir", true, null);
      } catch (e) {
        console.error("projectPathAdd failed", e);
      }
    }
    paths = await ipc.projectPathsList(project.id);
    rules = await ipc.rulesList("project", project.id);
  }

  async function removePath(p: ProjectPath) {
    if (!project) return;
    try {
      await ipc.projectPathDelete(p.id, project.id);
      paths = await ipc.projectPathsList(project.id);
    } catch (e) {
      console.error("projectPathDelete failed", e);
    }
  }

  function openCreateRule() {
    ruleModalMode = "create";
    ruleModalId = null;
    ruleModalTitle = "";
    ruleModalBody = "";
    ruleModalOpen = true;
  }

  function openEditRule(r: Rule) {
    ruleModalMode = "edit";
    ruleModalId = r.id;
    ruleModalTitle = r.title;
    ruleModalBody = r.text;
    ruleModalOpen = true;
  }

  function closeRuleModal() {
    ruleModalOpen = false;
  }

  async function saveRuleModal() {
    if (!project) return;
    const title = ruleModalTitle.trim();
    const body = ruleModalBody.trim();
    if (!title || !body) return;
    try {
      if (ruleModalMode === "create") {
        await ipc.ruleAdd("project", project.id, title, body);
      } else if (ruleModalId) {
        await ipc.ruleUpdate(ruleModalId, title, body);
      }
      ruleModalOpen = false;
      rules = await ipc.rulesList("project", project.id);
    } catch (e) {
      console.error("rule save failed", e);
    }
  }

  async function deleteRule(r: Rule) {
    try {
      await ipc.ruleDelete(r.id);
      rules = await ipc.rulesList("project", project!.id);
    } catch (e) {
      console.error("ruleDelete failed", e);
    }
  }

  async function reindexRag() {
    if (!project) return;
    ragBusy = true;
    try {
      const n = await ipc.ragReindexProject(project.id);
      try {
        const rs = await ipc.ragStatus(project.id, null);
        ragChunks = rs.project_chunks;
      } catch {
        ragChunks = 0;
      }
      if (n === 0) {
        toast.info(m.rag_no_embedding_model());
      } else {
        toast.success(`${m.rag_reindex_done()} (${n})`);
      }
    } catch (e) {
      toast.error(m.rag_reindex_done(), String(e));
    } finally {
      ragBusy = false;
    }
  }

  function removeProject() {
    if (!project) return;
    confirmDeleteOpen = true;
  }

  async function confirmDeleteProject() {
    if (!project) return;
    deleting = true;
    try {
      const name = project.name;
      await deleteProject(project.id);
      toast.success(m.project_deleted(), name);
      confirmDeleteOpen = false;
      close();
    } catch (e) {
      toast.error(m.common_delete_failed(), String(e));
    } finally {
      deleting = false;
    }
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
        <span class="title">{m.project_settings()}</span>
        <button class="close" title={m.common_close()} onclick={close}><X size={16} /></button>
      </header>

      {#if project}
        <div class="body">
          <div class="grid">
            <label class="field">
              <span class="lbl">{m.project_name()}</span>
              <input bind:value={name} />
            </label>
            <div class="field narrow">
              <span class="lbl">{m.project_color()}</span>
              <ColorPicker value={color} onchange={(v) => (color = v)} align="right" />
            </div>
          </div>

          <label class="field">
            <span class="lbl">{m.project_description()}</span>
            <textarea bind:value={description} rows="2"></textarea>
          </label>

          <label class="field">
            <span class="lbl">{m.project_system_prompt()}</span>
            <textarea bind:value={systemPrompt} rows="4"></textarea>
          </label>

          <div class="field">
            <span class="lbl">{m.project_cross_chat()}</span>
            <Select
              class="w-full"
              value={crossChat}
              items={CROSS_CHAT_MODES.map((mode) => ({
                value: mode,
                label: crossChatLabel(mode),
              }))}
              onchange={(v) => (crossChat = v)}
            />
          </div>

          <div class="section">
            <div class="sec-head">
              <span>{m.project_paths()}</span>
              <div class="rag-row">
                {#if ragChunks > 0}
                  <span class="rag-count">{ragChunks} {m.rag_chunks()}</span>
                {/if}
                <button class="mini" onclick={reindexRag} disabled={ragBusy}>
                  {ragBusy ? m.rag_reindexing() : m.rag_reindex()}
                </button>
                <button class="mini" onclick={addPath}><Plus size={12} /> {m.sidebar_add_folder()}</button>
              </div>
            </div>
            {#each paths as p (p.id)}
              <div class="row">
                <span class="mono" title={p.path}>{p.path}</span>
                <button class="del" title={m.common_delete()} onclick={() => removePath(p)}><Trash2 size={12} /></button>
              </div>
            {/each}
          </div>

          <div class="section">
            <div class="sec-head">
              <span>{m.project_rules()}</span>
              <button class="mini" onclick={openCreateRule}><Plus size={12} /> {m.project_rule_add()}</button>
            </div>
            {#each userRules as r (r.id)}
              <div class="row">
                <span class="rule-title" title={r.title}>{r.title || r.text}</span>
                <button class="del" title={m.common_edit()} onclick={() => openEditRule(r)}><Pencil size={12} /></button>
                <button class="del" title={m.common_delete()} onclick={() => deleteRule(r)}><Trash2 size={12} /></button>
              </div>
            {/each}
          </div>
        </div>

        <footer class="foot">
          <button class="danger" onclick={removeProject}>{m.common_delete()}</button>
          <div class="spacer"></div>
          <button class="ghost" onclick={close}>{m.common_cancel()}</button>
          <button class="primary" onclick={save}>{m.common_save()}</button>
        </footer>
      {/if}
    </div>
  </div>

  {#if ruleModalOpen}
    <div class="overlay rule-overlay" onkeydown={onRuleKeydown} role="presentation">
      <div class="dialog rule-dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onRuleKeydown} role="dialog">
        <header class="head">
          <span class="title">{ruleModalMode === "create" ? m.project_rule_add() : m.common_edit()}</span>
          <button class="close" title={m.common_close()} onclick={closeRuleModal}><X size={16} /></button>
        </header>
        <div class="body">
          <div class="field">
            <span class="lbl">{m.project_rule_title()}</span>
            <input bind:value={ruleModalTitle} placeholder={m.project_rule_title()} />
          </div>
          <div class="field">
            <span class="lbl">{m.project_rule_body()}</span>
            <textarea bind:value={ruleModalBody} bind:this={ruleArea} rows="6"></textarea>
          </div>
        </div>
        <footer class="foot">
          <div class="spacer"></div>
          <button class="ghost" onclick={closeRuleModal}>{m.common_cancel()}</button>
          <button class="primary" onclick={() => void saveRuleModal()}>{m.common_save()}</button>
        </footer>
      </div>
    </div>
  {/if}

  <ConfirmDialog
    open={confirmDeleteOpen}
    message={m.project_delete_confirm()}
    loading={deleting}
    loadingLabel={m.common_deleting()}
    onconfirm={() => void confirmDeleteProject()}
    oncancel={() => (confirmDeleteOpen = false)}
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
  .grid {
    display: flex;
    gap: 0.75rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
  }
  .field.narrow {
    flex: 0 0 5rem;
  }
  .lbl {
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }
  input, textarea {
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
  input:focus, textarea:focus {
    border-color: var(--ring);
  }
  textarea {
    resize: vertical;
    font-family: var(--font-mono);
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
  .rag-row {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
  }
  .rag-count {
    font-size: 0.7rem;
    font-weight: 400;
    color: var(--muted-foreground);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.78rem;
    min-width: 0;
  }
  .mono {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rule-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rule-overlay {
    z-index: 70;
  }
  .rule-dialog {
    width: 420px;
  }
  .del {
    flex-shrink: 0;
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
  .del {
    display: inline-flex;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    cursor: default;
  }
  .del:hover {
    color: var(--destructive);
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
  .ghost {
    background: var(--background);
    color: var(--foreground);
  }
  .primary {
    background: var(--primary);
    color: var(--primary-foreground);
    border-color: var(--primary);
  }
  .danger {
    background: transparent;
    color: var(--destructive);
    border-color: var(--destructive);
  }
</style>