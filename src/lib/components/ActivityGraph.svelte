<script lang="ts">
  import { onMount } from "svelte";
  import { m } from "$lib/i18n";
  import { activityDaily, type DayActivity } from "$lib/tauri";
  import ActivityDayModal from "./ActivityDayModal.svelte";

  const CELL = 13;
  const GAP = 3;
  const LABEL_W = 28;
  const MIN_WEEKS = 8;
  const MAX_WEEKS = 52;

  interface DayCell {
    key: string;
    date: Date;
    level: number;
    requests: number;
    clickable: boolean;
  }

  interface WeekColumn {
    key: string;
    sunday: Date;
    days: DayCell[];
  }

  let data = $state<Map<string, DayActivity>>(new Map());
  let loading = $state(true);
  let containerW = $state(0);
  let selectedDate = $state<string | null>(null);
  let gridEl: HTMLDivElement | undefined = $state();

  const today = todayLocal();
  const weekdayNames = Array.from({ length: 7 }, (_, i) =>
    addDays(new Date(2024, 0, 7), i).toLocaleDateString(undefined, { weekday: "short" }),
  );

  let numWeeks = $derived.by(() => {
    if (containerW <= 0) return MIN_WEEKS;
    const fit = Math.floor((containerW - LABEL_W) / (CELL + GAP));
    return Math.min(MAX_WEEKS, Math.max(MIN_WEEKS, fit));
  });

  let weeks = $derived.by(() => {
    const currentSunday = addDays(today, -today.getDay());
    const columns: WeekColumn[] = [];
    for (let i = 0; i < numWeeks; i++) {
      const sunday = addDays(currentSunday, -(numWeeks - 1 - i) * 7);
      const days: DayCell[] = [];
      for (let d = 0; d < 7; d++) {
        const date = addDays(sunday, d);
        const key = toKey(date);
        const activity = data.get(key);
        const future = date.getTime() > today.getTime();
        days.push({
          key,
          date,
          level: future || !activity ? 0 : levelOf(activity.requests),
          requests: future || !activity ? 0 : activity.requests,
          clickable: !future,
        });
      }
      columns.push({ key: toKey(sunday), sunday, days });
    }
    return columns;
  });

  let monthLabels = $derived.by(() => {
    const labels = new Map<number, string>();
    let prevMonth = -1;
    weeks.forEach((col, i) => {
      const month = col.sunday.getMonth();
      if (i === 0) {
        if (weeks[1] && weeks[1].sunday.getMonth() !== month) {
          labels.set(0, monthName(col.sunday));
        }
      } else if (month !== prevMonth) {
        labels.set(i, monthName(col.sunday));
      }
      prevMonth = month;
    });
    return labels;
  });

  $effect(() => {
    if (!gridEl) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) containerW = entry.contentRect.width;
    });
    observer.observe(gridEl);
    return () => observer.disconnect();
  });

  onMount(async () => {
    try {
      const rows = await activityDaily();
      const map = new Map<string, DayActivity>();
      for (const row of rows) map.set(row.date, row);
      data = map;
    } catch (err) {
      console.error("activity_daily failed:", err);
      data = new Map();
    } finally {
      loading = false;
    }
  });

  function todayLocal(): Date {
    const n = new Date();
    return new Date(n.getFullYear(), n.getMonth(), n.getDate());
  }

  function addDays(d: Date, n: number): Date {
    return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
  }

  function toKey(d: Date): string {
    const mm = String(d.getMonth() + 1).padStart(2, "0");
    const dd = String(d.getDate()).padStart(2, "0");
    return `${d.getFullYear()}-${mm}-${dd}`;
  }

  function levelOf(req: number): number {
    if (req <= 0) return 0;
    if (req === 1) return 1;
    if (req <= 3) return 2;
    if (req <= 5) return 3;
    return 4;
  }

  function monthName(d: Date): string {
    return d.toLocaleDateString(undefined, { month: "short" });
  }

  function formatDateLong(d: Date): string {
    return d.toLocaleDateString(undefined, {
      weekday: "short",
      day: "numeric",
      month: "short",
      year: "numeric",
    });
  }

  function cellTitle(cell: DayCell): string {
    const prefix =
      cell.requests > 0 ? `${cell.requests} ${m.activity_requests()}` : m.activity_no_activity();
    return `${prefix} · ${formatDateLong(cell.date)}`;
  }

  function onCellKeydown(e: KeyboardEvent, cell: DayCell) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      selectedDate = cell.key;
    }
  }
