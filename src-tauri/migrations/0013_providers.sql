-- Multi-provider support: providers + per-provider models in the DB.
-- Chats now reference providers/models by UUID (nullable when deleted).

CREATE TABLE IF NOT EXISTS providers (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL DEFAULT '',
    kind          TEXT NOT NULL DEFAULT 'openai',
    base_url      TEXT NOT NULL DEFAULT '',
    api_key_ref   TEXT NOT NULL DEFAULT '',
    extra_headers TEXT NOT NULL DEFAULT '{}',
    timeout_ms    INTEGER NOT NULL DEFAULT 30000,
    is_active     INTEGER NOT NULL DEFAULT 1,
    position      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_models (
    id             TEXT PRIMARY KEY,
    provider_id    TEXT NOT NULL,
    name           TEXT NOT NULL,
    display_name   TEXT NOT NULL DEFAULT '',
    enabled        INTEGER NOT NULL DEFAULT 1,
    alias          TEXT NOT NULL DEFAULT '',
    capabilities   TEXT NOT NULL DEFAULT '[]',
    context_window INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    FOREIGN KEY (provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_provider_models_provider ON provider_models(provider_id);

-- chats.provider_id / model_id: were NOT NULL config-string refs; now nullable UUID refs.
-- SQLite has no ALTER COLUMN to drop NOT NULL; use DROP+ADD COLUMN (bundled SQLite >= 3.35).
ALTER TABLE chats DROP COLUMN provider_id;
ALTER TABLE chats ADD COLUMN provider_id TEXT;
ALTER TABLE chats DROP COLUMN model_id;
ALTER TABLE chats ADD COLUMN model_id TEXT;