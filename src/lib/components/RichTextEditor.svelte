<script lang="ts">
  import { tick } from "svelte";
  import {
    Bold,
    Code,
    Eye,
    Heading,
    Italic,
    Link,
    List,
    Pencil,
  } from "@lucide/svelte";
  import { renderMarkdown } from "$lib/markdown";
  import { m } from "$lib/i18n";

  let {
    value = $bindable(""),
    placeholder = "",
    minHeight = "120px",
  }: {
    value?: string;
    placeholder?: string;
    minHeight?: string;
  } = $props();

  let mode: "edit" | "preview" = $state("edit");
  let textareaEl: HTMLTextAreaElement | undefined = $state();

  let previewHtml = $derived(renderMarkdown(value));

  async function restoreSelection(
    el: HTMLTextAreaElement,
    start: number,
    end: number,
  ) {
    await tick();
    if (textareaEl !== el || mode !== "edit") return;
    el.focus();
    el.setSelectionRange(start, end);
  }

  async function wrapSelection(before: string, after: string) {
    const el = textareaEl;
    if (!el || mode !== "edit") return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const selected = value.slice(start, end);
    value = value.slice(0, start) + before + selected + after + value.slice(end);
    await restoreSelection(
      el,
      start + before.length,
      start + before.length + selected.length,
    );
  }

  function prefixLines(prefix: string) {
    const el = textareaEl;
    if (!el || mode !== "edit") return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const lineStart = value.lastIndexOf("\n", start - 1) + 1;
    let lineEnd = value.indexOf("\n", end);
    if (lineEnd === -1) lineEnd = value.length;
    const block = value
      .slice(lineStart, lineEnd)
      .split("\n")
      .map((line) => (line === "" || line.startsWith(prefix) ? line : prefix + line))
      .join("\n");
    value = value.slice(0, lineStart) + block + value.slice(lineEnd);
    void restoreSelection(el, lineStart, lineStart + block.length);
  }

  async function insertLink() {
    const el = textareaEl;
    if (!el || mode !== "edit") return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const text = value.slice(start, end) || "text";
    value = value.slice(0, start) + `[${text}](url)` + value.slice(end);
    const urlStart = start + text.length + 3;
    await restoreSelection(el, urlStart, urlStart + 3);
  }

  function onKeydown(e: KeyboardEvent) {
    if (mode !== "edit" || !(e.ctrlKey || e.metaKey)) return;
    const key = e.key.toLowerCase();
    if (key === "b") {
      e.preventDefault();
      void wrapSelection("**", "**");
    } else if (key === "i") {
      e.preventDefault();
      void wrapSelection("*", "*");
    }
  }

  function toggleMode() {
    mode = mode === "edit" ? "preview" : "edit";
  }
</script>