</script>

<div class="activity-graph">
  <div class="head">
    <span class="title">{m.activity_title()}</span>
    <div class="legend">
      <span class="legend-label">{m.activity_legend_less()}</span>
      {#each [0, 1, 2, 3, 4] as lvl (lvl)}
        <span class="swatch l{lvl}"></span>
      {/each}
      <span class="legend-label">{m.activity_legend_more()}</span>
    </div>
  </div>

  <div class="grid-wrap" bind:this={gridEl}>
    {#if loading}
      <div class="loading">{m.activity_loading()}</div>
    {:else}
      <div class="month-row">
        <div class="row-spacer"></div>
        {#each weeks as col, i (col.key)}
          <div class="month-slot">{monthLabels.get(i) ?? ""}</div>
        {/each}
      </div>
      <div class="grid-row">
        <div class="weekday-col">
          {#each weekdayNames as name, i (i)}
            <div class="weekday-slot">{i % 2 === 1 ? name : ""}</div>
          {/each}
        </div>
        <div class="columns">
          {#each weeks as col (col.key)}
            <div class="column">
              {#each col.days as cell (cell.key)}
                {#if cell.clickable}
                  <div
                    class="cell clickable l{cell.level}"
                    role="button"
                    tabindex="0"
                    title={cellTitle(cell)}
                    onclick={() => (selectedDate = cell.key)}
                    onkeydown={(e) => onCellKeydown(e, cell)}
                  ></div>
                {:else}
                  <div class="cell l{cell.level}"></div>
                {/if}
              {/each}
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>

{#if selectedDate}
  <ActivityDayModal date={selectedDate} onclose={() => (selectedDate = null)} />
{/if}

<style>
  .activity-graph {
    --hm-l0: var(--muted);
    --hm-l1: hsl(140 50% 80%);
    --hm-l2: hsl(140 55% 63%);
    --hm-l3: hsl(140 60% 45%);
    --hm-l4: hsl(140 65% 30%);
    width: 100%;
    max-width: 900px;
  }
  :global(.dark) .activity-graph {
    --hm-l0: hsl(240 3.7% 15.9%);
    --hm-l1: hsl(140 45% 26%);
    --hm-l2: hsl(140 50% 36%);
    --hm-l3: hsl(140 55% 48%);
    --hm-l4: hsl(140 60% 60%);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 6px;
  }
  .title {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--foreground);
  }
  .legend {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .legend-label {
    margin: 0 3px;
    font-size: 0.65rem;
    color: var(--muted-foreground);
  }
  .swatch {
    width: 11px;
    height: 11px;
    border-radius: 2px;
  }
  .l0 {
    background: var(--hm-l0);
  }
  .l1 {
    background: var(--hm-l1);
  }
  .l2 {
    background: var(--hm-l2);
  }
  .l3 {
    background: var(--hm-l3);
  }
  .l4 {
    background: var(--hm-l4);
  }
  .grid-wrap {
    width: 100%;
  }
  .loading {
    padding: 4px 0;
    font-size: 0.8rem;
    color: var(--muted-foreground);
    opacity: 0.6;
  }
  .month-row {
    display: flex;
    height: 14px;
    margin-bottom: 2px;
  }
  .row-spacer {
    width: 28px;
    flex: none;
  }
  .month-slot {
    width: 16px;
    flex: none;
    overflow: visible;
    font-size: 0.6rem;
    line-height: 14px;
    color: var(--muted-foreground);
    white-space: nowrap;
  }
  .grid-row {
    display: flex;
  }
  .weekday-col {
    display: flex;
    flex-direction: column;
    gap: 3px;
    width: 28px;
    flex: none;
  }
  .weekday-slot {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    height: 13px;
    font-size: 0.6rem;
    line-height: 1;
    color: var(--muted-foreground);
  }
  .columns {
    display: flex;
    gap: 3px;
  }
  .column {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .cell {
    box-sizing: border-box;
    width: 13px;
    height: 13px;
    flex: none;
    border: 1px solid transparent;
    border-radius: 2px;
  }
  .cell.l0 {
    border-color: var(--border);
  }
  .cell.clickable {
    cursor: pointer;
  }
  .cell.clickable:hover {
    outline: 1px solid var(--muted-foreground);
  }
  .cell.clickable:focus-visible {
    outline: 1px solid var(--foreground);
    outline-offset: 1px;
  }
</style>