<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { Check, Copy, Globe, GitFork } from "@lucide/svelte";
  import { m } from "$lib/i18n";

  const FALLBACK_VERSION = "1.0.0";
  const WEBSITE_URL = "https://agnostic-ai-assistant.com/";
  const REPO_URL = "https://github.com/risubrevis/aiassistant";

  type PaymentMethod = { id: string; name: string; address: string; url?: string };

  const FIAT_METHODS: PaymentMethod[] = [
    { id: "donatello", name: "Donatello", address: "https://donatello.to/risubrevis", url: "https://donatello.to/risubrevis" },
    { id: "paypal", name: "PayPal", address: "https://paypal.me/risubrevis", url: "https://paypal.me/risubrevis" },
  ];
  const CRYPTO_METHODS: PaymentMethod[] = [
    { id: "ton", name: "TON (GRAM)", address: "UQARfS0xURhiGBLZ6_EYdSBJSMynLqAb_ktcoPHeCWNZ9RYs" },
    { id: "btc", name: "Bitcoin (BTC)", address: "bc1qcptl4v8ccywn5azf8ah9lcwc7af0df765je5yh" },
    { id: "eth", name: "Ethereum (ETH)", address: "0x2CDe86E15E955d8490EBC6b18e8c2567a4E412Ab" },
    { id: "xmr", name: "Monero (XMR)", address: "88uPgR4mV8DekYWDAXsKpxgCn7LWkVJQtjko1dKzFX2G8rZGg9qqFLdcdDC14aoKiRUv8HDgHN1mp8eAvV95dPmb1EgHg2m" },
    { id: "usdt-trc20", name: "USDT (Tron, TRC-20)", address: "TGWVREi6ogYDDcykoRhURK9QSAah2GYLh3" },
    { id: "usdt-solana", name: "USDT (Solana, SPL)", address: "CU3BCQmH7TTXNzZzAdhMXzGQwyD2Gnjw4" },
  ];

  let currentVersion = $state(FALLBACK_VERSION);
  let copiedId = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;

  async function copyToClipboard(id: string, text: string) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      copiedId = id;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedId = null;
      }, 1500);
    } catch (e) {
      console.error("clipboard write failed", e);
    }
  }

  // Wrapped so the import is referenced in the module body (SSR build drops
  // inline onclick handlers, which would otherwise flag `openUrl` as unused).
  function openExternal(url: string) {
    void openUrl(url);
  }

  onMount(async () => {
    try {
      currentVersion = await getVersion();
    } catch (e) {
      console.error("getVersion failed", e);
      currentVersion = FALLBACK_VERSION;
    }
  });
</script>

{#snippet copyBtn(id: string, text: string)}
  <button
    class="icon-btn"
    type="button"
    title={copiedId === id ? m.settings_about_copied() : m.settings_about_copy()}
    aria-label={copiedId === id ? m.settings_about_copied() : m.settings_about_copy()}
    onclick={() => void copyToClipboard(id, text)}
  >
    {#if copiedId === id}<Check size={13} />{:else}<Copy size={13} />{/if}
  </button>
{/snippet}

<div class="about">
  <div class="name">{m.app_name()}</div>
  <div class="desc">{m.settings_about_description()}</div>

  <div class="rows">
    <div class="row">
      <span class="label">{m.settings_about_version()}</span>
      <span class="value">{currentVersion}</span>
    </div>
  </div>

  <div class="section-heading">{m.settings_about_public_sources()}</div>
  <div class="rows">
    <div class="row copyable">
      <span class="label"><Globe size={13} /> {m.settings_about_public_website()}</span>
      <a
        class="value link mono"
        href={WEBSITE_URL}
        title={WEBSITE_URL}
        onclick={(e) => {
          e.preventDefault();
          openExternal(WEBSITE_URL);
        }}
      >{WEBSITE_URL}</a>
      {@render copyBtn("website", WEBSITE_URL)}
    </div>
    <div class="row copyable">
      <span class="label"><GitFork size={13} /> {m.settings_about_public_repository()}</span>
      <a
        class="value link mono"
        href={REPO_URL}
        title={REPO_URL}
        onclick={(e) => {
          e.preventDefault();
          openExternal(REPO_URL);
        }}
      >{REPO_URL}</a>
      {@render copyBtn("repo", REPO_URL)}
    </div>
  </div>

  <div class="section-heading">{m.settings_about_sponsors()}</div>
  <div class="sponsors-intro">
    <p>{m.settings_about_sponsors_intro()}</p>
    <p>{m.settings_about_sponsors_usage()}</p>
  </div>
  <div class="sponsors-subheading">{m.settings_about_sponsors_fiat()}</div>
  <div class="rows">
    {#each FIAT_METHODS as method (method.id)}
      <div class="row copyable">
        <span class="label">{method.name}</span>
        <a
          class="value link mono"
          href={method.url}
          title={method.url}
          onclick={(e) => {
            e.preventDefault();
            if (method.url) openExternal(method.url);
          }}
        >{method.address}</a>
        {@render copyBtn(method.id, method.address)}
      </div>
    {/each}
  </div>
  <div class="sponsors-subheading">{m.settings_about_sponsors_crypto()}</div>
  <div class="rows">
    {#each CRYPTO_METHODS as method (method.id)}
      <div class="row copyable">
        <span class="label">{method.name}</span>
        <span class="value mono" title={method.address}>{method.address}</span>
        {@render copyBtn(method.id, method.address)}
      </div>
    {/each}
  </div>
</div>

<style>
  .about {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .name {
    font-size: 0.9375rem;
    font-weight: 600;
  }
  .desc {
    font-size: 0.8125rem;
    color: var(--muted-foreground);
  }
  .rows {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    padding: 0.4rem 0.625rem;
    font-size: 0.75rem;
  }
  .row + .row {
    border-top: 1px solid var(--border);
  }
  .label {
    width: 130px;
    flex-shrink: 0;
    color: var(--muted-foreground);
  }
  .value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 0.6875rem;
  }

  .section-heading {
    font-size: 0.8125rem;
    font-weight: 600;
    margin-top: 0.25rem;
  }
  .sponsors-intro {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .sponsors-intro p {
    margin: 0;
    font-size: 0.75rem;
    line-height: 1.5;
    color: var(--muted-foreground);
  }
  .sponsors-subheading {
    font-size: 0.6875rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted-foreground);
    margin-top: 0.25rem;
  }
  .row.copyable {
    align-items: center;
  }
  .row.copyable .value {
    flex: 1;
  }
  .row.copyable .label {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }
  .link {
    color: hsl(210 90% 50%);
    text-decoration: none;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--muted-foreground);
    cursor: default;
  }
  .icon-btn:hover {
    background: var(--accent);
    color: var(--foreground);
  }
</style>