CREATE TABLE IF NOT EXISTS embeddings (
    id           TEXT PRIMARY KEY,
    scope        TEXT NOT NULL,
    scope_id     TEXT NOT NULL,
    source_path  TEXT NOT NULL,
    source_kind  TEXT NOT NULL,
    source_id    TEXT,
    chunk_index  INTEGER NOT NULL,
    text         TEXT NOT NULL,
    hash         TEXT NOT NULL,
    dims         INTEGER NOT NULL,
    vec          BLOB NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_embeddings_scope ON embeddings(scope, scope_id);
CREATE INDEX IF NOT EXISTS idx_embeddings_source ON embeddings(source_kind, source_id);