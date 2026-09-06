<script lang="ts">
  import { onMount } from "svelte";
  import { toolsList, toolsSetEnabled, setDeleteToTrash, type ToolInfo } from "$lib/tauri";
  import { config as configStore } from "$lib/stores/config";
  import { m } from "$lib/i18n";
  import { Wrench } from "@lucide/svelte";

  let builtin = $state<ToolInfo[]>([]);
  let busy = $state<string | null>(null);

  onMount(async () => {
    try {
      builtin = (await toolsList()).sort((a, b) => a.name.localeCompare(b.name));
    } catch (e) {
      console.error(e);
    }
  });

  const CAT_COLOR: Record<string, string> = {
    readonly: "hsl(210 60% 45%)",
    write: "hsl(38 92% 45%)",
    exec: "hsl(0 70% 50%)",
    destructive: "hsl(0 80% 45%)",
    network: "hsl(280 50% 50%)",
    interaction: "hsl(140 50% 40%)",
  };
  function catColor(c: string) {
    return CAT_COLOR[c] ?? "var(--muted-foreground)";
  }

  let trashEnabled = $state($configStore?.defaults.delete_to_trash ?? false);

  async function toggleTrash() {
    const next = !trashEnabled;
    trashEnabled = next;
    try {
      await setDeleteToTrash(next);
    } catch (e) {
      console.error(e);
      trashEnabled = !next;
    }
  }

  async function toggle(t: ToolInfo) {
    if (busy) return;
    const next = !t.enabled;
    t.enabled = next;
    busy = t.name;
    try {
      await toolsSetEnabled(t.name, next);
    } catch (e) {
      console.error(e);
      t.enabled = !next;
    } finally {
      busy = null;
    }
  }
</script>

<div class="tools">
  <div class="head"><Wrench size={13} /> Builtin tools</div>

  <div class="trash-toggle">
    <label class="trash-label">
      <span class="trash-title">{m.settings_tools_trash()}</span>
      <input
        class="toggle"
        type="checkbox"
        checked={trashEnabled}
        onchange={toggleTrash}
      />
    </label>
    <div class="hint">{m.settings_tools_trash_hint()}</div>
  </div>

  {#if builtin.length === 0}
    <div class="empty">No builtin tools available.</div>
  {:else}
    <div class="table">
      <div class="th">
        <span>Tool</span>
        <span>Description</span>
        <span class="c">Enabled</span>
      </div>
      {#each builtin as t (t.name)}
        <div class="tr">
          <span class="tool-name">
            <span class="name">{t.name}</span>
            <span class="badge" style={`color:${catColor(t.category)}`}>{t.category}</span>
          </span>
          <span class="desc" title={t.description}>{t.description}</span>
          <input
            class="c"
            type="checkbox"
            checked={t.enabled}
            disabled={busy !== null}
            onchange={() => toggle(t)}
          />
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .tools { display: flex; flex-direction: column; gap: 0.5rem; min-width: 0; }
  .head { display: inline-flex; align-items: center; gap: 0.3rem; font-size: 0.8125rem; font-weight: 600; }
  .table { display: flex; flex-direction: column; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden; }
  .th, .tr { display: grid; grid-template-columns: minmax(130px, 0.9fr) 2fr 60px; gap: 0.5rem; align-items: center; padding: 0.3rem 0.5rem; }
  .th { background: var(--muted); font-size: 0.7rem; color: var(--muted-foreground); text-transform: uppercase; letter-spacing: 0.03em; }
  .tr { border-top: 1px solid var(--border); font-size: 0.8125rem; }
  .tool-name { display: inline-flex; align-items: center; gap: 0.35rem; min-width: 0; }
  .name { font-family: var(--font-mono); font-size: 0.75rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { font-size: 0.62rem; text-transform: uppercase; font-weight: 600; flex-shrink: 0; }
  .desc { color: var(--muted-foreground); font-size: 0.75rem; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .c { text-align: center; }
  .tr input[type="checkbox"] { justify-self: center; cursor: default; }
  .empty { color: var(--muted-foreground); font-size: 0.8125rem; padding: 0.5rem 0; }
  .trash-toggle { border: 1px solid var(--border); border-radius: var(--radius-md); padding: 0.5rem 0.6rem; display: flex; flex-direction: column; gap: 0.2rem; }
  .trash-label { display: flex; align-items: center; justify-content: space-between; gap: 0.5rem; }
  .trash-title { font-size: 0.8125rem; font-weight: 600; }
  .hint { color: var(--muted-foreground); font-size: 0.72rem; }
</style>