<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { detectOs } from "$lib/os";
  import { osTag } from "$lib/stores/app";
  import { theme, applyTheme, watchSystemTheme, type Theme } from "$lib/stores/theme";
  import {
    loadLanguageSetting,
    applyLanguageSetting,
  } from "$lib/stores/lang";
  import { toast } from "$lib/stores/toasts";
  import { sendNotification } from "@tauri-apps/plugin-notification";
  import { m } from "$lib/i18n";
  import {
    configGet,
    onConfigReloaded,
    onChatStatus,
    onChatBlockStart,
    onChatBlockDelta,
    onChatBlockStop,
    onChatToolResult,
    onChatApprovalRequest,
    onChatPendingUpdate,
    onChatPtyStart,
    onChatPtyOutput,
    onChatPtyDone,
    onChatMessageDone,
    onChatUserMessage,
    onChatTurnError,
    onChatStreamRetry,
    onCompacted,
    onChatRenamed,
    onChatAskUser,
    onChatTasksUpdate,
    onProjectTaskChanged,
    onProjectFileChanged,
    onAgentProgress,
    onAgentStatus,
    onAgentBatchComplete,
    onLangChanged,
    type AgentStatusEvent,
  } from "$lib/tauri";
  import * as chat from "$lib/stores/chat";
  import * as project from "$lib/stores/project";
  import * as projectTasks from "$lib/stores/projectTasks";
  import * as tasks from "$lib/stores/tasks";
  import * as agentsStore from "$lib/stores/agents";
  import { initConfigStore, config } from "$lib/stores/config";
  import { initSkillsStore } from "$lib/stores/skills";
  import { initNotifications } from "$lib/stores/notifications";
  import { DEFAULT_HOTKEYS, parseHotkey, hotkeyMatches } from "$lib/hotkeys";
 import { handleHotkey } from "$lib/actions";
  import SettingsApp from "$lib/components/Settings.svelte";
  import ContextMenu, { type ContextMenuItem } from "$lib/components/ContextMenu.svelte";
  import { Undo2, Redo2, Scissors, Copy, ClipboardPaste } from "@lucide/svelte";
  import { readText as clipboardReadText } from "@tauri-apps/plugin-clipboard-manager";

  let { children } = $props();

  // --- Custom text-edit context menu for input/textarea/contenteditable ---
  // Replaces the native webview context menu (which is GTK-styled on Linux and
  // looks foreign on KDE/Qt). The custom menu uses the app's CSS theme so it
  // always matches the app's dark/light appearance.
  let textCtxMenu = $state<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);
  let textCtxTarget: HTMLElement | null = null;

  const TEXT_INPUT_TYPES = new Set(["text", "password", "email", "search", "url", "tel", "number", ""]);

  function isTextEditable(target: EventTarget | null): target is HTMLElement {
    if (!(target instanceof HTMLElement)) return false;
    if (target.isContentEditable) return true;
    if (target instanceof HTMLTextAreaElement) return true;
    if (target instanceof HTMLInputElement) return TEXT_INPUT_TYPES.has(target.type.toLowerCase());
    return false;
  }

  function hasSelection(el: HTMLElement): boolean {
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
      return el.selectionStart !== null && el.selectionEnd !== null && el.selectionStart !== el.selectionEnd;
    }
    const sel = window.getSelection();
    return !!sel && sel.toString().length > 0;
  }

  function pasteInto(el: HTMLElement) {
    clipboardReadText()
      .then((text) => {
        if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
          const start = el.selectionStart ?? el.value.length;
          const end = el.selectionEnd ?? el.value.length;
          el.setRangeText(text, start, end, "end");
          el.dispatchEvent(new Event("input", { bubbles: true }));
        } else if (el.isContentEditable) {
          document.execCommand("insertText", false, text);
        }
      })
      .catch(() => {
        // Clipboard read may fail if the clipboard is empty or unavailable.
      });
  }

  function buildTextMenuItems(el: HTMLElement): ContextMenuItem[] {
    const canCutCopy = hasSelection(el);
    return [
      {
        label: m.ctx_undo(),
        icon: Undo2,
        onclick: () => {
          el.focus();
          document.execCommand("undo");
        },
      },
      {
        label: m.ctx_redo(),
        icon: Redo2,
        onclick: () => {
          el.focus();
          document.execCommand("redo");
        },
      },
      { label: "", separator: true },
      {
        label: m.ctx_cut(),
        icon: Scissors,
        disabled: !canCutCopy,
        onclick: () => {
          el.focus();
          document.execCommand("cut");
        },
      },
      {
        label: m.ctx_copy(),
        icon: Copy,
        disabled: !canCutCopy,
        onclick: () => {
          el.focus();
          document.execCommand("copy");
        },
      },
      {
        label: m.ctx_paste(),
        icon: ClipboardPaste,
        onclick: () => {
          el.focus();
          pasteInto(el);
        },
      },
      { label: "", separator: true },
      {
        label: m.ctx_select_all(),
        onclick: () => {
          el.focus();
          if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
            el.select();
          } else {
            document.execCommand("selectAll");
          }
        },
      },
    ];
  }

  onMount(() => {
    const preloader = document.getElementById("app-preloader");
    if (!preloader) return;
    preloader.classList.add("fade-out");
    window.setTimeout(() => preloader.remove(), 220);
  });

  // Suppress the native webview context menu (Reload/Back/Forward) globally.
  // For text-editable elements (input/textarea/contenteditable) we show a
  // custom app-styled menu with Cut/Copy/Paste/Undo/Redo/Select All.
  // Elements with their own custom context menus (Sidebar chat/project rows)
  // call stopPropagation() in their oncontextmenu handler, so this listener
  // only fires for areas without an explicitly programmed context menu.
  onMount(() => {
    const onContextMenu = (e: MouseEvent) => {
      const target = e.target;
      if (isTextEditable(target)) {
        e.preventDefault();
        textCtxTarget = target;
        textCtxMenu = { x: e.clientX, y: e.clientY, items: buildTextMenuItems(target) };
        return;
      }
      e.preventDefault();
    };
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  });

  function detectSettingsWindow(): boolean {
    try {
      if (typeof window !== "undefined") {
        return getCurrentWebviewWindow().label === "settings";
      }
    } catch {
      // Not in a Tauri webview (e.g. plain browser) — render the main app.
    }
    return false;
  }

  const isSettingsWindow = detectSettingsWindow();

  function isTheme(v: string): v is Theme {
    return v === "light" || v === "dark" || v === "system";
  }

  async function applyConfig() {
    try {
      const cfg = await configGet();
      const t = isTheme(cfg.appearance.theme) ? cfg.appearance.theme : "system";
      theme.set(t);
      applyTheme(t);
    } catch {
      theme.set("system");
      applyTheme("system");
    }
  }

  // OS notification: long-running background agent runs (docs/19).
  // Wrapped in try/catch — the plugin may be unavailable or lack permission.
  function notifyOs(title: string, body: string) {
    try {
      sendNotification({ title, body });
    } catch {
      // OS notification unavailable; the in-app agent panel stays the source of truth
    }
  }

  function agentOsBody(e: AgentStatusEvent, title: string): string {
    return e.result_summary ? `${title}: ${e.result_summary}` : title;
  }

  onMount(async () => {
    // Language is stored client-side (localStorage); paraglide reloads the
    // document when the locale changes, so nothing after this runs on switch.
    if (await applyLanguageSetting(loadLanguageSetting())) return;

    const os = await detectOs();
    osTag.set(os);
    document.documentElement.setAttribute("data-os", os);

    await applyConfig();
    watchSystemTheme();

    onConfigReloaded(() => applyConfig());

    // Config store is needed by both windows (Settings reads providers/models).
    initConfigStore();

    // Skills live in the DB now; load for both windows (main-window Composer).
    initSkillsStore();

    // The Settings window only needs theme + config; skip chat/agent wiring.
    if (isSettingsWindow) return;

    // Desktop notifications for completed chat turns and agent batches.
    initNotifications();

    // Chat streaming event wiring (docs/13, docs/18).
    onChatStatus((e) => {
      chat.applyStatus(e);
      if (e.status === "error") {
        toast.error(m.toasts_chat_error(), e.detail ?? undefined);
      }
    });
    onChatBlockStart((e) => chat.applyBlockStart(e));
    onChatBlockDelta((e) => chat.applyBlockDelta(e));
    onChatBlockStop((e) => chat.applyBlockStop(e));
    onChatToolResult((e) => chat.applyToolResult(e));
    onChatApprovalRequest((e) => {
      chat.applyApprovalRequest(e);
      toast.info(m.toasts_approval_needed(), e.tool_name);
    });
    onChatPendingUpdate((e) => chat.applyPendingUpdate(e));
    onChatPtyStart((e) => chat.applyPtyStart(e));
    onChatPtyOutput((e) => chat.applyPtyOutput(e));
    onChatPtyDone((e) => chat.applyPtyDone(e));
    onChatMessageDone((e) => chat.applyMessageDone(e));
    onChatUserMessage((e) => chat.applyUserMessageId(e.chat_id, e.message_id));
    onChatTurnError((e) => chat.applyTurnError(e));
    onChatStreamRetry((e) => chat.applyStreamRetry(e));
    onCompacted((s) => chat.applyCompacted(s));
    onChatRenamed((e) => chat.applyChatRenamed(e));
    onChatAskUser((e) => project.applyAskUser(e));
    onChatTasksUpdate((e) => tasks.applyTasksUpdate(e.chat_id, e.tasks));
    onProjectTaskChanged((e) => projectTasks.loadProjectTasks(e.project_id));
    onProjectFileChanged((e) => project.applyFileChanged(e));

    onAgentProgress((e) => agentsStore.applyAgentProgress(e));
    onAgentStatus((e) => {
      agentsStore.applyAgentStatus(e);
      if (e.status === "error" || e.status === "timeout") {
        toast.error(m.toasts_agent_failed(), e.result_summary ?? undefined);
      }
      if (e.status === "done" || e.status === "error") {
        notifyOs(m.app_name(), agentOsBody(e, e.status === "done" ? m.toasts_agent_finished() : m.toasts_agent_failed()));
      }
    });

    onAgentBatchComplete((e) => {
      agentsStore.applyAgentBatchComplete(e);
      const okTxt = e.any_error ? m.toasts_agent_batch_partial() : m.toasts_agent_batch_done();
      if (e.any_error) {
        toast.warning(m.toasts_agent_batch_partial(), `${e.run_ids.length} runs`);
      } else {
        toast.success(m.toasts_agent_batch_done(), `${e.run_ids.length} runs`);
      }
      notifyOs(m.app_name(), okTxt);
      const status = get(chat.statusByChat)[e.chat_id];
      if (status !== "running") {
        void chat.sendBackgroundSynthesis(e.chat_id, e.parent_id, e.any_error);
      }
    });

    // Language change in the Settings window should reload the main window too.
    onLangChanged(() => location.reload());

    chat.loadChats();
    project.loadProjects();
    agentsStore.loadAgents();
    agentsStore.loadAgentRuns();
  });

  onMount(() => {
    let off: (() => void) | undefined;
    void (async () => {
      if (isSettingsWindow) return;
      const isMac = (await detectOs()) === "mac";
      const onKeydown = (e: KeyboardEvent) => {
        const target = e.target as HTMLElement | null;
        const isInput =
          !!target &&
          (target.tagName === "INPUT" ||
            target.tagName === "TEXTAREA" ||
            target.isContentEditable);
        if (isInput && !e.ctrlKey && !e.metaKey && !e.altKey) return;
        const hotkeys = { ...DEFAULT_HOTKEYS, ...(get(config)?.hotkeys ?? {}) };
        for (const [name, combo] of Object.entries(hotkeys)) {
          const hk = parseHotkey(combo, isMac);
          if (!hk || !hotkeyMatches(e, hk)) continue;
          if (handleHotkey(name)) {
            e.preventDefault();
            e.stopPropagation();
          }
          return;
        }
      };
      window.addEventListener("keydown", onKeydown);
      off = () => window.removeEventListener("keydown", onKeydown);
    })();
    return () => off?.();
  });
</script>

{#if isSettingsWindow}
  <SettingsApp />
{:else}
  {@render children()}
{/if}

{#if textCtxMenu}
  <ContextMenu
    x={textCtxMenu.x}
    y={textCtxMenu.y}
    items={textCtxMenu.items}
    onclose={() => {
      textCtxMenu = null;
      textCtxTarget = null;
    }}
  />
{/if}