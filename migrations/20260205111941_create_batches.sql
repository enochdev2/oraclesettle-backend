-- Add migration script here
CREATE TABLE batches (
    id TEXT PRIMARY KEY,
    merkle_root TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE batch_items (
    batch_id TEXT NOT NULL,
    market_id TEXT NOT NULL,

    PRIMARY KEY(batch_id, market_id),

    FOREIGN KEY(batch_id) REFERENCES batches(id),
    FOREIGN KEY(market_id) REFERENCES settlements(market_id)
);
