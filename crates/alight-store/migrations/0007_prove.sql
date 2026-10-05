CREATE TABLE prove_sessions (
  id TEXT PRIMARY KEY, source TEXT NOT NULL,
  forecast_hash TEXT NOT NULL REFERENCES forecast_ledger(hash),
  cell_key TEXT NOT NULL, lock_hash TEXT NOT NULL, lock_json TEXT NOT NULL
);
CREATE TRIGGER prove_lock_no_update BEFORE UPDATE ON prove_sessions BEGIN SELECT RAISE(ABORT,'immutable Prove lock'); END;
CREATE TRIGGER prove_lock_no_delete BEFORE DELETE ON prove_sessions BEGIN SELECT RAISE(ABORT,'immutable Prove lock'); END;
CREATE TABLE prove_state (
  session_id TEXT PRIMARY KEY REFERENCES prove_sessions(id), status TEXT NOT NULL,
  updated_utc TEXT NOT NULL, report_json TEXT NOT NULL
);
CREATE TABLE prove_cells (
  source TEXT NOT NULL, cell_key TEXT NOT NULL,
  session_id TEXT NOT NULL UNIQUE REFERENCES prove_sessions(id), PRIMARY KEY(source,cell_key)
);
CREATE TABLE prove_attempts (
  session_id TEXT NOT NULL REFERENCES prove_sessions(id), ordinal INTEGER NOT NULL CHECK(ordinal>=0),
  canary_id TEXT NOT NULL UNIQUE REFERENCES canaries(id), PRIMARY KEY(session_id,ordinal)
);
CREATE TRIGGER prove_attempt_no_update BEFORE UPDATE ON prove_attempts BEGIN SELECT RAISE(ABORT,'immutable Prove membership'); END;
CREATE TRIGGER prove_attempt_no_delete BEFORE DELETE ON prove_attempts BEGIN SELECT RAISE(ABORT,'immutable Prove membership'); END;
