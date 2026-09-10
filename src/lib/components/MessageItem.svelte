<script lang="ts">
  import {
    File as FileIcon,
    Image as ImageIcon,
    ImageOff,
    Pencil,
  } from "@lucide/svelte";
  import type { UiMessage } from "$lib/stores/chat";
  import { attachmentsByChat, editMessage } from "$lib/stores/chat";
  import { attachmentReadDataUrl } from "$lib/tauri";
  import { renderMarkdown } from "$lib/markdown";
  import { m } from "$lib/i18n";
  import { formatSize } from "$lib/utils";

  let { message, showThinking = false }: { message: UiMessage; showThinking?: boolean } = $props();

  let editing = $state(false);
  let editText = $state("");

  function startEdit() {
    editText = message.content;
    editing = true;
  }
  function saveEdit() {
    const t = editText.trim();
    if (t) void editMessage(message.id, t);
    editing = false;
  }
  function cancelEdit() {
    editing = false;
  }

  let isUser = $derived(message.role === "user");
  let userAttachments = $derived.by(() => {
    if (!isUser) return [];
    for (const list of Object.values($attachmentsByChat)) {
      const found = list.filter((a) => a.message_id === message.id);
      if (found.length) return found;
    }
    return [];
  });
  let imgCache = $state<Record<string, string>>({});

  $effect(() => {
    for (const a of userAttachments) {
      if (!a.is_image || a.id in imgCache) continue;
      const id = a.id;
      void attachmentReadDataUrl(id)
        .then((url) => (imgCache = { ...imgCache, [id]: url }))
        .catch(() => (imgCache = { ...imgCache, [id]: "" }));
    }
  });

  let textBlocks = $derived(message.blocks.filter((b) => b.type === "text"));
</script>

