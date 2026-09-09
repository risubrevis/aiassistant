<script lang="ts">
  import { onMount } from "svelte";
  import { Play } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import ActivityGraph from "./ActivityGraph.svelte";
  import { favoritePrompts, loadFavoritePrompts, runPrompt } from "$lib/stores/prompts";
  import { projects } from "$lib/stores/project";
  import { toast } from "$lib/stores/toasts";
  import type { Prompt } from "$lib/tauri";

  onMount(() => {
    void loadFavoritePrompts();
  });

  let favList = $derived($favoritePrompts.slice().sort((a, b) => a.position - b.position));

  // "Project Name / Prompt title" when the prompt is bound to a project,
  // otherwise just the title.
  function displayTitle(prompt: Prompt): string {
    if (!prompt.project_id) return prompt.title;
    const name = $projects.find((p) => p.id === prompt.project_id)?.name;
    return name ? `${name} / ${prompt.title}` : prompt.title;
  }

  async function onrun(id: string) {
    try {
      await runPrompt(id);
    } catch (e) {
      toast.error(m.prompt_run_failed(), e instanceof Error ? e.message : undefined);
    }
  }
</script>

<div class="home">
  <section class="home-section home-activity">
    <div class="activity-frame">
      <ActivityGraph />
    </div>
  </section>

  <section class="home-section home-prompts">
    <div class="prompts-frame">
      <header class="prompts-head">
        <span class="prompts-title">{m.prompts_favorites()}</span>
        {#if favList.length > 0}
          <span class="prompts-count">{favList.length}</span>
        {/if}
      </header>
      <div class="prompts-scroll">
        {#if favList.length === 0}
          <p class="prompts-empty">{m.prompts_favorites_empty()}</p>
        {:else}
          {#each favList as prompt (prompt.id)}
            <button
              class="prompt-row"
              title={displayTitle(prompt)}
              onclick={() => void onrun(prompt.id)}
            >
              <span class="prompt-name">{displayTitle(prompt)}</span>
              <span class="prompt-run-icon"><Play size={12} /></span>
            </button>
          {/each}
        {/if}
      </div>
    </div>
  </section>
</div>

<style>
  .home {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    flex: 1;
    min-height: 0;
    padding: 1rem 1rem 1.5rem;
  }
  /* Both sections share the Activity widget's max-width so the prompts frame
     aligns exactly with the activity grid. */
  .home-section {
    width: 100%;
    max-width: 900px;
    margin-inline: auto;
  }
  .home-activity {
    flex: 0 0 auto;
  }
  .activity-frame {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--background);
    padding: 0.75rem 0.875rem;
  }
  .home-prompts {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .prompts-frame {
    display: flex;
    flex-direction: column;
    flex: 0 1 auto;
    min-height: 0;
    max-height: 100%;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--background);
    overflow: hidden;
  }
  .prompts-head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-shrink: 0;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
  }
  .prompts-title {
    font-size: 0.72rem;
    font-weight: 600;
    color: var(--muted-foreground);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .prompts-count {
    padding: 0 0.4rem;
    border-radius: 9999px;
    background: var(--accent);
    color: var(--muted-foreground);
    font-size: 0.62rem;
    font-weight: 500;
    line-height: 1.15rem;
  }
  .prompts-scroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 0.375rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .prompts-empty {
    margin: 0;
    padding: 0.75rem 0.5rem;
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .prompt-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--foreground);
    font-size: 0.8rem;
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }
  .prompt-row:hover {
    background: var(--accent);
    border-color: var(--border);
  }
  .prompt-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .prompt-run-icon {
    flex-shrink: 0;
    color: #22c55e;
  }
</style>