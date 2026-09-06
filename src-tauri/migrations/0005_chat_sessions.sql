-- Context compaction: collapsed conversation segments (docs/04, docs/05).
-- Each row = one compaction event. boundary_message_id = first message of the
-- verbatim tail (an ancestor of the active leaf). Messages before the boundary
-- are replaced by `summary` when building LLM history; full history is preserved.
CREATE TABLE IF NOT EXISTS chat_sessions (
    id                   TEXT PRIMARY KEY,
    chat_id              TEXT NOT NULL,
    summary              TEXT NOT NULL,
    boundary_message_id  TEXT NOT NULL,
    token_count          INTEGER NOT NULL,
    model                TEXT,
    created_at           INTEGER NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_chat_sessions_chat ON chat_sessions(chat_id, created_at DESC);