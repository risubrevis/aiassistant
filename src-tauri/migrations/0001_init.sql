-- AIAssistant initial schema (MVP subset of docs/04-database.md).
-- Config-driven entities (providers/agents/global rules) live in config.toml,
-- not here. This schema covers runtime chats/messages + model cache.

CREATE TABLE IF NOT EXISTS chats (
    id           TEXT PRIMARY KEY,
    project_id   TEXT NULL,            -- NULL = standalone; FK added in projects phase
    title        TEXT NOT NULL,
    provider_id  TEXT NOT NULL,        -- refs config [[providers]].id (string, not FK)
    model_id     TEXT NOT NULL,        -- API model name
    system_prompt TEXT NULL,
    temperature  REAL NULL,
    pinned       INTEGER NOT NULL DEFAULT 0,
    archived     INTEGER NOT NULL DEFAULT 0,
    settings     TEXT NULL,            -- JSON
    meta         TEXT NULL,            -- JSON
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chats_updated ON chats(updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
    id            TEXT PRIMARY KEY,
    chat_id       TEXT NOT NULL,
    parent_id     TEXT NULL,           -- branching (later)
    role          TEXT NOT NULL,       -- user | assistant | system | tool
    content       TEXT NOT NULL,       -- plain text (for tool: summary)
    content_parts TEXT NULL,           -- JSON: ordered content blocks (thinking/text/tool_use/...)
    model         TEXT NULL,
    usage         TEXT NULL,           -- JSON {prompt_tokens, completion_tokens, total}
    thinking_ms   INTEGER NULL,
    finish_reason TEXT NULL,
    is_branch_root INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_chat ON messages(chat_id, created_at);

-- Cache of models freshly fetched from provider APIs (docs/04).
CREATE TABLE IF NOT EXISTS models_cache (
    id            TEXT PRIMARY KEY,    -- provider_id::model_name
    provider_id   TEXT NOT NULL,
    name          TEXT NOT NULL,
    context_window INTEGER NULL,
    meta          TEXT NULL,           -- JSON capabilities etc.
    fetched_at    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_models_provider ON models_cache(provider_id);