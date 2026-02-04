-- Add migration script here
CREATE TABLE reports (
    id TEXT PRIMARY KEY,
    market_id TEXT NOT NULL,
    source TEXT NOT NULL,
    value REAL NOT NULL,
    idempotency_key TEXT NOT NULL,
    created_at TEXT NOT NULL,

    UNIQUE(market_id, source),
    UNIQUE(idempotency_key),

    FOREIGN KEY(market_id) REFERENCES markets(id)
);
