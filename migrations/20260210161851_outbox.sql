-- Add migration script here
CREATE TABLE outbox (
  id UUID PRIMARY KEY,
  market_id UUID NOT NULL,
  payload JSONB NOT NULL,
  status TEXT NOT NULL, -- PENDING | SENT | FAILED
  retries INT DEFAULT 0,
  last_error TEXT,
  created_at TIMESTAMP DEFAULT NOW(),
  updated_at TIMESTAMP DEFAULT NOW()
);

-- CREATE TABLE IF NOT EXISTS outbox (
--   id TEXT PRIMARY KEY,
--   market_id TEXT NOT NULL,
--   payload TEXT NOT NULL,              -- JSON as TEXT in SQLite
--   status TEXT NOT NULL,               -- PENDING | SENT | FAILED
--   retries INTEGER NOT NULL DEFAULT 0,
--   last_error TEXT,
--   created_at TEXT NOT NULL,
--   updated_at TEXT NOT NULL
-- );

-- CREATE INDEX IF NOT EXISTS outbox_status_idx
-- ON outbox(status);
