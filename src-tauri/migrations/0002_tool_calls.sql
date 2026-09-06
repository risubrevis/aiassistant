-- Tool-call log (docs/04). One row per executed tool call (builtin / mcp / agent).
CREATE TABLE IF NOT EXISTS tool_calls (
    id            TEXT PRIMARY KEY,
    chat_id       TEXT NOT NULL,
    message_id    TEXT NOT NULL,        -- assistant message that initiated the call
    tool_call_id  TEXT,                  -- provider-assigned id (for tool-result linking)
    tool_name     TEXT NOT NULL,
    source        TEXT NOT NULL,         -- builtin | mcp:<server> | agent:<name>
    arguments     TEXT,                  -- JSON
    result        TEXT,                  -- JSON (tool output, possibly truncated)
    status        TEXT NOT NULL,         -- running | done | error | denied
    error         TEXT,
    started_at    INTEGER NOT NULL,
    finished_at   INTEGER
);

CREATE INDEX IF NOT EXISTS idx_tool_calls_message ON tool_calls(message_id);
CREATE INDEX IF NOT EXISTS idx_tool_calls_chat ON tool_calls(chat_id);