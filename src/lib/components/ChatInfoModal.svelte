<script lang="ts">
  import { X } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { chatInfo, type ChatInfo } from "$lib/tauri";

  let {
    open,
    chatId,
    onclose,
  }: {
    open: boolean;
    chatId: string | null;
    onclose: () => void;
  } = $props();

  let info = $state<ChatInfo | null>(null);
  let loading = $state(false);

  $effect(() => {
    if (!open || !chatId) {
      info = null;
      loading = false;
      return;
    }
    loading = true;
    let cancelled = false;
    chatInfo(chatId)
      .then((i) => {
        if (cancelled) return;
        info = i;
        loading = false;
      })
      .catch((e) => {
        if (cancelled) return;
        console.error("chatInfo failed", e);
        loading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (!open) return;
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onclose();
    };
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  type InfoRow = { label: string; value: string; mono?: boolean };
  type InfoSection = { title: string | null; rows: InfoRow[] };

  let sections = $derived.by((): InfoSection[] => {
    if (!info?.chat_id) return [];
    const n = (v: number) => v.toLocaleString();
    return [
      {
        title: m.info_general(),
        rows: [
          { label: m.info_title_label(), value: info.title },
          { label: m.info_project(), value: info.project_name ?? m.info_no_project() },
          { label: m.info_provider(), value: info.provider_name || "Undefined" },
          { label: m.info_model(), value: info.model_display_name || "Undefined" },
          { label: m.info_created(), value: fmtDate(info.created_at) },
          { label: m.info_updated(), value: fmtDate(info.updated_at) },
        ],
      },
      {
        title: m.info_messages(),
        rows: [
          { label: m.info_message_count(), value: n(info.message_count), mono: true },
          { label: m.info_user_messages(), value: n(info.user_messages), mono: true },
          { label: m.info_assistant_messages(), value: n(info.assistant_messages), mono: true },
          { label: m.info_tool_messages(), value: n(info.tool_messages), mono: true },
          { label: m.info_tool_calls(), value: n(info.tool_calls), mono: true },
          { label: m.info_attachments(), value: n(info.attachments), mono: true },
          { label: m.info_project_paths(), value: n(info.project_paths), mono: true },
          { label: m.info_chat_paths(), value: n(info.chat_paths), mono: true },
        ],
      },
      {
        title: m.info_tokens(),
        rows: [
          { label: m.info_prompt_tokens(), value: n(info.total_prompt_tokens), mono: true },
          { label: m.info_completion_tokens(), value: n(info.total_completion_tokens), mono: true },
          { label: m.info_total_tokens(), value: n(info.total_tokens), mono: true },
        ],
      },
      {
        title: m.info_durations(),
        rows: [
          { label: m.info_total_duration(), value: fmtMs(info.total_duration_ms), mono: true },
          { label: m.info_ttft(), value: fmtMs(info.total_ttft_ms), mono: true },
          { label: m.info_generation(), value: fmtMs(info.total_generation_ms), mono: true },
          {
            label: m.info_avg_prompt_rate(),
            value: fmtRate(info.total_prompt_tokens, info.total_ttft_ms),
            mono: true,
          },
          {
            label: m.info_avg_gen_rate(),
            value: fmtRate(info.total_completion_tokens, info.total_generation_ms),
            mono: true,
          },
        ],
      },
      {
        title: null,
        rows: [
          { label: m.info_first_message(), value: fmtDate(info.first_message_at) },
          { label: m.info_last_message(), value: fmtDate(info.last_message_at) },
        ],
      },
    ];
  });

  function fmtMs(ms: number): string {
    if (ms < 1000) return `${Math.round(ms)}ms`;
    const s = ms / 1000;
    if (s < 60) return `${s.toFixed(1)}s`;
    return `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`;
  }

  function fmtDate(ms: number | null): string {
    if (ms == null || ms <= 0) return "—";
    return new Date(ms).toLocaleString();
  }

  function fmtRate(tokens: number, ms: number): string {
    if (ms <= 0) return "—";
    return `${(tokens / (ms / 1000)).toFixed(1)} tok/s`;
  }
</script>

{#if open}
  <div class="overlay" role="presentation" onkeydown={onKeydown}>
    <div
      class="dialog"
      role="dialog"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <header class="head">
        <span class="name">{m.info_title()}</span>
        <button class="x" title={m.common_close()} onclick={onclose}><X size={15} /></button>
      </header>
      <div class="body">
        {#if loading && !info}
          <div class="pending">…</div>
        {:else if info && !info.chat_id}
          <div class="empty">—</div>
        {:else if info}
          {#each sections as sec (sec.title ?? "timeline")}
            <section class="section">
              {#if sec.title}<div class="sec-title">{sec.title}</div>{/if}
              <div class="rows">
                {#each sec.rows as row (row.label)}
                  <div class="row">
                    <span class="k">{row.label}</span>
                    <span class="v" class:mono={row.mono}>{row.value}</span>
                  </div>
                {/each}
              </div>
            </section>
          {/each}
        {:else}
          <div class="pending">—</div>
        {/if}
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
    background-color: rgb(0 0 0 / 0.5);
  }
  .dialog {
    width: 480px;
    max-width: 92vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background-color: var(--background);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    padding: 0.15rem;
    cursor: default;
  }
  .x:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.15rem 0.875rem 0.6rem;
  }
  .section {
    padding: 0.5rem 0;
  }
  .section + .section {
    border-top: 1px solid var(--border);
  }
  .sec-title {
    font-size: 0.72rem;
    font-weight: 600;
    margin-bottom: 0.3rem;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    min-width: 0;
  }
  .k {
    flex-shrink: 0;
    width: 120px;
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }
  .v {
    flex: 1;
    min-width: 0;
    text-align: right;
    overflow-wrap: anywhere;
    font-size: 0.78rem;
    color: var(--foreground);
  }
  .v.mono {
    font-family: var(--font-mono);
    font-size: 0.72rem;
  }
  .pending {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1.5rem 0;
    color: var(--muted-foreground);
  }
  .empty {
    padding: 1rem 0;
    text-align: center;
    color: var(--muted-foreground);
    font-size: 0.78rem;
  }
</style>