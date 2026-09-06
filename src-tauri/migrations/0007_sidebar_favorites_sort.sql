-- Sidebar: favorites (pinned projects), manual sort order, recursive-delete safety.

ALTER TABLE projects ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE projects ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

ALTER TABLE chats ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;