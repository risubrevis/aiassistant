<script lang="ts">
  import { tick } from "svelte";
  import { m } from "$lib/i18n";

  let {
    open,
    title,
    initial,
    onsave,
    oncancel,
  }: {
    open: boolean;
    title: string;
    initial: string;
    onsave: (newName: string) => void;
    oncancel?: () => void;
  } = $props();

  let name = $state("");
  let inputEl: HTMLInputElement | undefined = $state();

  let canSave = $derived(name.trim().length > 0);

  function save() {
    const value = name.trim();
    if (!value) return;
    onsave(value);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") oncancel?.();
    else if (e.key === "Enter") save();
  }

  $effect(() => {
    if (open) name = initial;
  });

  $effect(() => {
    if (!open) return;
    void tick().then(() => {
      inputEl?.focus();
      inputEl?.select();
    });
  });
</script>

<svelte:window onkeydown={(e) => open && e.key === "Escape" && oncancel?.()} />

{#if open}
  <div class="overlay" role="presentation">
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">{title}</header>
      <div class="body">
        <input
          class="input"
          bind:this={inputEl}
          bind:value={name}
          type="text"
          placeholder={m.rename_placeholder()}
          spellcheck="false"
        />
        <div class="foot">
          <button class="btn" onclick={() => oncancel?.()}>{m.common_cancel()}</button>
          <button class="btn primary" disabled={!canSave} onclick={save}>{m.common_save()}</button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .dialog {
    width: 360px;
    max-width: 90vw;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background-color: var(--background);
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.28);
  }
  .head {
    padding: 0.625rem 0.875rem 0.25rem;
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--foreground);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 0.25rem 0.875rem 0.875rem;
  }
  .input {
    width: 100%;
    height: 2rem;
    padding: 0 0.625rem;
    border: 1px solid var(--input);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    outline: none;
  }
  .input:focus {
    border-color: var(--ring);
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    height: 1.75rem;
    padding: 0 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.78rem;
    cursor: default;
  }
  .btn:hover:not(:disabled) {
    background: var(--accent);
  }
  .btn.primary {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover:not(:disabled) {
    background: var(--primary);
    opacity: 0.9;
  }
  .btn:disabled {
    opacity: 0.45;
  }
</style>