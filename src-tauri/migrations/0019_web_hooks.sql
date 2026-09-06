-- User-configurable HTTP endpoints the LLM can call via web_hook_run.
-- Secrets are NOT stored here — only a `has_secret` flag; the secret itself
-- lives in the OS keyring (secrets::webhook:<id>). URL/headers/body_template
-- may contain the literal `{{secret}}` placeholder, replaced at call time.
CREATE TABLE IF NOT EXISTS web_hooks (
    id               TEXT PRIMARY KEY,
    title            TEXT NOT NULL DEFAULT '',
    name             TEXT NOT NULL DEFAULT '',
    description      TEXT NOT NULL DEFAULT '',
    method           TEXT NOT NULL DEFAULT 'POST',
    url              TEXT NOT NULL DEFAULT '',
    headers          TEXT NOT NULL DEFAULT '{}',
    body_template    TEXT NOT NULL DEFAULT '',
    auth_type        TEXT NOT NULL DEFAULT 'none',
    auth_username    TEXT NOT NULL DEFAULT '',
    auth_header_name TEXT NOT NULL DEFAULT '',
    auth_param_name  TEXT NOT NULL DEFAULT '',
    has_secret       INTEGER NOT NULL DEFAULT 0,
    timeout_ms       INTEGER NOT NULL DEFAULT 30000,
    is_active        INTEGER NOT NULL DEFAULT 1,
    position         INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_web_hooks_name ON web_hooks(name);