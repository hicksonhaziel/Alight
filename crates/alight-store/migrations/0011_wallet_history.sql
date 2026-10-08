CREATE TABLE wallet_history_captures (
  hash TEXT PRIMARY KEY,
  source TEXT NOT NULL CHECK(source IN ('sim','replay')),
  wallet TEXT NOT NULL,
  payload_json TEXT NOT NULL
);
CREATE INDEX wallet_capture_lookup ON wallet_history_captures(source,wallet);
CREATE TRIGGER wallet_capture_no_update BEFORE UPDATE ON wallet_history_captures BEGIN SELECT RAISE(ABORT,'immutable wallet capture'); END;
CREATE TRIGGER wallet_capture_no_delete BEFORE DELETE ON wallet_history_captures BEGIN SELECT RAISE(ABORT,'immutable wallet capture'); END;
