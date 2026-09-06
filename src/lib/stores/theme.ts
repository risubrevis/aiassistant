import { writable } from "svelte/store";

export type Theme = "light" | "dark" | "system";

export const theme = writable<Theme>("system");

const darkQuery = () =>
  typeof window !== "undefined"
    ? window.matchMedia("(prefers-color-scheme: dark)")
    : null;

export function resolvedIsDark(t: Theme): boolean {
  if (t === "dark") return true;
  if (t === "light") return false;
  return darkQuery()?.matches ?? false;
}

export function applyTheme(t: Theme) {
  const root = document.documentElement;
  root.classList.toggle("dark", resolvedIsDark(t));
}

/** Keep the `.dark` class in sync with the OS theme when theme == "system". */
export function watchSystemTheme() {
  const q = darkQuery();
  if (!q) return;
  const handler = () => {
    theme.update((t) => {
      if (t === "system") applyTheme("system");
      return t;
    });
  };
  q.addEventListener("change", handler);
  return () => q.removeEventListener("change", handler);
}