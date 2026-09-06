# Project layout

```
AIAssistant/
├── src/                        # SvelteKit frontend
│   ├── routes/                 # +layout, +page (single-window app)
│   ├── lib/
│   │   ├── components/         # ChatView, Sidebar, Settings, AgentPanel, KanbanBoard, …
│   │   ├── stores/             # chat, project, config, agents, tasks, theme, …
│   │   └── {tauri,utils,hotkeys,i18n,os}.ts
│   ├── i18n/                   # paraglide: project.inlang + messages/{en,ru,es,…}.json (11 langs)
│   └── app.css                 # design tokens (shadcn), themes
├── src-tauri/                  # Rust backend (Tauri)
│   ├── src/
│   │   ├── providers/          # Provider trait + OpenAI / Anthropic implementations
│   │   ├── agents/             # external agent bridge (PTY, worktree, task extraction, presets)
│   │   ├── db/                 # SQLite layer (chats, projects, providers, models, prompts, MCP, …)
│   │   ├── mcp/                # MCP client, config, WebUI
│   │   ├── rag/                # embedding & vector store
│   │   ├── config/             # config.toml model, live watch, write
│   │   ├── tools/              # built-in tools
│   │   ├── {chat,projects,rules,secrets,net,updater,envinfo,approval,pty}.rs
│   │   └── {lib,main,logger}.rs
│   ├── capabilities/           # Tauri 2 permissions (default, settings window)
│   └── tauri.conf.json
├── packaging/                  # Arch PKGBUILD, .desktop template
├── .github/                    # CI (ubuntu/windows/macos), release workflow, funding.yml
├── components.json             # shadcn-svelte config
└── package.json
```