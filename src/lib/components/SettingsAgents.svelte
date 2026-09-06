<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    agentList,
    agentCreate,
    agentUpdate,
    agentDelete,
    agentReorder,
    agentSetActive,
    agentTestInput,
    agentDetect,
    agentPresets,
    providersActiveModels,
    onAgentsChanged,
    onProvidersChanged,
    onConfigReloaded,
    type AgentInfo,
    type AgentInput,
    type AgentRow,
    type ModelOption,
  } from "$lib/tauri";
  import { m as msg } from "$lib/i18n";
  import { toast } from "$lib/stores/toasts";
  import {
    Plus,
    Trash2,
    Pencil,
    X,
    ArrowUp,
    ArrowDown,
    HelpCircle,
    LoaderCircle,
  } from "@lucide/svelte";
  import Select from "./Select.svelte";

  const promptModeItems = ["arg", "stdin", "stdin_json"].map((v) => ({ value: v, label: v }));
  const outputFormatItems = ["raw_text", "ndjson", "stream_json"].map((v) => ({ value: v, label: v }));
  const resumeItems = [
    { value: "none", label: msg.agents_resume_none() },
    { value: "--session {session}", label: msg.agents_resume_session() },
    { value: "--resume {session}", label: msg.agents_resume_generic() },
    { value: "--id {session}", label: msg.agents_resume_id() },
  ];

  const phHelp = [
    { token: "{prompt}", desc: msg.agents_ph_prompt() },
    { token: "{cwd}", desc: msg.agents_ph_cwd() },
    { token: "{model}", desc: msg.agents_ph_model() },
    { token: "{session}", desc: msg.agents_ph_session() },
  ];

  function blankInput(): AgentInput {
    return {
      name: "",
      description: "",
      default_model: "",
      capabilities: [],
      kind: "subprocess",
      command: "",
      args: [],
      prompt_mode: "arg",
      cwd: "",
      env: {},
      output_format: "raw_text",
      event_schema: null,
      mode_flags: {},
      resume_flag: "none",
      timeout_ms: 300000,
      max_turns: 50,
      is_active: true,
    };
  }

  function rowToInput(row: AgentRow): AgentInput {
    return {
      name: row.name,
      description: row.description,
      default_model: row.default_model,
      capabilities: [...row.capabilities],
      kind: row.kind,
      command: row.command,
      args: [...row.args],
      prompt_mode: row.prompt_mode,
      cwd: row.cwd,
      env: { ...row.env },
      output_format: row.output_format,
      event_schema: row.event_schema ? { ...row.event_schema } : null,
      mode_flags: { ...row.mode_flags },
      resume_flag: row.resume_flag,
      timeout_ms: row.timeout_ms,
      max_turns: row.max_turns,
      is_active: row.is_active,
    };
  }

  let list = $state<AgentInfo[]>([]);
  let error = $state<string | null>(null);

  let modalOpen = $state(false);
  let editingId = $state<string | null>(null);
  let form = $state<AgentInput>(blankInput());
  let argsText = $state("");
  let fieldErrors = $state<Record<string, string>>({});
  let modalError = $state<string | null>(null);
  let testResult = $state<string | null>(null);
  let detectNote = $state<string | null>(null);
  let argsHelpOpen = $state(false);

  let saving = $state(false);
  let testing = $state(false);
  let detecting = $state(false);
  let deleting = $state(false);
  let togglingId = $state<string | null>(null);
  let deleteId = $state<string | null>(null);

  let presets = $state<AgentInput[]>([]);
  let presetSel = $state("");
  let modelOptions = $state<ModelOption[]>([]);

  let unlistenAgents: (() => void) | undefined;
  let unlistenProviders: (() => void) | undefined;
  let unlistenConfig: (() => void) | undefined;

  onMount(() => {
    void fetchList();
    void loadModelOptions();
    void loadPresets();
    onAgentsChanged(() => void fetchList()).then((u) => (unlistenAgents = () => u()));
    onProvidersChanged(() => void loadModelOptions()).then((u) => (unlistenProviders = () => u()));
    onConfigReloaded(() => void loadModelOptions()).then((u) => (unlistenConfig = () => u()));
  });
  onDestroy(() => {
    unlistenAgents?.();
    unlistenProviders?.();
    unlistenConfig?.();
  });

  async function fetchList() {
    try {
      list = await agentList();
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  // Active providers' enabled models; the agent stores the model's API name
  // (not the UUID) because it is passed to the external CLI via {model}.
  async function loadModelOptions() {
    try {
      modelOptions = await providersActiveModels();
    } catch {
      modelOptions = [];
    }
  }

  async function loadPresets() {
    try {
      presets = await agentPresets();
    } catch {
      presets = [];
    }
  }

  function dotClass(info: AgentInfo): string {
    if (info.status === "ok") return "ok";
    if (info.status === "inactive") return "warn";
    return "err";
  }

  function statusText(info: AgentInfo): string {
    if (info.status === "ok") return msg.agents_status_ok();
    if (info.status === "inactive") return msg.agents_status_inactive();
    return info.status_detail || msg.agents_status_error();
  }

  async function toggleActive(info: AgentInfo, active: boolean) {
    togglingId = info.row.id;
    try {
      await agentSetActive(info.row.id, active);
      error = null;
    } catch (e) {
      error = String(e);
    }
    await fetchList();
    togglingId = null;
  }

  async function move(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= list.length) return;
    const next = [...list];
    [next[i], next[j]] = [next[j], next[i]];
    list = next;
    try {
      await agentReorder(next.map((a) => a.row.id));
      error = null;
    } catch (e) {
      error = String(e);
    }
    await fetchList();
  }

  function parseArgs(text: string): string[] {
    return text.split(",").map((s) => s.trim()).filter((s) => s.length > 0);
  }

  function resetModalState() {
    fieldErrors = {};
    modalError = null;
    testResult = null;
    detectNote = null;
    argsHelpOpen = false;
    presetSel = "";
  }

  function openAdd() {
    editingId = null;
    form = blankInput();
    argsText = "";
    resetModalState();
    modalOpen = true;
  }

  function openEdit(info: AgentInfo) {
    editingId = info.row.id;
    form = rowToInput(info.row);
    argsText = info.row.args.join(", ");
    resetModalState();
    modalOpen = true;
  }

  function closeModal() {
    modalOpen = false;
  }

  function applyPreset(value: string) {
    presetSel = "";
    if (value === "") {
      form = blankInput();
      argsText = "";
      fieldErrors = {};
      modalError = null;
      testResult = null;
      detectNote = null;
      return;
    }
    const preset = presets[Number(value)];
    if (!preset) return;
    form = {
      ...preset,
      capabilities: [...preset.capabilities],
      args: [...preset.args],
      env: { ...preset.env },
    };
    argsText = preset.args.join(", ");
    fieldErrors = {};
    modalError = null;
    testResult = null;
    detectNote = null;
  }

  function onModelPick(value: string) {
    const opt = modelOptions.find((o) => `${o.provider_id}::${o.model_id}` === value);
    form.default_model = opt?.model_name ?? "";
  }

  // The stored API name may no longer exist in the active list; then show it
  // as a raw disabled option so the user sees the current value.
  const modelSelectValue = $derived.by(() => {
    if (!form.default_model) return "";
    const found = modelOptions.find((o) => o.model_name === form.default_model);
    return found ? `${found.provider_id}::${found.model_id}` : `raw:${form.default_model}`;
  });

  const modelItems = $derived.by(() => {
    const items: { value: string; label: string; disabled?: boolean }[] = [
      { value: "", label: "— none —" },
      ...modelOptions.map((opt) => ({
        value: `${opt.provider_id}::${opt.model_id}`,
        label: `${opt.provider_name} / ${opt.display_name || opt.model_name}`,
      })),
    ];
    if (modelSelectValue.startsWith("raw:")) {
      items.push({ value: modelSelectValue, label: form.default_model, disabled: true });
    }
    return items;
  });

  const presetItems = $derived([
    { value: "", label: msg.agents_preset_none() },
    ...presets.map((p, i) => ({ value: String(i), label: p.name || `preset ${i + 1}` })),
  ]);

  async function detect() {
    if (!form.command.trim()) return;
    detecting = true;
    detectNote = null;
    try {
      const r = await agentDetect(form.command.trim());
      if (r) {
        form.command = r;
        detectNote = r;
      } else {
        detectNote = "not found in PATH";
      }
    } catch (e) {
      detectNote = String(e);
    } finally {
      detecting = false;
    }
  }

  async function save() {
    const errors: Record<string, string> = {};
    if (!form.name.trim()) errors.name = msg.agents_name_required();
    if (!form.command.trim()) errors.command = msg.agents_command_required();
    const args = parseArgs(argsText);
    if (args.some((a) => a.includes("{model}")) && !form.default_model.trim()) {
      errors.default_model = msg.agents_model_required();
    }
    if (Object.keys(errors).length > 0) {
      fieldErrors = errors;
      return;
    }
    fieldErrors = {};
    saving = true;
    modalError = null;
    try {
      const input: AgentInput = { ...form, args };
      if (editingId !== null) {
        await agentUpdate(editingId, input);
        toast.success(msg.agents_saved());
      } else {
        await agentCreate(input);
        toast.success(msg.agents_created());
      }
      modalOpen = false;
      await fetchList();
    } catch (e) {
      modalError = String(e);
      toast.error(msg.agents_failed(), String(e));
    } finally {
      saving = false;
    }
  }

  async function runTest() {
    if (!form.command.trim()) {
      fieldErrors = { ...fieldErrors, command: msg.agents_command_required() };
      return;
    }
    testing = true;
    testResult = null;
    try {
      const input: AgentInput = { ...form, args: parseArgs(argsText) };
      testResult = await agentTestInput(input);
      toast.success(msg.agents_test_ok());
    } catch (e) {
      testResult = "Error: " + String(e);
      toast.error(msg.agents_test_failed(), String(e));
    } finally {
      testing = false;
    }
  }

  async function confirmDelete() {
    if (deleteId === null) return;
    deleting = true;
    try {
      await agentDelete(deleteId);
      toast.success(msg.agents_deleted());
      deleteId = null;
      await fetchList();
    } catch (e) {
      error = String(e);
      toast.error(msg.agents_failed(), String(e));
    } finally {
      deleting = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (modalOpen && argsHelpOpen) {
        argsHelpOpen = false;
        return;
      }
      if (modalOpen) closeModal();
      if (deleteId !== null) deleteId = null;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="ag">
  <div class="row">
    <span class="hint">{msg.settings_tab_agents()}</span>
    <div class="btns">
      <button class="btn" onclick={openAdd}><Plus size={13} /> {msg.agents_add_new()}</button>
    </div>
  </div>

  <div class="list">
    {#each list as info, i (info.row.id)}
      <div class="srv">
        <span class="dot {dotClass(info)}" title={statusText(info)}></span>
        <div class="srv-main">
          <span class="name">{info.row.name}</span>
          <span class="sub">
            {info.row.command}{#if info.status === "error" && info.status_detail}
              · {info.status_detail}{/if}
          </span>
        </div>
        <input
          class="toggle"
          type="checkbox"
          title={msg.agents_active()}
          checked={info.row.is_active}
          disabled={togglingId === info.row.id}
          onchange={(e) => void toggleActive(info, e.currentTarget.checked)}
        />
        <button class="icon-btn" disabled={i === 0} title="Move up" onclick={() => void move(i, -1)}>
          <ArrowUp size={13} />
        </button>
        <button
          class="icon-btn"
          disabled={i === list.length - 1}
          title="Move down"
          onclick={() => void move(i, 1)}
        >
          <ArrowDown size={13} />
        </button>
        <button class="icon-btn" title={msg.common_edit()} onclick={() => openEdit(info)}>
          <Pencil size={13} />
        </button>
        <button class="icon-btn danger" title={msg.common_delete()} onclick={() => (deleteId = info.row.id)}>
          <Trash2 size={13} />
        </button>
      </div>
    {/each}
    {#if list.length === 0}
      <div class="empty">{msg.agents_empty()}</div>
    {/if}
  </div>
  {#if error}<div class="err">{error}</div>{/if}
</div>

{#if modalOpen}
  <div class="overlay" onkeydown={onKeydown} role="presentation">
    <div class="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
      <header class="head">
        <span class="head-title">{editingId !== null ? msg.agents_edit_title() : msg.agents_add_title()}</span>
        <button class="x" title={msg.common_close()} onclick={closeModal}><X size={16} /></button>
      </header>
      <div class="body">
        {#if presets.length > 0}
          <div class="field">
            <span class="lbl">{msg.agents_add_preset()}</span>
            <Select
              value={presetSel}
              items={presetItems}
              onchange={applyPreset}
              placeholder={msg.agents_presets()}
            />
          </div>
        {/if}

        <label class="field">
          <span class="lbl">{msg.agents_name()}</span>
          <input bind:value={form.name} spellcheck="false" />
          {#if fieldErrors.name}<span class="ferr">{fieldErrors.name}</span>{/if}
        </label>

        <label class="field">
          <span class="lbl">{msg.agents_description()}</span>
          <textarea class="desc" bind:value={form.description} spellcheck="false"></textarea>
          <span class="hint-line">{msg.agents_description_hint()}</span>
        </label>

        <div class="field">
          <span class="lbl">{msg.agents_default_model()}</span>
          <Select value={modelSelectValue} items={modelItems} onchange={onModelPick} />
          {#if fieldErrors.default_model}<span class="ferr">{fieldErrors.default_model}</span>{/if}
        </div>

        <div class="field">
          <span class="lbl">{msg.agents_command()}</span>
          <span class="cmd-row">
            <input class="mono" bind:value={form.command} spellcheck="false" placeholder="claude" />
            <button class="btn" onclick={() => void detect()} disabled={detecting}>
              {#if detecting}<LoaderCircle size={12} class="spin" /> {msg.agents_detecting()}{:else}{msg.agents_detect()}{/if}
            </button>
          </span>
          {#if detectNote}<span class="note">{detectNote}</span>{/if}
          {#if fieldErrors.command}<span class="ferr">{fieldErrors.command}</span>{/if}
        </div>

        <div class="field">
          <span class="lbl row-lbl">
            {msg.agents_args()}
            <button
              class="icon-btn help"
              class:active={argsHelpOpen}
              title={msg.agents_args_help()}
              onclick={() => (argsHelpOpen = !argsHelpOpen)}
            >
              <HelpCircle size={13} />
            </button>
          </span>
          <input bind:value={argsText} spellcheck="false" placeholder={"-m {model}, -y, -p {prompt}"} />
          {#if argsHelpOpen}
            <div class="ph-help">
              {#each phHelp as h (h.token)}
                <div class="ph-row"><code>{h.token}</code><span>{h.desc}</span></div>
              {/each}
            </div>
          {/if}
        </div>

        <div class="grid2">
          <div class="field">
            <span class="lbl">{msg.agents_prompt_mode()}</span>
            <Select
              value={form.prompt_mode}
              items={promptModeItems}
              onchange={(v) => (form.prompt_mode = v)}
            />
          </div>
          <div class="field">
            <span class="lbl">{msg.agents_output_format()}</span>
            <Select
              value={form.output_format}
              items={outputFormatItems}
              onchange={(v) => (form.output_format = v)}
            />
          </div>
        </div>

        <div class="grid2">
          <div class="field">
            <span class="lbl">{msg.agents_resume_flag()}</span>
            <Select
              value={form.resume_flag}
              items={resumeItems}
              onchange={(v) => (form.resume_flag = v)}
            />
          </div>
          <label class="field">
            <span class="lbl">{msg.agents_timeout()}</span>
            <input type="number" bind:value={form.timeout_ms} />
          </label>
        </div>

        <label class="field check">
          <span class="lbl">{msg.agents_active()}</span>
          <input class="toggle" type="checkbox" bind:checked={form.is_active} />
        </label>

        {#if testResult}
          <div class="test-out">
            <div class="test-label">{msg.agents_test_result()}</div>
            <pre>{testResult}</pre>
          </div>
        {/if}
        {#if modalError}<div class="res err">{modalError}</div>{/if}
      </div>
      <footer class="foot">
        <button
          class="btn"
          onclick={() => void runTest()}
          disabled={testing}
        >
          {#if testing}<LoaderCircle size={12} class="spin" /> {msg.agents_testing()}{:else}{msg.agents_test()}{/if}
        </button>
        <div class="spacer"></div>
        <button class="btn ghost" onclick={closeModal}>{msg.common_cancel()}</button>
        <button class="btn primary" onclick={() => void save()} disabled={saving}>
          {#if saving}<LoaderCircle size={12} class="spin" /> {msg.agents_saving()}{:else}{msg.common_save()}{/if}
        </button>
      </footer>
    </div>
  </div>
{/if}

{#if deleteId !== null}
  <div class="overlay" onkeydown={onKeydown} role="presentation">
    <div class="dialog small" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={onKeydown} role="dialog">
      <div class="body">
        <div class="confirm">{msg.agents_delete_confirm()}</div>
        <code class="confirm-id">{list.find((a) => a.row.id === deleteId)?.row.name ?? deleteId}</code>
      </div>
      <footer class="foot">
        <div class="spacer"></div>
        <button class="btn ghost" onclick={() => (deleteId = null)} disabled={deleting}>
          {msg.common_cancel()}
        </button>
        <button class="btn danger" onclick={() => void confirmDelete()} disabled={deleting}>
          {#if deleting}<LoaderCircle size={12} class="spin" /> {msg.agents_deleting()}{:else}{msg.common_delete()}{/if}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .ag { display: flex; flex-direction: column; gap: 0.75rem; }
  .row { display: flex; align-items: center; justify-content: space-between; }
  .hint { font-weight: 500; font-size: 0.8125rem; }
  .btns { display: flex; align-items: center; gap: 0.4rem; }
  .list { display: flex; flex-direction: column; gap: 0.25rem; }
  .srv { display: flex; align-items: center; gap: 0.4rem; padding: 0.4rem 0.5rem; border: 1px solid var(--border); border-radius: var(--radius-md); }
  .srv-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .name { font-size: 0.8125rem; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sub { font-size: 0.6875rem; color: var(--muted-foreground); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .dot { width: 8px; height: 8px; border-radius: 9999px; flex-shrink: 0; background: var(--muted-foreground); }
  .dot.ok { background: hsl(140 60% 40%); }
  .dot.warn { background: hsl(38 92% 50%); }
  .dot.err { background: var(--destructive); }
  .toggle {
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    accent-color: var(--primary);
    cursor: default;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 1.5rem;
    width: 1.5rem;
    flex-shrink: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted-foreground);
    cursor: default;
  }
  .icon-btn:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .icon-btn.active {
    background: var(--accent);
    color: var(--accent-foreground);
  }
  .icon-btn.danger:hover {
    color: var(--destructive);
  }
  .icon-btn:disabled {
    opacity: 0.4;
  }
  .empty { padding: 1rem; color: var(--muted-foreground); font-size: 0.8125rem; }
  .err { color: var(--destructive); font-size: 0.75rem; overflow-wrap: anywhere; }

  .overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: rgb(0 0 0 / 0.4);
  }
  .dialog {
    width: 620px;
    max-width: 90vw;
    max-height: 90vh;
    display: flex;
    flex-direction: column;
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .dialog.small { width: 360px; }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
  }
  .head-title { font-size: 0.875rem; font-weight: 500; }
  .x {
    display: inline-flex;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .x:hover { background: var(--accent); }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.875rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .foot { display: flex; align-items: center; gap: 0.5rem; padding: 0.625rem 0.875rem; border-top: 1px solid var(--border); }
  .spacer { flex: 1; }
  .field { display: flex; flex-direction: column; gap: 0.25rem; }
  .field.check { flex-direction: row; align-items: center; gap: 0.5rem; }
  .lbl { font-size: 0.72rem; color: var(--muted-foreground); }
  .row-lbl { display: flex; align-items: center; gap: 0.3rem; }
  .row-lbl .help { margin-left: auto; height: 1.25rem; width: 1.25rem; }
  .hint-line { font-size: 0.68rem; color: var(--muted-foreground); }
  input {
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
  }
  input:focus { outline: none; border-color: var(--ring); }
  input.mono { font-family: var(--font-mono); }
  textarea.desc {
    width: 100%;
    height: 72px;
    font-size: 0.8125rem;
    line-height: 1.4;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    resize: vertical;
  }
  textarea.desc:focus { outline: none; border-color: var(--ring); }
  .cmd-row { display: flex; gap: 0.3rem; }
  .cmd-row input { flex: 1; min-width: 0; }
  .note { font-size: 0.7rem; color: var(--muted-foreground); font-family: var(--font-mono); overflow-wrap: anywhere; }
  .ferr { color: var(--destructive); font-size: 0.7rem; }
  .ph-help {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.5rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--accent);
  }
  .ph-row { display: flex; align-items: baseline; gap: 0.5rem; font-size: 0.72rem; }
  .ph-row code {
    flex-shrink: 0;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    padding: 0.05rem 0.3rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
  }
  .ph-row span { color: var(--muted-foreground); }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 0.6rem; }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.3rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.75rem;
  }
  .btn:hover { background: var(--accent); }
  .btn:disabled { opacity: 0.5; }
  .btn.primary { background: var(--primary); color: var(--primary-foreground); border-color: var(--primary); }
  .btn.ghost { background: transparent; }
  .btn.danger { color: var(--destructive); border-color: var(--destructive); }
  .btn.danger:hover { background: var(--destructive); color: var(--destructive-foreground); }
  .btn :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  .test-out { display: flex; flex-direction: column; gap: 0.25rem; }
  .test-label { font-size: 0.7rem; color: var(--muted-foreground); }
  .test-out pre {
    margin: 0;
    padding: 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--muted);
    font-family: var(--font-mono);
    font-size: 0.7rem;
    white-space: pre-wrap;
    max-height: 180px;
    overflow-y: auto;
  }
  .res { font-size: 0.75rem; color: var(--muted-foreground); }
  .res.err { color: var(--destructive); overflow-wrap: anywhere; }
  .confirm { font-size: 0.8125rem; }
  .confirm-id { font-family: var(--font-mono); font-size: 0.75rem; color: var(--muted-foreground); }
</style>