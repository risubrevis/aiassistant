# AIAssistant

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg" alt="License: GPL-3.0-or-later">
  <img src="https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=black" alt="Tauri 2">
  <img src="https://img.shields.io/badge/Rust-stable-000000?logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white" alt="Svelte 5">
  <img src="https://img.shields.io/badge/TypeScript-5.6-3178C6?logo=typescript&logoColor=white" alt="TypeScript">
  <img src="https://img.shields.io/badge/TailwindCSS-4-06B6D4?logo=tailwindcss&logoColor=white" alt="TailwindCSS">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Platform-Linux%20%7C%20Windows%20%7C%20macOS-lightgrey" alt="Platform: Linux | Windows | macOS">
  <img src="https://img.shields.io/badge/SQLite-bundled-003B57?logo=sqlite&logoColor=white" alt="SQLite (bundled)">
  <img src="https://img.shields.io/badge/i18n-11%20languages-8A2BE2" alt="i18n: 11 languages">
  <img src="https://img.shields.io/badge/MCP-supported-1E90FF" alt="MCP supported">
  <a href="https://agnostic-ai-assistant.com/"><img src="https://img.shields.io/badge/Website-agnostic--ai--assistant.com-4A90D9?logo=googlechrome&logoColor=white" alt="Website"></a>
</p>

Cross-platform desktop app for chatting with LLMs and working with code projects. Provider-agnostic: you configure your own LLM endpoints (Ollama, OpenAI-compatible, Anthropic, OpenRouter, llama.cpp, vllm, …), system prompt, tools, MCP servers, and external code agents (cline / opencode / claude-code, etc.). No login or sign-up. No required online accounts. No payments or paywalls. No tracking. No brands. Just you and your favorite useful tools.

## Features

- **Chats & projects.** Standalone chats and Projects (sets of related chats with shared context and connected directories).
- **Provider-agnostic LLM.** OpenAI-compatible and Anthropic providers behind a unified `Provider` trait with streaming, tool use, thinking blocks, and vision (multipart images).
- **External code agents.** Bridge to cline, opencode, claude-code and others via PTY — with run monitoring, diff view, and approve/reject workflow.
- **MCP.** Model Context Protocol client with server management, tool discovery, and WebUI embedding.
- **RAG.** Per-project retrieval-augmented generation with embedding and vector store.
- **Built-in tools.** Configurable tool set alongside MCP-provided tools.
- **Prompt library.** Saved prompts with favorites, per-chat system prompts, and per-project rules.
- **Project task board.** Kanban and list views for project tasks; tasks can be launched as agent runs.
- **Activity stats.** Token usage, message counts, and timing metrics with a daily activity graph.
- **Command palette.** Quick navigation and actions (`Ctrl+P`).
- **In-app updates.** Manual update checks with signed downloads (tauri-plugin-updater).
- **11 languages.** English, Español, Deutsch, Français, Italiano, Português, Українська, Беларуская, Русский, Polski, Čeština.

## Stack

- **Shell:** Tauri 2.x (native WebView: WebKitGTK / WebView2 / WKWebView).
- **Backend:** Rust (tokio, sqlx/SQLite bundled, reqwest + rustls, toml_edit, notify, keyring, portable-pty, sysinfo, tracing).
- **Frontend:** SvelteKit 5 + Svelte 5 + TypeScript + Vite 6, Tailwind CSS v4, shadcn-svelte, paraglide-svelte (i18n), marked, lucide icons.
- **Tauri plugins:** opener, os, notification, window-state, clipboard-manager, updater, process, dialog (XDG Portal on Linux).

## Project layout

See [`docs/PROJECT_LAYOUT.md`](./docs/PROJECT_LAYOUT.md).

## Develop

See [`docs/DEVELOP.md`](./docs/DEVELOP.md).

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for guidelines.

## Config & data locations

See [`docs/DATA_LOCATIONS.md`](./docs/DATA_LOCATIONS.md).

## Support the project

If AIAssistant is useful to you, consider supporting its development — see [`SPONSORS.md`](./SPONSORS.md).

## License

GPL-3.0-or-later. See [`LICENSE`](./LICENSE).
