# Develop

```bash
npm install
npm run tauri dev
```

## Build (no bundling, smoke)

```bash
npm run tauri build -- --no-bundle
```

## Checks

```bash
npm run check                                              # svelte-check (TypeScript + Svelte)
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check  # Rust formatting
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings  # Rust lints
cargo test --manifest-path src-tauri/Cargo.toml --lib     # unit tests
```