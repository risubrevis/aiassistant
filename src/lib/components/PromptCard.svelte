<script lang="ts">
  import { Star, Play, Pencil, Trash2, ChevronUp, ChevronDown, LayoutTemplate } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import type { Prompt } from "$lib/tauri";

  let {
    prompt,
    projectName,
    skillTitles,
    onrun,
    onedit,
    onfavorite,
    onmove,
    ondelete,
    canMoveUp,
    canMoveDown,
  }: {
    prompt: Prompt;
    projectName: string | null;
    skillTitles: string[];
    onrun: (id: string) => void;
    onedit: (prompt: Prompt) => void;
    onfavorite: (id: string, isFavorite: boolean) => void;
    onmove: (id: string, toPosition: number) => void;
    ondelete: (id: string) => void;
    canMoveUp: boolean;
    canMoveDown: boolean;
  } = $props();

  function basename(path: string): string {
    return path.split(/[/\\]/).pop() ?? path;
  }
</script>

<div
  class="card"
  role="button"
  tabindex="0"
  onclick={() => onedit(prompt)}
  onkeydown={(e) => e.key === "Enter" && onedit(prompt)}
>
  <div class="top">
    <span class="title">{prompt.title}</span>
    <button
      class="act star"
      class:fav={prompt.is_favorite}
      title={m.prompt_is_favorite()}
      onclick={(e) => {
        e.stopPropagation();
        onfavorite(prompt.id, !prompt.is_favorite);
      }}
      onmousedown={(e) => e.stopPropagation()}
    >
      <Star size={13} fill={prompt.is_favorite ? "currentColor" : "none"} />
    </button>
  </div>
  <div class="badges">
    {#if prompt.project_id}
      <span class="badge" title={projectName ?? prompt.project_id}>
        {projectName ?? prompt.project_id}
      </span>
    {:else}
      <span class="badge">{m.prompt_standalone()}</span>
    {/if}
    {#each prompt.attach_files as file (file)}
      <span class="badge" title={file}>{basename(file)}</span>
    {/each}
    {#if prompt.is_template}
      <span class="badge tpl-badge">{m.prompt_is_template()}</span>
    {/if}
    {#each skillTitles as t, i (i)}
      <span class="badge">{t}</span>
    {/each}
  </div>
  <div class="bottom">
    <div class="actions">
      <button
        class="act"
        title={m.prompt_move_up()}
        disabled={!canMoveUp}
        onclick={(e) => {
          e.stopPropagation();
          onmove(prompt.id, prompt.position - 1);
        }}
        onmousedown={(e) => e.stopPropagation()}
      >
        <ChevronUp size={13} />
      </button>
      <button
        class="act"
        title={m.prompt_move_down()}
        disabled={!canMoveDown}
        onclick={(e) => {
          e.stopPropagation();
          onmove(prompt.id, prompt.position + 1);
        }}
        onmousedown={(e) => e.stopPropagation()}
      >
        <ChevronDown size={13} />
      </button>
      <button
        class="act"
        title={m.prompt_edit()}
        onclick={(e) => {
          e.stopPropagation();
          onedit(prompt);
        }}
        onmousedown={(e) => e.stopPropagation()}
      >
        <Pencil size={13} />
      </button>
      <button
        class="act del"
        title={m.prompt_delete()}
        onclick={(e) => {
          e.stopPropagation();
          ondelete(prompt.id);
        }}
        onmousedown={(e) => e.stopPropagation()}
      >
        <Trash2 size={13} />
      </button>
      {#if prompt.is_template}
        <button
          class="act tpl"
          title={m.prompt_use_template()}
          onclick={(e) => {
            e.stopPropagation();
            onrun(prompt.id);
          }}
          onmousedown={(e) => e.stopPropagation()}
        >
          <LayoutTemplate size={13} />
        </button>
      {:else}
        <button
          class="act run"
          title={m.prompt_run()}
          onclick={(e) => {
            e.stopPropagation();
            onrun(prompt.id);
          }}
          onmousedown={(e) => e.stopPropagation()}
        >
          <Play size={13} />
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    padding: 0.5rem 0.625rem;
    font-size: 0.8rem;
    color: var(--foreground);
    cursor: default;
    user-select: none;
  }
  .card:hover {
    border-color: var(--muted-foreground);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    min-width: 0;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    max-width: 12rem;
    padding: 0.05rem 0.45rem;
    border-radius: 9999px;
    background: var(--muted);
    color: var(--muted-foreground);
    font-size: 0.65rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bottom {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.68rem;
  }
  .actions {
    display: inline-flex;
    gap: 0.25rem;
    flex-shrink: 0;
    margin-left: auto;
  }
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.4rem;
    height: 1.4rem;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted-foreground);
    cursor: pointer;
  }
  .act:hover:not(:disabled) {
    background: var(--accent);
  }
  .act:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .act.star.fav {
    color: #f59e0b;
  }
  .act.del:hover {
    color: #ef4444;
  }
  .act.run {
    color: #22c55e;
  }
  .act.tpl {
    color: #f59e0b;
  }
  .tpl-badge {
    background: rgba(245, 158, 11, 0.15);
    color: #f59e0b;
  }
</style>