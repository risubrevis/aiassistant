-- Refactor rules: global-only table + project_rules + include_global_rules flag.
-- Drops chat-scoped rules (model-only, obsolete) and legacy auto-import snapshots.

CREATE TABLE project_rules (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL,
    title       TEXT NOT NULL DEFAULT '',
    text        TEXT NOT NULL DEFAULT '',
    is_active   INTEGER NOT NULL DEFAULT 1,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX idx_project_rules_project ON project_rules(project_id, position);

INSERT INTO project_rules (id, project_id, title, text, is_active, position, created_at, updated_at)
SELECT id, scope_id, title, text, enabled, sort_order, created_at, updated_at
FROM rules
WHERE scope = 'project' AND (added_by IS NULL OR added_by <> 'auto');

CREATE TABLE rules_new (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    text        TEXT NOT NULL DEFAULT '',
    is_active   INTEGER NOT NULL DEFAULT 1,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

INSERT INTO rules_new (id, title, text, is_active, position, created_at, updated_at)
SELECT id, title, text, enabled, sort_order, created_at, updated_at
FROM rules
WHERE scope = 'global' AND scope_id = '';

DROP TABLE rules;
ALTER TABLE rules_new RENAME TO rules;
CREATE INDEX idx_rules_position ON rules(position);

ALTER TABLE projects ADD COLUMN include_global_rules INTEGER NOT NULL DEFAULT 1;