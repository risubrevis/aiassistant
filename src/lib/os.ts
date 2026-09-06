import { platform } from "@tauri-apps/plugin-os";

export type OsTag = "mac" | "win" | "linux";

/** Map tauri-plugin-os platform() to a UI tag for platform-adaptive tokens. */
export async function detectOs(): Promise<OsTag> {
  try {
    const p = await platform();
    if (p === "macos") return "mac";
    if (p === "windows") return "win";
    return "linux";
  } catch {
    // Fallback for non-Tauri (plain browser) contexts.
    const ua = navigator.userAgent;
    if (ua.includes("Mac")) return "mac";
    if (ua.includes("Win")) return "win";
    return "linux";
  }
}