<script module lang="ts">
  export type SelectItem = {
    value: string;
    label: string;
    disabled?: boolean;
  };
</script>

<script lang="ts">
  import { m } from "$lib/i18n";
  import { ChevronDown } from "@lucide/svelte";

  let {
    value,
    items,
    onchange,
    placeholder,
    title,
    class: className = "",
    wide = true,
  }: {
    value: string;
    items: SelectItem[];
    onchange: (value: string) => void;
    placeholder?: string;
    title?: string;
    class?: string;
    wide?: boolean;
  } = $props();

  let open = $state(false);

  let triggerEl = $state<HTMLButtonElement | null>(null);
  let menuEl = $state<HTMLDivElement | null>(null);
  let menuTop = $state("");
  let menuLeft = $state("");
  let menuMinWidth = $state("");

  const current = $derived(items.find((it) => it.value === value));
  const currentLabel = $derived(
    current?.label ?? placeholder ?? m.common_select(),
  );

  function pick(item: SelectItem) {
    if (item.disabled) return;
    open = false;
    if (item.value !== value) onchange(item.value);
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (!open || e.key !== "Escape") return;
    // Capture phase: close only the dropdown, not enclosing dialogs.
    e.stopPropagation();
    e.preventDefault();
    open = false;
  }

  function positionMenu() {
    if (!open || !triggerEl) return;
    const r = triggerEl.getBoundingClientRect();
    let top = r.bottom + 4;
    let left = r.left;
    if (menuEl) {
      const rect = menuEl.getBoundingClientRect();
      if (top + rect.height > window.innerHeight - 4) {
        top = Math.max(4, r.top - rect.height - 4);
      }
      if (left + rect.width > window.innerWidth - 4) {
        left = Math.max(4, window.innerWidth - rect.width - 4);
      }
    }
    menuTop = `${top}px`;
    menuLeft = `${left}px`;
    menuMinWidth = `${r.width}px`;
  }

  $effect(() => {
    if (!open) return;
    positionMenu();
    requestAnimationFrame(positionMenu);
  });
</script>

<svelte:window
  onkeydowncapture={onWindowKeydown}
  onscroll={() => open && (open = false)}
  onresize={() => open && (open = false)}
/>

<div class="sel {className}">
  <button
    bind:this={triggerEl}
    class="sel-trigger"
    class:placeholder={!current}
    title={title}
    aria-haspopup="listbox"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <span class="sel-current">{currentLabel}</span>
    <ChevronDown size={14} />
  </button>

  {#if open}
    <button
      class="sel-backdrop"
      aria-label={m.common_close()}
      onclick={() => (open = false)}
    ></button>
    <div
      class="sel-menu"
      class:wide
      role="listbox"
      bind:this={menuEl}
      style:top={menuTop}
      style:left={menuLeft}
      style:min-width={menuMinWidth}
    >
      {#each items as item (item.value)}
        <button
          class="sel-item"
          class:active={item.value === value}
          disabled={item.disabled}
          role="option"
          aria-selected={item.value === value}
          onclick={() => pick(item)}
        >
          <span class="sel-item-label">{item.label}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .sel {
    position: relative;
    display: inline-block;
    max-width: 100%;
  }
  .sel-trigger {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    width: 100%;
    height: var(--sel-height, 1.75rem);
    max-width: var(--sel-max-width, none);
    padding: 0 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--sel-radius, var(--radius-md));
    background: var(--sel-background, var(--background));
    color: var(--sel-color, var(--foreground));
    font-size: var(--sel-font-size, 0.8125rem);
    font-family: inherit;
    text-align: left;
    cursor: default;
  }
  .sel-trigger:hover {
    background: var(--sel-hover-background, var(--accent));
    color: var(--sel-hover-color, var(--sel-color, var(--foreground)));
  }
  .sel-trigger.placeholder {
    color: var(--muted-foreground);
  }
  .sel-current {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sel-trigger :global(svg) {
    flex-shrink: 0;
    color: var(--muted-foreground);
  }
  .sel-backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: transparent;
    border: none;
    padding: 0;
    cursor: default;
  }
  .sel-menu {
    position: fixed;
    max-height: 320px;
    overflow-y: auto;
    padding: 0.25rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--popover);
    color: var(--popover-foreground);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.18);
    font-size: 0.8125rem;
    z-index: 30;
  }
  .sel-menu.wide {
    width: max-content;
    max-width: 320px;
    overflow-x: auto;
  }
  .sel-item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 0.3rem 0.5rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    font-size: inherit;
    font-family: inherit;
    cursor: default;
  }
  .sel-item:hover:not(:disabled),
  .sel-item:focus-visible {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .sel-item.active {
    background: var(--secondary);
    color: var(--secondary-foreground);
  }
  .sel-item:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .sel-item-label {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Wide menu grows with the longest item instead of truncating it. */
  .sel-menu.wide .sel-item-label {
    overflow: visible;
    text-overflow: clip;
  }
</style>