<div class="msg" class:user={isUser}>
  {#if isUser && editing}
    <div class="edit-box">
      <textarea bind:value={editText} rows="3"></textarea>
      <div class="edit-actions">
        <button class="apr no" onclick={cancelEdit}>{m.message_edit_cancel()}</button>
        <button class="apr yes" onclick={saveEdit}>{m.message_edit_save()}</button>
      </div>
    </div>
  {:else}
    <div class="msg-body" class:user={isUser}>
      {#if isUser && !editing}
        <button class="msg-action" title={m.message_edit()} onclick={startEdit}>
          <Pencil size={12} />
        </button>
      {/if}

      {#if isUser && userAttachments.length > 0}
        <div class="attachments">
          {#each userAttachments as a (a.id)}
            {#if a.is_image && imgCache[a.id]}
              <img class="att-image" src={imgCache[a.id]} alt={a.file_name} title={a.file_name} />
            {:else if a.is_image && a.id in imgCache}
              <span class="att-missing" title={m.attachment_not_found()}>
                <ImageOff size={14} />
                <span class="att-missing-name">{a.file_name}</span>
              </span>
            {:else}
              <span
                class="att-chip"
                title={a.file_name}
                aria-label="{(a.is_image ? m.attachment_image() : m.attachment_file()) + ": " + a.file_name}"
              >
                {#if a.is_image}
                  <ImageIcon size={12} />
                {:else}
                  <FileIcon size={12} />
                {/if}
                <span class="att-chip-name">{a.file_name}</span>
                {#if !a.is_image}
                  <span class="att-chip-size">{formatSize(a.file_size)}</span>
                {/if}
              </span>
            {/if}
          {/each}
        </div>
      {/if}

      {#each textBlocks as b (b.id)}
        <div class="prose">{@html renderMarkdown(b.text ?? "")}</div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .msg {
    display: flex;
    margin: 0.5rem 0;
    position: relative;
  }
  .msg.user {
    justify-content: flex-end;
  }
  .msg-body {
    flex: 1 1 auto;
    min-width: 0;
    padding: 0.625rem 0.875rem;
    border-radius: var(--radius-lg);
    background: var(--secondary);
    color: var(--secondary-foreground);
    font-size: 0.875rem;
    line-height: 1.5;
    word-break: break-word;
    overflow-wrap: break-word;
  }
  .msg-body.user {
    flex: 0 1 auto;
    max-width: 78%;
    background: var(--primary);
    color: var(--primary-foreground);
  }
  .attachments {
    display: flex;
    flex-wrap: wrap;
    gap: 0.375rem;
    margin-bottom: 0.375rem;
  }
  .att-image {
    display: block;
    width: 96px;
    height: 96px;
    object-fit: cover;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
  }
  .att-missing {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.2rem;
    width: 96px;
    height: 96px;
    padding: 0.4rem;
    border: 1px dashed var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--muted-foreground);
    font-size: 0.65rem;
    line-height: 1.2;
    text-align: center;
  }
  .att-missing :global(svg) {
    flex-shrink: 0;
  }
  .att-missing-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .att-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    max-width: 240px;
    padding: 0.2rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.75rem;
    line-height: 1.4;
  }
  .att-chip :global(svg) {
    flex-shrink: 0;
  }
  .att-chip-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .att-chip-size {
    color: var(--muted-foreground);
    font-size: 0.7rem;
    white-space: nowrap;
  }
  .prose :global(p) { margin: 0.4em 0; }
  .prose :global(p:first-child) { margin-top: 0; }
  .prose :global(p:last-child) { margin-bottom: 0; }
  .prose :global(pre) {
    margin: 0.5em 0;
    padding: 0.625rem 0.75rem;
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: 0.8125rem;
  }
  .prose :global(code) { font-family: var(--font-mono); font-size: 0.8125rem; }
  .prose :global(:not(pre) > code) { padding: 0.1em 0.3em; background: var(--muted); border-radius: 4px; }
  .prose :global(ul), .prose :global(ol) { margin: 0.4em 0; padding-left: 1.25em; }
  .prose :global(ul) { list-style: disc; }
  .prose :global(ol) { list-style: decimal; }
  .prose :global(ul ul) { list-style: circle; }
  .prose :global(li) { display: list-item; margin: 0.15em 0; }
  .prose :global(li > p) { margin: 0; }

  .prose :global(h1) { margin: 0.7em 0 0.3em; font-size: 1.25em; font-weight: 700; }
  .prose :global(h2) { margin: 0.65em 0 0.3em; font-size: 1.12em; font-weight: 700; }
  .prose :global(h3) { margin: 0.6em 0 0.3em; font-size: 1em; font-weight: 600; }
  .prose :global(h4), .prose :global(h5), .prose :global(h6) { margin: 0.6em 0 0.25em; font-size: 0.95em; font-weight: 600; }
  .prose :global(:is(h1, h2, h3, h4, h5, h6):first-child) { margin-top: 0; }

  .prose :global(.table-wrap) {
    display: block;
    max-width: 100%;
    overflow-x: auto;
    margin: 0.5em 0;
  }
  .prose :global(table) {
    display: table;
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85em;
  }
  .prose :global(th),
  .prose :global(td) {
    border: 1px solid var(--border);
    padding: 0.3em 0.5em;
    text-align: left;
    vertical-align: top;
  }
  .prose :global(th) { background: var(--muted); font-weight: 600; }
  .prose :global(tbody tr:nth-child(even)) { background: color-mix(in srgb, var(--muted) 50%, transparent); }

  .prose :global(a) { color: hsl(210 90% 50%); text-decoration: none; }
  .prose :global(a:hover) { text-decoration: underline; }
  .prose :global(a:visited) { color: hsl(210 90% 50%); }

  .prose :global(blockquote) {
    margin: 0.4em 0;
    padding-left: 0.75em;
    border-left: 3px solid var(--border);
    color: var(--muted-foreground);
    font-style: italic;
  }
  .prose :global(blockquote blockquote) {
    border-left-width: 4px;
    border-left-color: var(--muted-foreground);
  }

  .prose :global(hr) { border: none; border-top: 1px solid var(--border); margin: 0.6em 0; }
  .prose :global(img) { max-width: 100%; }
  .prose :global(em) { font-style: italic; }
  .prose :global(strong) { font-weight: 600; }

  .msg-action {
    position: absolute;
    top: 0.3rem;
    right: 0.3rem;
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    color: var(--muted-foreground);
    opacity: 0;
    border-radius: var(--radius-sm);
    padding: 0.15rem;
    cursor: default;
    z-index: 1;
  }
  .msg:hover .msg-action { opacity: 0.7; }
  .msg-action:hover { opacity: 1; background: var(--accent); }
  .msg.user .msg-action { color: var(--primary-foreground); }
  .edit-box {
    max-width: 78%;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .edit-box textarea {
    width: 100%;
    padding: 0.5rem 0.625rem;
    border: 1px solid var(--ring);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-sans);
    outline: none;
    resize: vertical;
  }
  .edit-actions { display: flex; gap: 0.4rem; justify-content: flex-end; }
  .apr {
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 0.75rem;
    cursor: default;
  }
  .apr.yes { background: hsl(140 60% 40%); color: white; border-color: hsl(140 60% 40%); }
  .apr.no { background: var(--destructive); color: var(--destructive-foreground); border-color: var(--destructive); }
</style>