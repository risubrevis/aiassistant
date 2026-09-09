-- Per-prompt launch settings: model + mode/commands/edits + thinking, applied to
-- the chat created when the prompt is run. JSON; absent fields inherit defaults.
ALTER TABLE prompts ADD COLUMN launch_settings TEXT NOT NULL DEFAULT '{}';