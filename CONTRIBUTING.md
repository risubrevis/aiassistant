# Contributing to AIAssistant

Thanks for your interest in contributing! AIAssistant is a cross-platform desktop app (Tauri 2 + Rust + SvelteKit 5) for chatting with LLMs and working with code projects. It is provider-agnostic and cross-platform.

This document covers how to set up a development environment, the code style we expect, and how to submit issues and pull requests.

## Table of contents

- [Prerequisites](#prerequisites)
- [Local setup](#local-setup)
- [Development workflow](#development-workflow)
- [Code style & conventions](#code-style--conventions)
- [Testing](#testing)
- [Reporting bugs](#reporting-bugs)
- [Suggesting features](#suggesting-features)
- [Pull requests](#pull-requests)
- [Project principles](#project-principles)
- [License](#license)

## Prerequisites

- **Rust** (stable), <https://rustup.rs>
- **Node.js** 20+ and **npm**
- **Git**

### Linux system dependencies

Tauri 2 on Linux requires native WebKit/GTK libraries. Install the packages matching your distribution:

```bash
# Debian / Ubuntu
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  build-essential curl wget file

# Arch Linux
sudo pacman -S --needed \
  webkit2gtk-4.1 gtk3 libayatana-appindicator3 librsvg libsoup3 \
  base-devel

# Fedora
sudo dnf install -y \
  webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel \
  librsvg2-devel libsoup3-devel javascriptcoregtk4.1-devel \
  gcc gcc-c++ curl wget file
```

Windows (WebView2, preinstalled on Win10/11) and macOS (WKWebView) need no extra system packages.

## Local setup

```bash
git clone https://github.com/risubrevis/aiassistant.git
cd aiassistant
npm install
npm run tauri dev
```

The app window opens once the Rust backend and Vite dev server are ready.

## Development workflow

```bash
npm run tauri dev                    # dev mode (hot reload for frontend)
npm run tauri build -- --no-bundle   # release binary, no packaging
npm run check                        # svelte-check (TypeScript + Svelte)
```

Rust backend lives in `src-tauri/`, the SvelteKit frontend in `src/`. See the [README](./README.md) for the full project layout.

## Code style & conventions

### Rust

- Edition 2021+, async on **tokio**.
- `snake_case` for all identifiers.
- Formatting: `cargo fmt`. CI checks `cargo fmt -- --check`.
- Linting: `cargo clippy -- -D warnings`. CI enforces this.
- Comments in English, and only where the code is not self-explanatory.
- Prefer existing dependencies; add a new crate only with justification.

```bash
cd src-tauri
cargo fmt
cargo clippy -- -D warnings
```

### TypeScript / Svelte

- **Strict mode** (see `tsconfig.json`), `camelCase` for identifiers.
- Tailwind CSS v4 utilities in markup; design tokens via `@theme` in `app.css`.
- shadcn-svelte components; do not introduce inline styles or separate CSS files without need.
- Run `npm run check` before submitting — it must pass without errors.

### Commits

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add provider timeout config
fix: handle empty API key gracefully
docs: update build instructions
refactor: extract provider trait
i18n: add Spanish translations
style: fix import ordering
```

Keep commit messages short and imperative. One logical change per commit.

### Comments & documentation

Code should be self-documenting. Add comments only for non-obvious logic that cannot be understood at a glance. Doc-comments are welcome on public types and functions.

## Testing

```bash
# Rust unit tests
cargo test --manifest-path src-tauri/Cargo.toml --lib

# Frontend type-check
npm run check
```

CI runs both of these on Ubuntu, Windows, and macOS. Make sure they pass locally before opening a PR.

## Reporting bugs

Open a [GitHub Issue](https://github.com/risubrevis/aiassistant/issues) and include:

1. **OS and version** (e.g. Arch Linux, Windows 11, macOS 15).
2. **AIAssistant version** (or commit hash if building from source).
3. **Steps to reproduce** — be specific.
4. **Expected vs. actual behavior.**
5. **Logs** — attach the relevant log file:
   - Linux: `~/.local/state/aiassistant/logs/aiassistant.log`
   - macOS: `~/Library/Logs/aiassistant/aiassistant.log`
   - Windows: `%LOCALAPPDATA%/aiassistant/logs/aiassistant.log`

> **Never** paste API keys or secrets into an issue. Redact them.

## Suggesting features

Before opening a feature-request issue, search existing issues to avoid duplicates. For larger proposals, open an issue first so the approach can be agreed on before implementation.

## Pull requests

1. **Fork** the repository and create a branch from `main`:
   ```bash
   git checkout -b feat/my-feature
   ```
2. **Keep changes focused** — one feature or fix per PR. Large refactors should be discussed in an issue first.
3. **Write clear commit messages** following Conventional Commits.
4. **Run all checks locally:**
   ```bash
   npm run check
   cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
   cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
   cargo test --manifest-path src-tauri/Cargo.toml --lib
   ```
5. **Open a PR** against `main`. Describe what changed and why, and link any related issues (`Closes #123`).
6. Be responsive to review feedback.

### PR checklist

- [ ] Code follows the style conventions above.
- [ ] `npm run check` passes.
- [ ] `cargo fmt --check` and `cargo clippy -D warnings` pass.
- [ ] `cargo test --lib` passes.
- [ ] No secrets, API keys, or provider-specific hardcodes introduced.
- [ ] Commit messages follow Conventional Commits.

## Project principles

These are the core rules every contribution must respect:

1. **Provider-agnostic.** No hardcoded dependencies on a specific LLM provider in business logic. Everything goes through the `Provider` abstraction.
2. **User controls everything.** System prompt, tools, MCP, agent — all user-configurable. Defaults must be minimal and overridable.
3. **Secrets stay safe.** API keys are stored in the OS keyring, never in plaintext configs or the database.
4. **Cross-platform from day one.** Avoid platform-specific solutions without a fallback. CI builds on Linux, Windows, and macOS.
5. **Minimal dependencies.** Prefer existing deps; justify any new one.
6. **Keep docs in sync.** If a feature is implemented or changed, update the relevant documentation in the same PR.

## License

By contributing, you agree that your contributions are licensed under **GPL-3.0-or-later**, the same license as the project. See [`LICENSE`](./LICENSE).