-- Bump default timeout for Ollama providers from 30s to 120s.
-- Local models with large contexts need more time for prompt processing
-- (time-to-first-token) than cloud APIs. Only updates providers still
-- at the old 30000ms default; custom values are preserved.
UPDATE providers SET timeout_ms = 120000 WHERE kind = 'ollama' AND timeout_ms = 30000;