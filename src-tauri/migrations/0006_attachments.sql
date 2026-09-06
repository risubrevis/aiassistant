CREATE TABLE IF NOT EXISTS attachments (
    id           TEXT PRIMARY KEY,
    chat_id      TEXT NOT NULL,
    message_id   TEXT NULL,
    file_name    TEXT NOT NULL,
    mime_type    TEXT NOT NULL,
    file_size    INTEGER NOT NULL,
    storage_path TEXT NOT NULL,
    is_image     INTEGER NOT NULL DEFAULT 0,
    width        INTEGER NULL,
    height       INTEGER NULL,
    created_at   INTEGER NOT NULL,
    FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id);
CREATE INDEX IF NOT EXISTS idx_attachments_chat ON attachments(chat_id, created_at);