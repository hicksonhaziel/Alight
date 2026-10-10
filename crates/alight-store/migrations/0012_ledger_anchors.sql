CREATE TABLE ledger_anchor_attempts (
  reservation_id TEXT PRIMARY KEY REFERENCES budget_reservations(id),
  source TEXT NOT NULL CHECK(source IN ('live','sim','replay')),
  head_hash TEXT NOT NULL,
  signature TEXT NOT NULL UNIQUE,
  content_hash TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  UNIQUE(source,head_hash)
);
CREATE TABLE ledger_anchor_events (
  id INTEGER PRIMARY KEY,
  reservation_id TEXT NOT NULL REFERENCES ledger_anchor_attempts(reservation_id),
  status TEXT NOT NULL CHECK(status IN ('PREPARED','ACCEPTED','REJECTED','UNKNOWN','FINALIZED','FINALIZED_FAILED')),
  payload_json TEXT NOT NULL
);
CREATE INDEX anchor_events_lookup ON ledger_anchor_events(reservation_id,id);
CREATE TRIGGER anchor_attempt_no_update BEFORE UPDATE ON ledger_anchor_attempts BEGIN SELECT RAISE(ABORT,'immutable anchor attempt'); END;
CREATE TRIGGER anchor_attempt_no_delete BEFORE DELETE ON ledger_anchor_attempts BEGIN SELECT RAISE(ABORT,'immutable anchor attempt'); END;
CREATE TRIGGER anchor_event_no_update BEFORE UPDATE ON ledger_anchor_events BEGIN SELECT RAISE(ABORT,'immutable anchor event'); END;
CREATE TRIGGER anchor_event_no_delete BEFORE DELETE ON ledger_anchor_events BEGIN SELECT RAISE(ABORT,'immutable anchor event'); END;
