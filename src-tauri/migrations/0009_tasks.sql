-- Agent task manager: per-chat task list written by the model via the
-- todo_write tool (full replace per call; UI reads/writes via task_* commands).

CREATE TABLE IF NOT EXISTS tasks (
    id          TEXT PRIMARY KEY,
    chat_id     TEXT NOT NULL,
    position    INTEGER NOT NULL,
    content     TEXT NOT NULL,
    active_form TEXT,
    status      TEXT NOT NULL CHECK (status IN ('pending','in_progress','completed','cancelled')),
    message_id  TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tasks_chat ON tasks(chat_id, position);