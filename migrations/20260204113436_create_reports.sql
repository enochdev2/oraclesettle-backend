CREATE TABLE IF NOT EXISTS reports (
  id UUID PRIMARY KEY,
  market_id UUID NOT NULL REFERENCES markets(id) ON DELETE CASCADE,
  source TEXT NOT NULL,
  value DOUBLE PRECISION NOT NULL,
  idempotency_key TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

  UNIQUE (market_id, idempotency_key)
);

CREATE INDEX IF NOT EXISTS idx_reports_market_created
  ON reports (market_id, created_at);