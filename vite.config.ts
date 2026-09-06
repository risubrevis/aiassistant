import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { paraglideVitePlugin } from "@inlang/paraglide-js";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    sveltekit(),
    tailwindcss(),
    paraglideVitePlugin({
      project: "src/i18n/project.inlang",
      outdir: "src/i18n/paraglide",
      // Tauri serves the SPA over a custom protocol (tauri://localhost) where
      // document.cookie is unreliable across reloads/restarts. The default
      // strategy ["cookie", "globalVariable", "baseLocale"] therefore makes
      // getLocale() fall back to baseLocale after every reload, which turns
      // setLocale()'s default page reload into an infinite loop. localStorage
      // persists reliably in the Tauri webview, so it is the correct strategy
      // for this desktop app.
      strategy: ["localStorage", "baseLocale"],
    }),
  ],

  // Vite options tailored for Tauri development.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target:
      process.env.TAURI_ENV_PLATFORM == "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});