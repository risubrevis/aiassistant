<script lang="ts">
  import { ChevronLeft, ChevronRight, ExternalLink, X } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { openExternalLocation, type MediaImage } from "$lib/media";

  let {
    open = $bindable(false),
    images,
    index,
    onclose,
  }: {
    open: boolean;
    images: MediaImage[];
    index: number;
    onclose: () => void;
  } = $props();

  // Snapshot is intentional; the $effect below re-syncs it to the index prop.
  // svelte-ignore state_referenced_locally
  let current = $state(index);
  let zoomed = $state(false);

  const clampedIndex = $derived(Math.max(0, Math.min(index, images.length - 1)));
  const safeCurrent = $derived(Math.max(0, Math.min(current, images.length - 1)));
  const image = $derived(images[safeCurrent]);

  $effect(() => {
    if (open) {
      current = clampedIndex;
      zoomed = false;
    }
  });

  $effect(() => {
    void current;
    zoomed = false;
  });

  $effect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onclose();
      else if (e.key === "ArrowLeft") {
        e.preventDefault();
        prev();
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        next();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  });

  function prev() {
    if (images.length < 2) return;
    current = (safeCurrent + images.length - 1) % images.length;
  }

  function next() {
    if (images.length < 2) return;
    current = (safeCurrent + 1) % images.length;
  }
</script>

{#if open && images.length > 0}
  <div
    class="viewer"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onkeydown={() => {}}
    onclick={() => onclose()}
  >
    <button
      class="stage"
      class:zoomed
      aria-label={m.media_toggle_zoom()}
      onclick={(e) => {
        e.stopPropagation();
        zoomed = !zoomed;
      }}
    >
      <img class="image" src={image.src} alt={image.alt} draggable="false" />
    </button>

    {#if images.length > 1}
      <button
        class="nav prev"
        aria-label={m.media_previous()}
        onclick={(e) => {
          e.stopPropagation();
          prev();
        }}
      >
        <ChevronLeft size={26} />
      </button>
      <button
        class="nav next"
        aria-label={m.media_next()}
        onclick={(e) => {
          e.stopPropagation();
          next();
        }}
      >
        <ChevronRight size={26} />
      </button>
    {/if}

    <button
      class="close"
      aria-label={m.media_close()}
      onclick={(e) => {
        e.stopPropagation();
        onclose();
      }}
    >
      <X size={18} />
    </button>

    <button
      class="open"
      aria-label={m.media_open_external()}
      onclick={(e) => {
        e.stopPropagation();
        void openExternalLocation(image.orig ?? image.src);
      }}
    >
      <ExternalLink size={16} />
    </button>

    <div class="counter">{safeCurrent + 1} / {images.length}</div>
  </div>
{/if}

<style>
  .viewer {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    background-color: rgb(0 0 0 / 0.92);
  }
  .stage {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 0;
    background: none;
    max-width: 90vw;
    max-height: 88vh;
    cursor: zoom-in;
    transition: transform 0.2s ease;
  }
  .stage.zoomed {
    transform: scale(2);
    cursor: zoom-out;
  }
  .image {
    max-width: 90vw;
    max-height: 88vh;
    object-fit: contain;
    border-radius: var(--radius-md);
    user-select: none;
  }
  .nav,
  .close,
  .open {
    position: absolute;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    width: 2.75rem;
    height: 2.75rem;
    border: 1px solid rgb(255 255 255 / 0.2);
    border-radius: 999px;
    background-color: color-mix(in srgb, var(--background) 60%, transparent);
    color: var(--foreground);
    cursor: default;
    transition: background-color 0.15s ease;
  }
  .close {
    top: 1rem;
    right: 1rem;
    width: 2.25rem;
    height: 2.25rem;
  }
  .open {
    top: 3.6rem;
    right: 1rem;
    width: 2.25rem;
    height: 2.25rem;
  }
  .prev {
    top: 50%;
    left: 1rem;
    transform: translateY(-50%);
  }
  .next {
    top: 50%;
    right: 1rem;
    transform: translateY(-50%);
  }
  .nav:hover,
  .close:hover,
  .open:hover {
    background-color: color-mix(in srgb, var(--background) 85%, transparent);
  }
  .counter {
    position: absolute;
    bottom: 1.25rem;
    left: 50%;
    transform: translateX(-50%);
    padding: 0.25rem 0.75rem;
    border-radius: var(--radius-md);
    background-color: color-mix(in srgb, var(--background) 60%, transparent);
    color: var(--foreground);
    font-size: 0.78rem;
    pointer-events: none;
  }
</style>