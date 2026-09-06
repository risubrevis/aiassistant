import { writable } from "svelte/store";

export type ToastKind = "info" | "success" | "error" | "warning";

export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  message?: string;
}

export const toasts = writable<Toast[]>([]);

const AUTO_DISMISS_MS = 4500;
let nextId = 1;

export function dismissToast(id: number) {
  toasts.update((list) => list.filter((t) => t.id !== id));
}

function push(kind: ToastKind, title: string, message?: string) {
  const id = nextId++;
  toasts.update((list) => [...list, { id, kind, title, message }]);
  setTimeout(() => dismissToast(id), AUTO_DISMISS_MS);
}

export const toast = {
  info: (title: string, message?: string) => push("info", title, message),
  success: (title: string, message?: string) => push("success", title, message),
  error: (title: string, message?: string) => push("error", title, message),
  warning: (title: string, message?: string) => push("warning", title, message),
};