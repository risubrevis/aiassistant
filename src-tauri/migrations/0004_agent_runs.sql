-- Этап 4: внешние код-агенты + параллельные суб-агенты (docs/04, docs/07, docs/16).

CREATE TABLE IF NOT EXISTS agent_runs (
    id                   TEXT PRIMARY KEY,    -- run_id
    chat_id              TEXT NOT NULL,
    parent_tool_call_id  TEXT,                -- tool_call run_batch / agent__run
    parent_id            TEXT,                -- batch parent run id ("" for standalone)
    agent_connection_id  TEXT NOT NULL,       -- id из config [[agents]] (не FK)
    subtask_index        INTEGER NOT NULL DEFAULT 0,
    subtask_prompt       TEXT NOT NULL,
    cwd                  TEXT NOT NULL,
    worktree_branch      TEXT,                -- ветка git worktree при Write-изоляции
    agent_session_id     TEXT,                -- resume-id, возвращённый агентом
    status               TEXT NOT NULL,        -- queued|running|done|error|cancelled|merged
    result_summary       TEXT,
    started_at           INTEGER,
    ended_at             INTEGER,
    created_at           INTEGER NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_agent_runs_chat ON agent_runs(chat_id);
CREATE INDEX IF NOT EXISTS idx_agent_runs_parent ON agent_runs(parent_id);

-- Resume sessions of external agents (docs/07).
CREATE TABLE IF NOT EXISTS agent_sessions (
    id                   TEXT PRIMARY KEY,
    chat_id              TEXT NOT NULL,
    agent_connection_id  TEXT NOT NULL,
    agent_session_id    TEXT NOT NULL,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);