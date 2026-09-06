-- Agent task manager: distinguish internal LLM plan tasks from external-agent
-- tasks and scope agent rows to their run.

ALTER TABLE tasks ADD COLUMN source TEXT NOT NULL DEFAULT 'internal';
ALTER TABLE tasks ADD COLUMN run_id TEXT;
CREATE INDEX IF NOT EXISTS idx_tasks_run ON tasks(run_id) WHERE run_id IS NOT NULL;