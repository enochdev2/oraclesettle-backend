-- Add migration script here
CREATE TABLE settlements (
    id TEXT PRIMARY KEY,
    market_id TEXT NOT NULL UNIQUE,
    outcome REAL NOT NULL,
    decided_at TEXT NOT NULL,

    FOREIGN KEY(market_id) REFERENCES markets(id)
);