<div class="editor">
  <div class="toolbar">
    <button
      class="tool"
      type="button"
      title={m.editor_bold()}
      disabled={mode === "preview"}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => void wrapSelection("**", "**")}
    >
      <Bold size={14} />
    </button>
    <button
      class="tool"
      type="button"
      title={m.editor_italic()}
      disabled={mode === "preview"}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => void wrapSelection("*", "*")}
    >
      <Italic size={14} />
    </button>
    <button
      class="tool"
      type="button"
      title={m.editor_heading()}
      disabled={mode === "preview"}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => prefixLines("## ")}
    >
      <Heading size={14} />
    </button>
    <button
      class="tool"
      type="button"
      title={m.editor_bullet_list()}
      disabled={mode === "preview"}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => prefixLines("- ")}
    >
      <List size={14} />
    </button>
    <button
      class="tool"
      type="button"
      title={m.editor_code()}
      disabled={mode === "preview"}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => void wrapSelection("`", "`")}
    >
      <Code size={14} />
    </button>
    <button
      class="tool"
      type="button"
      title={m.editor_insert_link()}
      disabled={mode === "preview"}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => void insertLink()}
    >
      <Link size={14} />
    </button>
    <span class="spacer"></span>
    <button
      class="tool"
      type="button"
      title={mode === "edit" ? m.editor_preview() : m.common_edit()}
      onclick={toggleMode}
    >
      {#if mode === "edit"}
        <Eye size={14} />
      {:else}
        <Pencil size={14} />
      {/if}
    </button>
  </div>
  {#if mode === "edit"}
    <textarea
      bind:this={textareaEl}
      bind:value={value}
      class="input"
      style:min-height={minHeight}
      {placeholder}
      onkeydown={onKeydown}
    ></textarea>
  {:else}
    <div class="preview" style:min-height={minHeight}>
      {#if previewHtml}
        {@html previewHtml}
      {:else}
        <span class="preview-empty">{m.editor_empty()}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 0.125rem;
  }
  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    padding: 0.25rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted-foreground);
    cursor: default;
  }
  .tool:hover:not(:disabled),
  .tool:focus-visible {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .tool:disabled {
    opacity: 0.4;
  }
  .spacer {
    flex: 1;
  }
  .input {
    width: 100%;
    box-sizing: border-box;
    min-width: 0;
    padding: 0.5rem 0.625rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-sans);
    line-height: 1.5;
    resize: vertical;
    outline: none;
  }
  .input:focus {
    border-color: var(--ring);
  }
  .preview {
    min-width: 0;
    padding: 0.5rem 0.625rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    line-height: 1.5;
    overflow-y: auto;
    overflow-wrap: break-word;
  }
  .preview :global(*:first-child) {
    margin-top: 0;
  }
  .preview :global(*:last-child) {
    margin-bottom: 0;
  }
  .preview :global(p),
  .preview :global(ul),
  .preview :global(ol) {
    margin: 0 0 0.5em;
  }
  .preview :global(ul),
  .preview :global(ol) {
    padding-left: 1.25rem;
  }
  .preview :global(h1),
  .preview :global(h2),
  .preview :global(h3),
  .preview :global(h4),
  .preview :global(h5),
  .preview :global(h6) {
    margin: 0.75em 0 0.35em;
    font-weight: 600;
    line-height: 1.3;
  }
  .preview :global(h1) {
    font-size: 1.25rem;
  }
  .preview :global(h2) {
    font-size: 1.125rem;
  }
  .preview :global(h3) {
    font-size: 1rem;
  }
  .preview :global(h4),
  .preview :global(h5),
  .preview :global(h6) {
    font-size: 0.9375rem;
  }
  .preview :global(blockquote) {
    margin: 0 0 0.5em;
    padding-left: 0.6em;
    border-left: 2px solid var(--border);
    color: var(--muted-foreground);
  }
  .preview :global(code) {
    font-family: var(--font-mono);
    font-size: 0.85em;
    padding: 0.1em 0.3em;
    border-radius: var(--radius-sm);
    background: var(--muted);
  }
  .preview :global(pre) {
    margin: 0 0 0.5em;
    padding: 0.5rem 0.625rem;
    border-radius: var(--radius-sm);
    background: var(--muted);
    overflow-x: auto;
    font-size: 0.78rem;
  }
  .preview :global(pre code) {
    padding: 0;
    background: transparent;
    font-size: inherit;
  }
  .preview :global(a) {
    color: hsl(260 70% 60%);
  }
  .preview :global(hr) {
    margin: 0.75em 0;
    border: none;
    border-top: 1px solid var(--border);
  }
  .preview :global(img) {
    max-width: 100%;
    border-radius: var(--radius-sm);
  }
  .preview :global(.table-wrap) {
    display: block;
    max-width: 100%;
    margin: 0 0 0.5em;
    overflow-x: auto;
  }
  .preview :global(table) {
    display: table;
    width: 100%;
    border-collapse: collapse;
    font-size: 0.78rem;
  }
  .preview :global(th),
  .preview :global(td) {
    padding: 0.2rem 0.5rem;
    border: 1px solid var(--border);
    text-align: left;
  }
  .preview :global(th) {
    background: var(--muted);
    font-weight: 600;
  }
  .preview-empty {
    color: var(--muted-foreground);
    font-style: italic;
  }
</style>