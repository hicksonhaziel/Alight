CREATE TABLE diagnostic_windows (
  id TEXT PRIMARY KEY, source TEXT NOT NULL, origin TEXT NOT NULL,
  through_utc TEXT NOT NULL, payload_json TEXT NOT NULL
);
CREATE INDEX diagnostic_source_time ON diagnostic_windows(source,origin,through_utc);
CREATE TABLE regime_changes (
  id TEXT PRIMARY KEY, source TEXT NOT NULL, detected_at_utc TEXT NOT NULL,
  regime_id TEXT NOT NULL, payload_json TEXT NOT NULL,
  UNIQUE(source,regime_id)
);
CREATE INDEX regime_source_time ON regime_changes(source,detected_at_utc);
CREATE TABLE detector_checkpoints (
  source TEXT NOT NULL, origin TEXT NOT NULL, window_id TEXT NOT NULL,
  payload_json TEXT NOT NULL, PRIMARY KEY(source,origin)
);
CREATE TABLE observer_disagreements (
  id TEXT PRIMARY KEY, source TEXT NOT NULL, canary_id TEXT NOT NULL,
  detected_at_utc TEXT NOT NULL, payload_json TEXT NOT NULL, last_observed_utc TEXT NOT NULL
);
CREATE INDEX disagreement_source_time ON observer_disagreements(source,detected_at_utc);
CREATE TABLE diagnostic_snapshots (
  source TEXT PRIMARY KEY, as_of_utc TEXT NOT NULL, payload_json TEXT NOT NULL
);
CREATE TABLE block_diagnostic_samples (
  source TEXT NOT NULL, slot TEXT NOT NULL, block_id TEXT NOT NULL,
  payload_json TEXT NOT NULL, PRIMARY KEY(source,slot,block_id)
);
CREATE TABLE backfill_reports (
  id TEXT PRIMARY KEY, source TEXT NOT NULL, payload_json TEXT NOT NULL
);
