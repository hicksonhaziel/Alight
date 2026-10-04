CREATE TABLE policy_state (policy_key TEXT PRIMARY KEY, next_draw INTEGER NOT NULL CHECK(next_draw >= 0));
CREATE TABLE send_attempts (
  canary_id TEXT PRIMARY KEY REFERENCES canaries(id), policy_key TEXT NOT NULL,
  draw INTEGER NOT NULL CHECK(draw >= 0), assignment_json TEXT NOT NULL,
  wire_sha256 TEXT NOT NULL, prepared_utc TEXT NOT NULL, completed_utc TEXT,
  status TEXT NOT NULL CHECK(status IN ('PREPARED','ACCEPTED','REJECTED','UNKNOWN')),
  result_json TEXT, UNIQUE(policy_key,draw)
);
