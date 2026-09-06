<script lang="ts">
  import { onMount } from "svelte";
  import { m } from "$lib/i18n";
  import { config } from "$lib/stores/config";
  import {
    configGet,
    networkHasPassword,
    networkTest,
    setNetwork,
    type Network,
    type NetworkInput,
    type NetworkTestResult,
  } from "$lib/tauri";
  import Select from "./Select.svelte";

  function defaults(): NetworkInput {
    return {
      proxy_enabled: false,
      proxy_type: "http",
      proxy_host: "",
      proxy_port: 0,
      proxy_username: "",
      password: "",
      no_proxy: "localhost,127.0.0.1,::1",
      test_url: "https://www.google.com",
      verify_tls: true,
      ca_cert_path: "",
      connect_timeout_ms: 10000,
    };
  }

  let form = $state<NetworkInput>(defaults());
  let savedNetwork = $state<Network>(defaults());
  let hasPassword = $state(false);
  let savedFlash = $state(false);
  let saving = $state(false);
  let testing = $state(false);
  let testResult = $state<NetworkTestResult | null>(null);
  let error = $state("");

  let savedTimer: ReturnType<typeof setTimeout> | undefined;

  const proxyTypes = [
    { value: "http", label: m.settings_network_proxy_type_http() },
    { value: "https", label: m.settings_network_proxy_type_https() },
    { value: "socks5", label: m.settings_network_proxy_type_socks5() },
    { value: "socks5h", label: m.settings_network_proxy_type_socks5h() },
  ];

  let dirty = $derived(
    form.proxy_enabled !== savedNetwork.proxy_enabled ||
      form.proxy_type !== savedNetwork.proxy_type ||
      form.proxy_host !== savedNetwork.proxy_host ||
      form.proxy_port !== savedNetwork.proxy_port ||
      form.proxy_username !== savedNetwork.proxy_username ||
      form.no_proxy !== savedNetwork.no_proxy ||
      form.test_url !== savedNetwork.test_url ||
      form.verify_tls !== savedNetwork.verify_tls ||
      form.ca_cert_path !== savedNetwork.ca_cert_path ||
      form.connect_timeout_ms !== savedNetwork.connect_timeout_ms ||
      form.password.length > 0,
  );

  let testDetail = $derived(
    testResult
      ? `${testResult.detail || "—"} · ${testResult.elapsed_ms} ms${
          testResult.status !== null ? ` · HTTP ${testResult.status}` : ""
        }`
      : "",
  );

  onMount(() => {
    void load();
    return () => clearTimeout(savedTimer);
  });

  async function load() {
    try {
      const cfg = $config ?? (await configGet());
      savedNetwork = { ...cfg.network };
      form = { ...cfg.network, password: "" };
      hasPassword = await networkHasPassword();
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function test() {
    testing = true;
    testResult = null;
    try {
      testResult = await networkTest(form);
      error = "";
    } catch (e) {
      error = String(e);
    } finally {
      testing = false;
    }
  }

  async function save() {
    saving = true;
    try {
      await setNetwork(form);
      savedNetwork = {
        proxy_enabled: form.proxy_enabled,
        proxy_type: form.proxy_type,
        proxy_host: form.proxy_host,
        proxy_port: form.proxy_port,
        proxy_username: form.proxy_username,
        no_proxy: form.no_proxy,
        test_url: form.test_url,
        verify_tls: form.verify_tls,
        ca_cert_path: form.ca_cert_path,
        connect_timeout_ms: form.connect_timeout_ms,
      };
      form.password = "";
      hasPassword = await networkHasPassword();
      error = "";
      savedFlash = true;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (savedFlash = false), 1500);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function cancel() {
    form = { ...savedNetwork, password: "" };
    testResult = null;
    error = "";
  }

  function clear() {
    form = defaults();
    testResult = null;
    error = "";
  }
</script>

<div class="network">
  <section class="section">
    <span class="sec-title">{m.settings_network_title()}</span>
    <div class="hint">{m.settings_network_hint()}</div>

    <label class="switch-row">
      <input class="toggle" type="checkbox" bind:checked={form.proxy_enabled} />
      <span class="field-label">{m.settings_network_proxy_enabled()}</span>
    </label>

    <div class="proxy" class:dim={!form.proxy_enabled}>
      <div class="grid2">
        <div class="field">
          <span class="lbl">{m.settings_network_proxy_type()}</span>
          <Select
            class="w-full"
            value={form.proxy_type}
            items={proxyTypes}
            onchange={(v) => (form.proxy_type = v)}
          />
        </div>
        <label class="field">
          <span class="lbl">{m.settings_network_proxy_host()}</span>
          <input bind:value={form.proxy_host} spellcheck="false" autocomplete="off" />
        </label>
      </div>
      <div class="grid2">
        <label class="field">
          <span class="lbl">{m.settings_network_proxy_port()}</span>
          <input
            type="number"
            min="0"
            max="65535"
            value={form.proxy_port}
            oninput={(e) => (form.proxy_port = Number(e.currentTarget.value))}
          />
        </label>
        <label class="field">
          <span class="lbl">{m.settings_network_proxy_username()}</span>
          <input bind:value={form.proxy_username} spellcheck="false" autocomplete="off" />
        </label>
      </div>
      <label class="field">
        <span class="lbl">
          {m.settings_network_proxy_password()}
          {#if hasPassword}
            <span class="pw-set">{m.settings_network_proxy_password_set()}</span>
          {/if}
        </span>
        <input
          type="password"
          bind:value={form.password}
          placeholder={m.settings_network_proxy_password_hint()}
          autocomplete="new-password"
        />
      </label>
      <label class="field">
        <span class="lbl">{m.settings_network_no_proxy()}</span>
        <textarea bind:value={form.no_proxy} rows="2" spellcheck="false"></textarea>
        <span class="hint">{m.settings_network_no_proxy_hint()}</span>
      </label>
    </div>

    <div class="grid2">
      <label class="field">
        <span class="lbl">{m.settings_network_test_url()}</span>
        <input bind:value={form.test_url} spellcheck="false" />
      </label>
      <label class="field">
        <span class="lbl">{m.settings_network_connect_timeout()}</span>
        <input
          type="number"
          min="0"
          value={form.connect_timeout_ms}
          oninput={(e) => (form.connect_timeout_ms = Number(e.currentTarget.value))}
        />
      </label>
    </div>

    <label class="switch-row">
      <input class="toggle" type="checkbox" bind:checked={form.verify_tls} />
      <span class="field-label">{m.settings_network_verify_tls()}</span>
    </label>
    <div class="hint">{m.settings_network_verify_tls_hint()}</div>

    <label class="field">
      <span class="lbl">{m.settings_network_ca_cert()}</span>
      <input bind:value={form.ca_cert_path} spellcheck="false" />
      <span class="hint">{m.settings_network_ca_cert_hint()}</span>
    </label>

    <div class="info">{m.settings_network_tor_hint()}</div>

    <div class="actions">
      <button class="btn" disabled={testing || saving} onclick={() => void test()}>
        {testing ? m.settings_network_testing() : m.settings_network_test()}
      </button>
      <button class="btn primary" disabled={!dirty || saving} onclick={() => void save()}>
        {m.settings_network_save()}
      </button>
      <button class="btn" disabled={!dirty || saving} onclick={cancel}>
        {m.settings_network_cancel()}
      </button>
      <button class="btn" onclick={clear}>{m.settings_network_clear()}</button>
    </div>

    <div class="status">
      {#if savedFlash}<span class="saved">{m.settings_network_saved()}</span>
      {:else if testing}<span class="hint">{m.settings_network_testing()}</span>
      {:else if testResult}
        <span class={testResult.ok ? "ok" : "err"}>
          {testResult.ok ? m.settings_network_test_ok() : m.settings_network_test_fail()}
        </span>
        <span class="hint">{testDetail}</span>
      {:else if error}<span class="err">{error}</span>{/if}
    </div>
  </section>
</div>

<style>
  .network {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }
  .sec-title {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .proxy {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }
  .proxy.dim {
    opacity: 0.55;
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.55rem;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .lbl {
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }
  .pw-set {
    margin-left: 0.4rem;
    font-size: 0.68rem;
    color: hsl(140 60% 40%);
  }
  .field-label {
    font-size: 0.8125rem;
  }
  .switch-row {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    cursor: default;
  }
  .hint {
    font-size: 0.7rem;
    color: var(--muted-foreground);
  }
  .info {
    font-size: 0.72rem;
    color: var(--muted-foreground);
    padding: 0.45rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--secondary);
  }
  input,
  textarea {
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.8125rem;
    font-family: var(--font-sans);
    outline: none;
  }
  input:focus,
  textarea:focus {
    border-color: var(--ring);
  }
  textarea {
    resize: vertical;
    font-family: var(--font-mono);
  }
  .toggle {
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    accent-color: var(--primary);
    cursor: default;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .status {
    min-height: 0.95rem;
    font-size: 0.7rem;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.5rem;
  }
  .saved,
  .ok {
    color: hsl(140 60% 40%);
  }
  .err {
    font-size: 0.72rem;
    color: var(--destructive);
    overflow-wrap: anywhere;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.28rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font-size: 0.78rem;
    cursor: default;
  }
  .btn:hover {
    background: var(--accent);
  }
  .btn:disabled,
  .btn:disabled:hover {
    opacity: 0.5;
    cursor: default;
    background: var(--background);
  }
  .btn.primary {
    background: var(--primary);
    border-color: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover {
    filter: brightness(1.08);
  }
  .btn.primary:disabled,
  .btn.primary:disabled:hover {
    opacity: 0.5;
    filter: none;
    background: var(--primary);
  }
</style>