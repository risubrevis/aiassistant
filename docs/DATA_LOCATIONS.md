# Config & data locations (per-OS standards)

- **Config:** `~/.config/aiassistant/config.toml` (Linux), `%APPDATA%/aiassistant/` (Windows), `~/Library/Preferences/aiassistant/` (macOS).
- **Database:** `app.db` (SQLite, bundled) in the OS data dir — `~/.local/share/aiassistant/` (Linux), `%APPDATA%/.../aiassistant/` (Windows), `~/Library/Application Support/aiassistant/` (macOS). Same dir holds `attachments/` and `exports/`.
- **Logs:** `~/.local/state/aiassistant/logs/` (Linux), `%LOCALAPPDATA%/aiassistant/logs/` (Windows), `~/Library/Logs/aiassistant/` (macOS).
- **Secrets:** OS keyring — API keys are stored in the system keychain, never in plaintext.