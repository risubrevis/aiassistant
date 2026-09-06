<script lang="ts">
  import PromptCard from "./PromptCard.svelte";
  import PromptModal from "./PromptModal.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { m } from "$lib/i18n";
  import { prompts, loadPrompts, movePrompt, deletePrompt, setPromptFavorite, runPrompt } from "$lib/stores/prompts";
  import { projects } from "$lib/stores/project";
  import { skills as skillsStore } from "$lib/stores/skills";
  import { toast } from "$lib/stores/toasts";
  import type { Prompt } from "$lib/tauri";
  import { Plus } from "@lucide/svelte";

  let modalOpen = $state(false);
  let editingPrompt: Prompt | null = $state(null);
  let deleteTargetId = $state<string | null>(null);

  let list = $derived($prompts.slice().sort((a, b) => a.position - b.position));

  $effect(() => {
    void loadPrompts();
  });

  function onAddPrompt() {
    editingPrompt = null;
    modalOpen = true;
  }

  function onEditPrompt(p: Prompt) {
    editingPrompt = p;
    modalOpen = true;
  }

  function onfavorite(id: string, fav: boolean) {
    void setPromptFavorite(id, fav);
  }

  function onmove(id: string, toPos: number) {
    void movePrompt(id, toPos);
  }

  function ondelete(id: string) {
    deleteTargetId = id;
  }

  function confirmDelete() {
    if (deleteTargetId) void deletePrompt(deleteTargetId);
    deleteTargetId = null;
  }

  async function onrun(id: string) {
    try {
      await runPrompt(id);
    } catch (e) {
      toast.error(m.prompt_run_missing_files(), e instanceof Error ? e.message : undefined);
    }
  }
</script>

<div class="wrap">
  <header class="head">
    <span class="title">{m.prompts_view_title()}</span>
    <button class="add" onclick={onAddPrompt}>
      <Plus size={13} />
      <span>{m.prompts_add()}</span>
    </button>
  </header>
  <div class="boardarea">
    {#if list.length === 0}
      <div class="empty">
        <p>{m.prompts_empty()}</p>
        <button class="add" onclick={onAddPrompt}>
          <Plus size={13} />
          <span>{m.prompts_add()}</span>
        </button>
      </div>
    {:else}
      {#each list as prompt, i (prompt.id)}
        <div class="row-item">
          <PromptCard
            {prompt}
            projectName={$projects.find((p) => p.id === prompt.project_id)?.name ?? null}
            skillTitles={prompt.skill_ids.map((id) => $skillsStore.find((s) => s.id === id)?.title ?? id)}
            onrun={onrun}
            onedit={onEditPrompt}
            onfavorite={onfavorite}
            onmove={onmove}
            ondelete={ondelete}
            canMoveUp={i > 0}
            canMoveDown={i < list.length - 1}
          />
        </div>
      {/each}
    {/if}
  </div>
</div>
<PromptModal open={modalOpen} prompt={editingPrompt} onclose={() => (modalOpen = false)} />
<ConfirmDialog
  open={deleteTargetId !== null}
  message={m.prompt_delete_confirm()}
  onconfirm={confirmDelete}
  oncancel={() => (deleteTargetId = null)}
/>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-shrink: 0;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--foreground);
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    flex-shrink: 0;
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--foreground);
    font-size: 0.72rem;
    cursor: pointer;
  }
  .head .add {
    margin-left: auto;
  }
  .add:hover {
    background: var(--accent);
  }
  .boardarea {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0.5rem;
  }
  .row-item {
    margin-bottom: 0.4rem;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    flex: 1;
    color: var(--muted-foreground);
    font-size: 0.8rem;
    text-align: center;
  }
  .empty p {
    margin: 0;
  }
</style>