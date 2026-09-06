-- Project Kanban board: task cards per project with a status column
-- (backlog | todo | in_progress | review | done), priority, manual ordering
-- within each column, and an optional link to the chat created when a task
-- is "run". project_task_changelog records status transitions.

CREATE TABLE IF NOT EXISTS project_tasks (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL,
    chat_id     TEXT,                          -- linked chat when task is "run"; NULL until then
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',      -- markdown
    status      TEXT NOT NULL DEFAULT 'backlog',   -- backlog | todo | in_progress | review | done
    priority    TEXT NOT NULL DEFAULT 'medium',    -- low | medium | high | urgent
    position    INTEGER NOT NULL DEFAULT 0,    -- order within (project_id, status) column
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_project_tasks_project ON project_tasks(project_id, status, position);
CREATE INDEX IF NOT EXISTS idx_project_tasks_chat ON project_tasks(chat_id);

CREATE TABLE IF NOT EXISTS project_task_changelog (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL,
    from_status TEXT,
    to_status   TEXT NOT NULL,
    changed_at  INTEGER NOT NULL,
    FOREIGN KEY (task_id) REFERENCES project_tasks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_project_task_changelog_task ON project_task_changelog(task_id, changed_at);