-- Move global rules + skills from config.toml into the DB.

ALTER TABLE rules ADD COLUMN title TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS skills (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    body        TEXT NOT NULL DEFAULT '',
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);