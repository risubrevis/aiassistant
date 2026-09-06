-- Reusable prompts: user-defined prompts that can be run from the Home view or
-- the Prompts view. Running a prompt creates a NEW chat (standalone when
-- project_id is NULL, or inside the linked project) and sends the prompt text.
-- project_id is SET NULL when the project is deleted (prompts survive).

CREATE TABLE IF NOT EXISTS prompts (
    id           TEXT PRIMARY KEY,
    title        TEXT NOT NULL,
    body         TEXT NOT NULL DEFAULT '',           -- markdown
    project_id   TEXT,                               -- NULL = standalone chat
    attach_files TEXT NOT NULL DEFAULT '[]',         -- JSON array of file paths
    skill_ids    TEXT NOT NULL DEFAULT '[]',         -- JSON array of global skill ids
    is_favorite  INTEGER NOT NULL DEFAULT 0,
    position     INTEGER NOT NULL DEFAULT 0,         -- global manual order
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_prompts_position ON prompts(position, created_at);
CREATE INDEX IF NOT EXISTS idx_prompts_favorite ON prompts(is_favorite, position);