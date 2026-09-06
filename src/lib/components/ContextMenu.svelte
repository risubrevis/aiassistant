<script lang="ts">
  export interface ContextMenuItem {
    label: string;
    icon?: typeof import("@lucide/svelte").X;
    danger?: boolean;
    disabled?: boolean;
    separator?: boolean;
    onclick?: () => void;
  }

  let {
    x,
    y,
    items,
    onclose,
  }: {
    x: number;
    y: number;
    items: ContextMenuItem[];
    onclose?: () => void;
  } = $props();

  let menuEl: HTMLElement | undefined = $state();
  // Initial position from props is intentional; $effect below clamps it to the viewport.
  // svelte-ignore state_referenced_locally
  let pos = $state({ left: x, top: y });

  function close() {
    onclose?.();
  }

  function pick(item: ContextMenuItem) {
    item.onclick?.();
    close();
  }

  $effect(() => {
    const w = menuEl?.offsetWidth ?? 200;
    const h = menuEl?.offsetHeight ?? items.length * 30;
    pos = {
      left: Math.min(x, window.innerWidth - w - 8),
      top: Math.min(y, window.innerHeight - h - 8),
    };
  });

  $effect(() => {
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });
</script>

<div
  class="ctx-overlay"
  role="presentation"
  onclick={close}
  oncontextmenu={(e) => { e.preventDefault(); close(); }}
  onkeydown={(e) => e.key === "Escape" && close()}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="ctx-menu"
    bind:this={menuEl}
    style="left:{pos.left}px;top:{pos.top}px"
    role="menu"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    oncontextmenu={(e) => { e.preventDefault(); e.stopPropagation(); }}
  >
    {#each items as item, i (i)}
      {#if item.separator}
        <div class="sep"></div>
      {:else}
        <button
          class="item"
          class:danger={item.danger}
          class:disabled={item.disabled}
          disabled={item.disabled}
          role="menuitem"
          onclick={() => pick(item)}
        >
          {#if item.icon}
            {@const Icon = item.icon}
            <Icon size={14} />
          {/if}
          <span>{item.label}</span>
        </button>
      {/if}
    {/each}
  </div>
</div>

<style>
  .ctx-overlay {
    position: fixed;
    inset: 0;
    z-index: 90;
  }
  .ctx-menu {
    position: fixed;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    max-width: 280px;
    padding: 0.25rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--popover);
    color: var(--popover-foreground);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.22);
    font-size: 0.78rem;
    z-index: 100;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    width: 100%;
    padding: 0.3rem 0.45rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    font-size: 0.78rem;
    text-align: left;
    cursor: default;
  }
  .item :global(svg) {
    flex-shrink: 0;
    color: var(--muted-foreground);
  }
  .item:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .item.danger {
    color: var(--destructive);
  }
  .item.danger :global(svg) {
    color: var(--destructive);
  }
  .item.danger:hover {
    background: var(--destructive);
    color: var(--destructive-foreground);
  }
  .item.disabled {
    opacity: 0.5;
    pointer-events: none;
  }
  .sep {
    height: 1px;
    margin: 0.25rem 0.35rem;
    background: var(--border);
  }
</style>