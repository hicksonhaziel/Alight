CREATE TABLE model_snapshots (
  hash TEXT PRIMARY KEY, source TEXT NOT NULL, regime_id TEXT NOT NULL,
  as_of_utc TEXT NOT NULL, payload_json TEXT NOT NULL
);
CREATE TABLE forecast_heads (
  source TEXT PRIMARY KEY, sequence INTEGER NOT NULL CHECK(sequence>=0), hash TEXT NOT NULL
);
CREATE TABLE forecast_ledger (
  source TEXT NOT NULL, sequence INTEGER NOT NULL CHECK(sequence>0),
  forecast_id TEXT NOT NULL UNIQUE, prev_hash TEXT NOT NULL, hash TEXT NOT NULL UNIQUE,
  payload_json TEXT NOT NULL, PRIMARY KEY(source,sequence)
);
CREATE TRIGGER forecast_no_update BEFORE UPDATE ON forecast_ledger BEGIN SELECT RAISE(ABORT,'immutable forecast'); END;
CREATE TRIGGER forecast_no_delete BEFORE DELETE ON forecast_ledger BEGIN SELECT RAISE(ABORT,'immutable forecast'); END;
CREATE TABLE forecast_grades (
  id INTEGER PRIMARY KEY, forecast_hash TEXT NOT NULL REFERENCES forecast_ledger(hash),
  graded_at_utc TEXT NOT NULL, payload_json TEXT NOT NULL, UNIQUE(forecast_hash,graded_at_utc)
);
CREATE TABLE signal_reports (
  hash TEXT PRIMARY KEY, source TEXT NOT NULL, day TEXT NOT NULL,
  as_of_utc TEXT NOT NULL, methodology_hash TEXT NOT NULL, payload_json TEXT NOT NULL
);
CREATE INDEX daily_signals ON signal_reports(source,day,as_of_utc);
CREATE TABLE canary_model_context (
  canary_id TEXT PRIMARY KEY REFERENCES canaries(id), payload_json TEXT NOT NULL
);
