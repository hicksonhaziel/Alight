CREATE TABLE curve_snapshots (
  snapshot_id TEXT PRIMARY KEY,
  source TEXT NOT NULL CHECK(source IN ('live','sim','replay')),
  regime_id TEXT NOT NULL,
  as_of_ms INTEGER NOT NULL,
  methodology_hash TEXT NOT NULL,
  payload_json TEXT NOT NULL
);
CREATE INDEX curve_history ON curve_snapshots(source,regime_id,as_of_ms);
