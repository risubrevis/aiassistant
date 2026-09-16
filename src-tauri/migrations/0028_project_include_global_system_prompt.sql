-- Per-project system prompt: toggle to include the global system prompt
-- (Settings -> Prompts: system prompt + environment info) alongside the
-- project-specific prompt. Default 1 = include global (preserves prior
-- behaviour where the global prompt was the base).
ALTER TABLE projects ADD COLUMN include_global_system_prompt INTEGER NOT NULL DEFAULT 1;