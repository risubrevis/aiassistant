-- Worker agents moved from config.toml ([agent]) into the DB (multi-agent).
CREATE TABLE IF NOT EXISTS agents (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL DEFAULT '',
    description   TEXT NOT NULL DEFAULT '',
    default_model TEXT NOT NULL DEFAULT '',
    capabilities  TEXT NOT NULL DEFAULT '[]',
    kind          TEXT NOT NULL DEFAULT 'subprocess',
    command       TEXT NOT NULL DEFAULT '',
    args          TEXT NOT NULL DEFAULT '[]',
    prompt_mode   TEXT NOT NULL DEFAULT 'arg',
    cwd           TEXT NOT NULL DEFAULT '',
    env           TEXT NOT NULL DEFAULT '{}',
    output_format TEXT NOT NULL DEFAULT 'raw_text',
    event_schema  TEXT NOT NULL DEFAULT '',
    mode_flags    TEXT NOT NULL DEFAULT '{}',
    resume_flag   TEXT NOT NULL DEFAULT 'none',
    timeout_ms    INTEGER NOT NULL DEFAULT 300000,
    max_turns     INTEGER NOT NULL DEFAULT 50,
    is_active     INTEGER NOT NULL DEFAULT 1,
    position      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);