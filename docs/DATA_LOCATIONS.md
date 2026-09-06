# Config & data locations (per-OS standards)

- **Config:** `~/.config/aiassistant/config.toml` (Linux), `%APPDATA%/aiassistant/` (Windows), `~/Library/Preferences/aiassistant/` (macOS).
- **Database:** `aiassistant.db` (SQLite, bundled) alongside the config.
- **Logs:** `~/.local/state/aiassistant/logs/` (Linux), `%LOCALAPPDATA%/aiassistant/logs/` (Windows), `~/Library/Logs/aiassistant/` (macOS).
- **Secrets:** OS keyring — API keys are stored in the system keychain, never in plaintext.