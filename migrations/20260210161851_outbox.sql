CREATE TABLE IF NOT EXISTS outbox (
  id UUID PRIMARY KEY,
  market_id UUID NOT NULL REFERENCES markets(id) ON DELETE CASCADE,
  payload JSONB NOT NULL,
  status TEXT NOT NULL,
  retries INT NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_outbox_status_created
  ON outbox (status, created_at);