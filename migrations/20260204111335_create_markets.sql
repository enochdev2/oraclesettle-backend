-- Add migration script here
CREATE TABLE markets (
    id TEXT PRIMARY KEY,
    question TEXT NOT NULL,
    closes_at TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL
);
