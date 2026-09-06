<script lang="ts">
  import { X } from "@lucide/svelte";
  import { m } from "$lib/i18n";
  import { activityDayDetail, type DayDetail, type DayProjectActivity } from "$lib/tauri";

  let {
    date,
    onclose,
  }: {
    date: string;
    onclose: () => void;
  } = $props();

  let detail = $state<DayDetail | null>(null);
  let loading = $state(false);

  const n = (v: number) => v.toLocaleString();

  $effect(() => {
    if (!date) return;
    loading = true;
    detail = null;
    let cancelled = false;
    activityDayDetail(date)
      .then((d) => {
        if (cancelled) return;
        detail = d;
        loading = false;
      })
      .catch((e) => {
        if (cancelled) return;
        console.error("activityDayDetail failed", e);
        loading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onclose();
    };
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  let dateLabel = $derived.by(() => {
    const parts = date.split("-").map(Number);
    if (parts.length !== 3 || parts.some((p) => !Number.isFinite(p))) return date;
    const [y, mo, d] = parts;
    const parsed = new Date(y, mo - 1, d);
    if (Number.isNaN(parsed.getTime())) return date;
    return parsed.toLocaleDateString(undefined, {
      weekday: "long",
      day: "numeric",
      month: "long",
      year: "numeric",
    });
  });

  function projectLabel(p: DayProjectActivity): string {
    if (p.project_name) return p.project_name;
    if (p.project_id === null) return m.activity_standalone();
    return p.project_id;
  }
</script>

<div class="overlay" role="presentation" onkeydown={onKeydown}>
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
  >
    <header class="head">
      <span class="date">{dateLabel}</span>
      <button class="x" title={m.common_close()} onclick={onclose}><X size={15} /></button>
    </header>
    <div class="body">
      {#if loading && !detail}
        <div class="pending">…</div>
      {:else if detail}
        <div class="stats">
          <div class="stat">
            <span class="num">{n(detail.requests)}</span>
            <span class="lbl">{m.activity_requests()}</span>
          </div>
          <div class="stat">
            <span class="num">{n(detail.messages)}</span>
            <span class="lbl">{m.activity_messages()}</span>
          </div>
          <div class="stat">
            <span class="num">{n(detail.total_tokens)}</span>
            <span class="lbl">{m.activity_tokens()}</span>
          </div>
        </div>
        {#if detail.messages === 0}
          <div class="empty">{m.activity_no_activity()}</div>
        {:else}
          {#if detail.projects.length > 0}
            <section class="section">
              <div class="sec-title">{m.activity_projects()}</div>
              <div class="list">
                {#each detail.projects as p (p.project_id ?? p.project_name ?? "standalone")}
                  <div class="row">
                    <span class="name">{projectLabel(p)}</span>
                    <span class="vals">{n(p.requests)} · {n(p.total_tokens)}</span>
                  </div>
                {/each}
              </div>
            </section>
          {/if}
          {#if detail.chats.length > 0}
            <section class="section">
              <div class="sec-title">{m.activity_chats()}</div>
              <div class="list">
                {#each detail.chats as c (c.chat_id)}
                  <div class="row">
                    <span class="name">{c.title}</span>
                    <span class="meta">{c.project_name ?? m.activity_standalone()}</span>
                    <span class="vals">{n(c.requests)} · {n(c.total_tokens)}</span>
                  </div>
                {/each}
              </div>
            </section>
          {/if}
        {/if}
      {:else}
        <div class="pending">—</div>
      {/if}
    </div>
  </div>
</div>

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
    width: 520px;
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
  .date {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  .stats {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    padding: 0.55rem 0 0.45rem;
    margin-bottom: 0.35rem;
    border-bottom: 1px solid var(--border);
  }
  .stat {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.1rem;
  }
  .stat + .stat {
    border-left: 1px solid var(--border);
  }
  .num {
    font-family: var(--font-mono);
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--foreground);
  }
  .lbl {
    font-size: 0.68rem;
    color: var(--muted-foreground);
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
  .list {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    min-width: 0;
    padding: 0.3rem 0.4rem;
    margin: 0 -0.4rem;
    border-radius: var(--radius-sm);
  }
  .row:hover {
    background: var(--accent);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.78rem;
    color: var(--foreground);
  }
  .meta {
    flex-shrink: 0;
    font-size: 0.68rem;
    color: var(--muted-foreground);
  }
  .vals {
    flex-shrink: 0;
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--muted-foreground);
    white-space: nowrap;
    text-align: right;
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