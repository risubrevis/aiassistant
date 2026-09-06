<script lang="ts">
  import { pendingByChat, approvePending, rejectPending } from "$lib/stores/chat";
  import { m } from "$lib/i18n";
  import { Check, X, FilePlus, FileEdit, FileX, ChevronRight, Loader } from "@lucide/svelte";

  let { chatId }: { chatId: string } = $props();

  let expanded = $state<Record<string, boolean>>({});

  let changes = $derived($pendingByChat[chatId] ?? []);

  let busy = $state<"approve" | "reject" | null>(null);

  async function handleApprove() {
    if (busy) return;
    busy = "approve";
    try { await approvePending(chatId); } catch (e) { console.error(e); }
    busy = null;
  }

  async function handleReject() {
    if (busy) return;
    busy = "reject";
    try { await rejectPending(chatId); } catch (e) { console.error(e); }
    busy = null;
  }

  function kindLabel(kind: string) {
    return kind === "created" ? "created" : kind === "deleted" ? "deleted" : "modified";
  }
</script>

{#if changes.length}
  <div class="pending">
    <div class="head">
      <span class="title">Pending changes ({changes.length})</span>
      <div class="actions">
        {#if busy}
          <span class="apr busy"><Loader size={12} class="spin" /> {busy === "approve" ? "Approving…" : "Rejecting…"}</span>
        {:else}
          <button class="apr yes" onclick={handleApprove}>
            <Check size={12} /> Approve
          </button>
          <button class="apr no" onclick={handleReject}>
            <X size={12} /> Reject
          </button>
        {/if}
      </div>
    </div>
    <div class="files">
      {#each changes as c (c.path)}
        <div class="file">
          <button
            class="file-head"
            onclick={() => (expanded[c.path] = !expanded[c.path])}
          >
            <ChevronRight size={12} class={expanded[c.path] ? "rot" : ""} />
            {#if c.kind === "created"}
              <FilePlus size={12} />
            {:else if c.kind === "deleted"}
              <FileX size={12} />
            {:else}
              <FileEdit size={12} />
            {/if}
            <span class="path">{c.path}</span>
            <span class="kind">{kindLabel(c.kind)}</span>
          </button>
          {#if expanded[c.path] && c.diff}
            <pre class="diff">{c.diff}</pre>
          {/if}
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .pending {
    border-bottom: 1px solid var(--border);
    background: var(--muted);
    padding: 0.4rem 0.75rem;
    max-height: 40%;
    overflow-y: auto;
  }
  .head { display: flex; align-items: center; justify-content: space-between; }
  .title { font-size: 0.75rem; font-weight: 600; }
  .actions { display: flex; gap: 0.4rem; }
  .apr { display: inline-flex; align-items: center; gap: 0.25rem; padding: 0.2rem 0.5rem; border: none; border-radius: var(--radius-sm); font-size: 0.72rem; cursor: default; }
  .apr.yes { background: hsl(140 60% 40%); color: white; }
  .apr.no { background: var(--destructive); color: var(--destructive-foreground); }
  .apr.busy { color: var(--muted-foreground); }
  :global(.spin) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .files { margin-top: 0.3rem; display: flex; flex-direction: column; gap: 0.15rem; }
  .file { background: var(--background); border: 1px solid var(--border); border-radius: var(--radius-sm); }
  .file-head { display: flex; align-items: center; gap: 0.3rem; width: 100%; padding: 0.3rem 0.4rem; background: transparent; border: none; text-align: left; font-size: 0.72rem; cursor: default; }
  .file-head :global(.rot) { transform: rotate(90deg); }
  .path { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono); }
  .kind { font-size: 0.62rem; color: var(--muted-foreground); text-transform: uppercase; }
  .diff { margin: 0; padding: 0.4rem; max-height: 200px; overflow: auto; font-family: var(--font-mono); font-size: 0.68rem; white-space: pre; border-top: 1px solid var(--border); }
</style>