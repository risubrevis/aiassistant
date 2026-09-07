-- API-backed search providers for the `web_search` built-in tool.
-- `kind` distinguishes legacy scrape providers ('{query}' URL template) from
-- API providers (request method/auth/body templates). API keys live in the OS
-- keyring; `has_key` only tracks presence in the DB.

ALTER TABLE web_search_tool_providers ADD COLUMN kind TEXT NOT NULL DEFAULT 'scrape';
ALTER TABLE web_search_tool_providers ADD COLUMN api_method TEXT NOT NULL DEFAULT '';
ALTER TABLE web_search_tool_providers ADD COLUMN auth_scheme TEXT NOT NULL DEFAULT 'none';
ALTER TABLE web_search_tool_providers ADD COLUMN auth_header TEXT NOT NULL DEFAULT '';
ALTER TABLE web_search_tool_providers ADD COLUMN body_template TEXT NOT NULL DEFAULT '';
ALTER TABLE web_search_tool_providers ADD COLUMN results_path TEXT NOT NULL DEFAULT '';
ALTER TABLE web_search_tool_providers ADD COLUMN title_field TEXT NOT NULL DEFAULT '';
ALTER TABLE web_search_tool_providers ADD COLUMN url_field TEXT NOT NULL DEFAULT '';
ALTER TABLE web_search_tool_providers ADD COLUMN snippet_field TEXT NOT NULL DEFAULT '';
ALTER TABLE web_search_tool_providers ADD COLUMN has_key INTEGER NOT NULL DEFAULT 0;