<script lang="ts">
  import {
    Square,
    ArrowUp,
    Paperclip,
    X,
    Image as ImageIcon,
    File as FileIcon,
    Sparkles,
    Check,
  } from "@lucide/svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { get } from "svelte/store";
  import { tick, type Snippet } from "svelte";
  import { m } from "$lib/i18n";
  import { sendMessage, cancelTurn, currentChatId } from "$lib/stores/chat";
  import { skills as skillsStore } from "$lib/stores/skills";
  import { attachmentAdd, attachmentRemove, attachmentReadDataUrl, type Attachment, type Skill } from "$lib/tauri";
  import { COMPOSER_SEND_EVENT, FOCUS_COMPOSER_EVENT, DROP_FILES_EVENT, COMPOSER_INSERT_EVENT } from "$lib/events";
  import { formatSize } from "$lib/utils";

  let { running = false, inProject = false, modelSelected = true, children }: {
    running?: boolean;
    inProject?: boolean;
    modelSelected?: boolean;
    children?: Snippet;
  } = $props();

  let text = $state("");
  let textareaEl: HTMLTextAreaElement | undefined = $state();
  let pending = $state<Attachment[]>([]);
  let thumbCache = $state<Record<string, string>>({});
  let selectedSkillIds = $state<string[]>([]);
  let skillsOpen = $state(false);

  const MAX_ROWS = 12;

  let placeholder = $derived(inProject ? m.composer_hint_project() : m.composer_placeholder());
  let canSend = $derived((Boolean(text.trim()) || pending.length > 0) && modelSelected);
  let availableSkills = $derived<Skill[]>($skillsStore);
  let selectedSkills = $derived(
    selectedSkillIds
      .map((id) => availableSkills.find((s) => s.id === id))
      .filter((s): s is Skill => Boolean(s)),
  );

  function autoResize() {
    const el = textareaEl;
    if (!el) return;
    el.style.height = "auto";
    const cs = getComputedStyle(el);
    const line = parseFloat(cs.lineHeight) || (parseFloat(cs.fontSize) || 14) * 1.4;
    const padding = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);
    const borders = parseFloat(cs.borderTopWidth) + parseFloat(cs.borderBottomWidth);
    const maxHeight = line * MAX_ROWS + padding + borders;
    el.style.height = Math.min(el.scrollHeight + borders, maxHeight) + "px";
  }

  // Pending attachments belong to the chat they were added to; reset on chat switch.
  $effect(() => {
    if ($currentChatId) {
      pending = [];
      thumbCache = {};
      selectedSkillIds = [];
      skillsOpen = false;
    }
  });

  async function addPaths(paths: string[]) {
    const chatId = get(currentChatId);
    if (!chatId) return;
    for (const p of paths) {
      if (get(currentChatId) !== chatId) return;
      let att: Attachment;
      try {
        att = await attachmentAdd(chatId, p);
      } catch (e) {
        console.error("attachmentAdd failed", e);
        continue;
      }
      if (get(currentChatId) !== chatId) return;
      pending = [...pending, att];
      if (att.is_image) {
        let url: string;
        try {
          url = await attachmentReadDataUrl(att.id);
        } catch (e) {
          console.error("attachmentReadDataUrl failed", e);
          continue;
        }
        if (get(currentChatId) !== chatId) return;
        thumbCache = { ...thumbCache, [att.id]: url };
      }
    }
  }

  async function removePending(id: string) {
    try {
      await attachmentRemove(id);
    } catch (e) {
      console.error("attachmentRemove failed", e);
    }
    pending = pending.filter((a) => a.id !== id);
    const next = { ...thumbCache };
    delete next[id];
    thumbCache = next;
  }

  async function pickFiles() {
    try {
      const res = await open({ multiple: true });
      if (res) void addPaths(Array.isArray(res) ? res : [res]);
    } catch (e) {
      console.error("file dialog failed", e);
    }
  }

  function toggleSkill(id: string) {
    if (selectedSkillIds.includes(id)) {
      selectedSkillIds = selectedSkillIds.filter((x) => x !== id);
    } else {
      selectedSkillIds = [...selectedSkillIds, id];
    }
  }

  function removeSkill(id: string) {
    selectedSkillIds = selectedSkillIds.filter((x) => x !== id);
  }

  function toggleSkillsPanel() {
    skillsOpen = !skillsOpen;
  }

  function submit() {
    if (running || !modelSelected) return;
    const t = text.trim();
    const ids = [...pending.map((a) => a.id)];
    const sIds = [...selectedSkillIds];
    if (!t && ids.length === 0) return;
    text = "";
    pending = [];
    thumbCache = {};
    selectedSkillIds = [];
    skillsOpen = false;
    void sendMessage(t, ids, sIds);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  // Grow the textarea to fit its content (CSS min/max-height clamp 3..12 rows).
  $effect(() => {
    void text;
    autoResize();
  });

  // Hotkey-driven send (see hotkeys in config.toml), e.g. CmdOrCtrl+Enter.
  $effect(() => {
    const onExternalSend = () => submit();
    window.addEventListener(COMPOSER_SEND_EVENT, onExternalSend);
    return () => window.removeEventListener(COMPOSER_SEND_EVENT, onExternalSend);
  });

  // Focus on newly created chats (dispatched by ChatView).
  $effect(() => {
    const onFocus = () => {
      textareaEl?.focus();
      autoResize();
    };
    window.addEventListener(FOCUS_COMPOSER_EVENT, onFocus);
    return () => window.removeEventListener(FOCUS_COMPOSER_EVENT, onFocus);
  });

  // Files dropped onto the chat area (dispatched by ChatView drag-drop overlay).
  $effect(() => {
    const onDropFiles = (e: Event) => {
      const paths = (e as CustomEvent<{ paths?: string[] }>).detail?.paths ?? [];
      if (paths.length) void addPaths(paths);
    };
    window.addEventListener(DROP_FILES_EVENT, onDropFiles);
    return () => window.removeEventListener(DROP_FILES_EVENT, onDropFiles);
  });

  // @file refs inserted by ContextPanel (per-row insert / "Pull all").
  $effect(() => {
    const onInsert = (e: Event) => {
      const t = (e as CustomEvent<{ text?: string }>).detail?.text;
      if (!t) return;
      text = text && !text.endsWith("\n") ? text + "\n" + t : text + t;
      void tick().then(() => {
        textareaEl?.focus();
        autoResize();
      });
    };
    window.addEventListener(COMPOSER_INSERT_EVENT, onInsert);
    return () => window.removeEventListener(COMPOSER_INSERT_EVENT, onInsert);
  });
</script>

<div class="composer">
  {#if skillsOpen}
    <div class="skills-backdrop" onclick={() => (skillsOpen = false)} role="presentation"></div>
  {/if}
  {#if selectedSkills.length > 0 || pending.length > 0}
    <div class="composer-attachments">
      {#if selectedSkills.length > 0}
        <div class="skill-badges">
          {#each selectedSkills as skill (skill.id)}
            <span class="skill-badge" title={skill.body}>
              <Sparkles size={11} />
              <span class="skill-badge-name">{skill.title}</span>
              <button
                class="skill-badge-x"
                title={m.common_close()}
                onclick={() => removeSkill(skill.id)}
              >
                <X size={10} />
              </button>
            </span>
          {/each}
        </div>
      {/if}
      {#if pending.length > 0}
        <div class="attachments">
          {#each pending as a (a.id)}
            <div class="att-badge" title={a.file_name}>
              {#if a.is_image && thumbCache[a.id]}
                <img class="att-thumb" src={thumbCache[a.id]} alt={a.file_name} />
              {:else if a.is_image}
                <span class="att-icon"><ImageIcon size={14} /></span>
              {:else}
                <span class="att-icon"><FileIcon size={14} /></span>
              {/if}
              <span class="att-name">{a.file_name}</span>
              {#if !a.is_image}
                <span class="att-size">{formatSize(a.file_size)}</span>
              {/if}
              <button class="att-remove" title={m.attachment_remove()} onclick={() => void removePending(a.id)}>
                <X size={12} />
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
  <div class="composer-textarea">
    <textarea
      bind:this={textareaEl}
      bind:value={text}
      onkeydown={onKeydown}
      placeholder={modelSelected ? placeholder : ""}
      spellcheck="false"
      disabled={!modelSelected}
    ></textarea>
    {#if !modelSelected}
      <div class="model-hint">Select a model to start</div>
    {/if}
  </div>
  <div class="composer-bottom">
    <div class="composer-left">
      <div class="skills-wrap">
        <button
          class="btn attach"
          title={m.composer_skills()}
          class:active={selectedSkillIds.length > 0}
          onclick={(e) => {
            e.stopPropagation();
            toggleSkillsPanel();
          }}
        >
          <Sparkles size={15} />
        </button>
        {#if skillsOpen}
          <div
            class="skills-dropdown"
            role="menu"
            tabindex="-1"
            onclick={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              if (e.key === "Escape") skillsOpen = false;
            }}
          >
            {#if availableSkills.length === 0}
              <div class="skills-empty">{m.composer_skills_empty()}</div>
            {:else}
              {#each availableSkills as skill (skill.id)}
                <button
                  class="skill-option"
                  class:checked={selectedSkillIds.includes(skill.id)}
                  onclick={() => toggleSkill(skill.id)}
                  role="menuitemcheckbox"
                  aria-checked={selectedSkillIds.includes(skill.id)}
                >
                  <span class="skill-check">
                    {#if selectedSkillIds.includes(skill.id)}<Check size={13} />{/if}
                  </span>
                  <span class="skill-label">
                    <span class="skill-opt-title">{skill.title}</span>
                    <span class="skill-opt-body">{skill.body}</span>
                  </span>
                </button>
              {/each}
            {/if}
          </div>
        {/if}
      </div>
      <button class="btn attach" title={m.composer_attach()} onclick={() => void pickFiles()}>
        <Paperclip size={15} />
      </button>
    </div>
    <div class="composer-right">
      {@render children?.()}
      {#if running}
        <button class="btn stop" title={m.composer_stop()} onclick={cancelTurn}>
          <Square size={14} />
        </button>
      {:else}
        <button class="btn send" title={m.composer_send()} onclick={submit} disabled={!canSend}>
          <ArrowUp size={16} />
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.6rem 0.5rem;
    background: var(--background);
  }
  .composer-attachments {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.375rem;
    padding: 0 0.25rem;
    min-height: 0;
  }
  .composer-textarea {
    position: relative;
    width: 100%;
  }
  .model-hint {
    position: absolute;
    inset: 0;
    padding: 0.5rem 0.75rem;
    display: flex;
    align-items: center;
    pointer-events: none;
    font-size: 0.8125rem;
    font-weight: 500;
    background: linear-gradient(
      90deg,
      hsl(48 90% 60%),
      hsl(28 85% 55%),
      hsl(0 78% 55%),
      hsl(28 85% 55%),
      hsl(48 90% 60%)
    );
    background-size: 200% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    animation: hint-pulse 2.4s ease-in-out infinite, hint-shift 6s linear infinite;
  }
  @keyframes hint-pulse {
    0%,
    100% {
      opacity: 0.6;
    }
    50% {
      opacity: 1;
    }
  }
  @keyframes hint-shift {
    to {
      background-position: 200% 0;
    }
  }
  .composer-bottom {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    justify-content: space-between;
    flex-wrap: wrap;
  }
  .composer-left {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    flex-wrap: wrap;
  }
  .composer-right {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .attachments {
    display: flex;
    flex-wrap: wrap;
    gap: 0.375rem;
    min-width: 0;
  }
  .att-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    max-width: 260px;
    padding: 0.2rem 0.3rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    font-size: 0.75rem;
    color: var(--foreground);
  }
  .att-thumb,
  .att-icon {
    width: 26px;
    height: 26px;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }
  .att-thumb {
    object-fit: cover;
    display: block;
  }
  .att-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--muted-foreground);
  }
  .att-name {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .att-size {
    color: var(--muted-foreground);
    white-space: nowrap;
  }
  .att-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted-foreground);
    cursor: default;
    flex-shrink: 0;
  }
  .att-remove:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  textarea {
    width: 100%;
    resize: none;
    overflow-y: auto;
    min-height: calc(1.4em * 3 + 1rem);
    max-height: calc(1.4em * 12 + 1rem);
    padding: 0.5rem 0.75rem;
    border: none;
    border-radius: var(--radius-md);
    background: var(--muted);
    color: var(--foreground);
    font-family: var(--font-sans);
    font-size: 0.875rem;
    line-height: 1.4;
    outline: none;
  }
  textarea:focus {
    outline: 1px solid var(--ring);
    outline-offset: -1px;
  }
  textarea:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 1.75rem;
    width: 1.75rem;
    border: none;
    border-radius: var(--radius-md);
    cursor: default;
  }
  .btn.attach {
    background: transparent;
    color: var(--muted-foreground);
  }
  .btn.attach:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .btn.send {
    background: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.send:disabled {
    opacity: 0.5;
  }
  .btn.stop {
    background: var(--destructive);
    color: var(--destructive-foreground);
  }
  .skills-wrap {
    position: relative;
    display: inline-flex;
  }
  .btn.attach.active {
    color: var(--primary);
  }
  .skills-backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }
  .skills-dropdown {
    position: absolute;
    bottom: calc(100% + 0.25rem);
    left: 0;
    z-index: 41;
    min-width: 260px;
    max-width: 360px;
    max-height: 320px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    padding: 0.3rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.18);
  }
  .skills-empty {
    padding: 0.5rem;
    font-size: 0.75rem;
    color: var(--muted-foreground);
  }
  .skill-option {
    display: flex;
    align-items: flex-start;
    gap: 0.4rem;
    padding: 0.35rem 0.4rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--foreground);
    text-align: left;
    cursor: default;
  }
  .skill-option:hover {
    background: var(--accent);
  }
  .skill-option.checked {
    background: var(--secondary);
  }
  .skill-check {
    width: 14px;
    flex-shrink: 0;
    color: var(--primary);
    display: inline-flex;
    align-items: center;
    padding-top: 0.1rem;
  }
  .skill-label {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    min-width: 0;
  }
  .skill-opt-title {
    font-size: 0.78rem;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .skill-opt-body {
    font-size: 0.7rem;
    color: var(--muted-foreground);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .skill-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 0.375rem;
    min-width: 0;
  }
  .skill-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    max-width: 240px;
    padding: 0.2rem 0.4rem;
    border: 1px solid var(--primary);
    border-radius: var(--radius-md);
    background: var(--secondary);
    color: var(--primary);
    font-size: 0.72rem;
  }
  .skill-badge-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .skill-badge-x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    background: transparent;
    color: inherit;
    cursor: default;
    flex-shrink: 0;
  }
  .skill-badge-x:hover {
    opacity: 0.7;
  }
</style>