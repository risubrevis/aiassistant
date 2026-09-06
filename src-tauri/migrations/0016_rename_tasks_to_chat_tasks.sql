-- Rename the per-chat task list table to `chat_tasks` for clarity (the model
-- writes its todo plan here via the todo_write tool; agent-run rows live here
-- too with source='agent'). SQLite has no RENAME TABLE-with-FK rewrite; ALTER
-- TABLE … RENAME TO … preserves columns, constraints, and indexes.

ALTER TABLE tasks RENAME TO chat_tasks;