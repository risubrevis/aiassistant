-- Dedicated columns for thinking mode settings (previously stored in meta JSON).
ALTER TABLE chats ADD COLUMN thinking_enabled INTEGER NOT NULL DEFAULT 1;
ALTER TABLE chats ADD COLUMN thinking_effort TEXT NOT NULL DEFAULT 'medium';

-- Migrate existing values from meta JSON to dedicated columns.
UPDATE chats SET
  thinking_enabled = COALESCE(json_extract(meta, '$.thinking'), 1),
  thinking_effort = COALESCE(json_extract(meta, '$.thinking_effort'), 'medium')
WHERE meta IS NOT NULL;