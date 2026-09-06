-- Этап 3: projects, project_paths, chat_paths, rules, FTS5 (docs/04, docs/08).

CREATE TABLE IF NOT EXISTS projects (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    description        TEXT NOT NULL DEFAULT '',
    system_prompt      TEXT NOT NULL DEFAULT '',
    default_provider_id TEXT,
    default_model_id   TEXT,
    color              TEXT NOT NULL DEFAULT '',
    settings           TEXT,                -- JSON: cross_chat mode, max sibling chats, ...
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS project_paths (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    path          TEXT NOT NULL,
    kind          TEXT NOT NULL,            -- dir | file
    watch         INTEGER NOT NULL DEFAULT 0,
    exclude_globs TEXT,                     -- JSON array of strings
    created_at    INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_project_paths_project ON project_paths(project_id);

CREATE TABLE IF NOT EXISTS chat_paths (
    id            TEXT PRIMARY KEY,
    chat_id       TEXT NOT NULL,
    path          TEXT NOT NULL,
    kind          TEXT NOT NULL,            -- dir | file
    watch         INTEGER NOT NULL DEFAULT 0,
    exclude_globs TEXT,                     -- JSON array
    created_at    INTEGER NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_chat_paths_chat ON chat_paths(chat_id);

CREATE TABLE IF NOT EXISTS rules (
    id          TEXT PRIMARY KEY,
    scope       TEXT NOT NULL,              -- project | chat
    scope_id    TEXT NOT NULL,              -- projects.id or chats.id
    text        TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    added_by    TEXT,                       -- user | model
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_rules_scope ON rules(scope, scope_id, sort_order);

-- Branching support: index parent_id for active-branch walks.
CREATE INDEX IF NOT EXISTS idx_messages_parent ON messages(parent_id);

-- Full-text search over messages.content + chats.title (docs/04).
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    chat_id UNINDEXED,
    title UNINDEXED,
    content_rowid,
    tokenize = 'unicode61'
);

-- Triggers keep messages_fts in sync with messages.
CREATE TRIGGER IF NOT EXISTS messages_fts_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (content, chat_id, title, content_rowid)
    SELECT new.content, new.chat_id,
           (SELECT title FROM chats WHERE id = new.chat_id),
           new.rowid;
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_ad AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE content_rowid = old.rowid;
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_au AFTER UPDATE ON messages BEGIN
    DELETE FROM messages_fts WHERE content_rowid = old.rowid;
    INSERT INTO messages_fts (content, chat_id, title, content_rowid)
    SELECT new.content, new.chat_id,
           (SELECT title FROM chats WHERE id = new.chat_id),
           new.rowid;
END;