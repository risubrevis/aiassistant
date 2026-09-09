-- OAuth metadata for HTTP MCP servers; tokens live in the OS keychain (secrets.rs).
CREATE TABLE IF NOT EXISTS mcp_oauth (
    server_id               TEXT PRIMARY KEY REFERENCES mcp_servers(id) ON DELETE CASCADE,
    auth_server_issuer      TEXT NOT NULL DEFAULT '',
    client_id               TEXT NOT NULL DEFAULT '',
    client_secret           TEXT NOT NULL DEFAULT '',
    token_endpoint          TEXT NOT NULL DEFAULT '',
    authorization_endpoint  TEXT NOT NULL DEFAULT '',
    registration_endpoint   TEXT NOT NULL DEFAULT '',
    revocation_endpoint     TEXT NOT NULL DEFAULT '',
    scopes                  TEXT NOT NULL DEFAULT '',
    redirect_uri            TEXT NOT NULL DEFAULT '',
    expires_at              INTEGER NOT NULL DEFAULT 0,
    has_refresh_token       INTEGER NOT NULL DEFAULT 0,
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL
);