-- Mark a prompt as a template: template prompts create a new chat with launch
-- settings applied but do NOT send automatically — the prompt text is placed in
-- the composer for the user to edit before sending manually.
ALTER TABLE prompts ADD COLUMN is_template INTEGER NOT NULL DEFAULT 0;