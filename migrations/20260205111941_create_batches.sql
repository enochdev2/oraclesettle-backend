CREATE TABLE IF NOT EXISTS batches (
  id UUID PRIMARY KEY,
  merkle_root TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS batch_items (
  batch_id UUID NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
  market_id UUID NOT NULL REFERENCES markets(id) ON DELETE CASCADE,
  PRIMARY KEY (batch_id, market_id)
);