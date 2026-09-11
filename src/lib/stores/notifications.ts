import { get, type Unsubscriber } from "svelte/store";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { config } from "./config";
import { onChatMessageDone, onAgentBatchComplete } from "$lib/tauri";
import { m } from "$lib/i18n";

export function initNotifications(): () => void {
  let enabled = get(config)?.general?.notifications_enabled ?? true;
  const unsubConfig: Unsubscriber = config.subscribe((c) => {
    enabled = c?.general?.notifications_enabled ?? true;
  });

  const unsubs: Array<() => void> = [];
  let granted: boolean | null = null;

  async function ensureGranted(): Promise<boolean> {
    if (granted === true) return true;
    try {
      let g = await isPermissionGranted();
      if (!g) {
        const perm = await requestPermission();
        g = perm === "granted";
      }
      granted = g;
      return g;
    } catch {
      return false;
    }
  }

  async function notify(title: string, body: string) {
    if (!enabled) return;
    try {
      // Skip while the user is watching; if focus can't be determined, notify anyway.
      try {
        if (await getCurrentWindow().isFocused()) return;
      } catch {
        // fall through
      }
      if (!(await ensureGranted())) return;
      sendNotification({ title, body });
    } catch (e) {
      console.error("notification failed", e);
    }
  }

  onChatMessageDone(() => {
    void notify(m.notification_response_title(), m.notification_response_body());
  }).then((u) => unsubs.push(u));
  onAgentBatchComplete(() => {
    void notify(m.notification_agent_title(), m.notification_agent_body());
  }).then((u) => unsubs.push(u));

  return () => {
    unsubConfig();
    for (const u of unsubs) u();
  };
}