-- MCP server definitions moved from config file (mcp.json) into the DB.
CREATE TABLE IF NOT EXISTS mcp_servers (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    name        TEXT NOT NULL DEFAULT '',
    json_body   TEXT NOT NULL DEFAULT '{}',
    is_active   INTEGER NOT NULL DEFAULT 1,
    webui_url   TEXT NOT NULL DEFAULT '',
    webui_icon  TEXT NOT NULL DEFAULT '',
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_name ON mcp_servers(name);