-- Configurable search-engine providers for the `web_search` built-in tool.
-- `url` is a template containing the literal `{query}` placeholder.

CREATE TABLE IF NOT EXISTS web_search_tool_providers (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    url         TEXT NOT NULL DEFAULT '',
    enabled     INTEGER NOT NULL DEFAULT 1,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- Seed sensible defaults (no API key required, best-effort scraping).
INSERT OR IGNORE INTO web_search_tool_providers (id, title, url, enabled, position, created_at, updated_at) VALUES
('ddg-lite',   'DuckDuckGo Lite',      'https://lite.duckduckgo.com/lite/?q={query}',          1, 0, 0, 0),
('ddg-html',   'DuckDuckGo HTML',      'https://html.duckduckgo.com/html/?q={query}',          1, 1, 0, 0),
('searxng-be', 'SearxNG (searx.be)',   'https://searx.be/search?q={query}',                    1, 2, 0, 0),
('mojeek',     'Mojeek',               'https://www.mojeek.com/search?q={query}',              1, 3, 0, 0),
('brave',      'Brave Search',         'https://search.brave.com/search?q={query}',            0, 4, 0, 0);