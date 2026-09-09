CREATE TABLE memory (
  id TEXT PRIMARY KEY,
  chat_id TEXT NOT NULL,
  project_id TEXT,
  content TEXT NOT NULL,
  category TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (chat_id) REFERENCES chats(id) ON DELETE CASCADE,
  FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX idx_memory_chat ON memory(chat_id);
CREATE INDEX idx_memory_project ON memory(project_id);