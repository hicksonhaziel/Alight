CREATE TABLE runs (
  id TEXT PRIMARY KEY, mode TEXT NOT NULL, started_utc TEXT NOT NULL,
  ended_utc TEXT, status TEXT NOT NULL DEFAULT 'running'
);
CREATE TABLE raw_evidence (
  ref TEXT PRIMARY KEY, payload_json TEXT NOT NULL, bytes INTEGER NOT NULL CHECK(bytes >= 0)
);
CREATE TABLE slot_events (
  source TEXT NOT NULL, observer TEXT NOT NULL, slot TEXT NOT NULL,
  block_key TEXT NOT NULL, status TEXT NOT NULL, payload_json TEXT NOT NULL,
  PRIMARY KEY(source,observer,slot,block_key,status)
);
CREATE TABLE blocks (
  source TEXT NOT NULL, observer TEXT NOT NULL, slot TEXT NOT NULL, block_id TEXT NOT NULL,
  payload_json TEXT NOT NULL, PRIMARY KEY(source,observer,slot,block_id)
);
CREATE TABLE observations (
  source TEXT NOT NULL, observer TEXT NOT NULL, signature TEXT NOT NULL,
  slot_key TEXT NOT NULL, block_key TEXT NOT NULL, index_scope TEXT NOT NULL,
  index_key TEXT NOT NULL, success_key TEXT NOT NULL, payload_json TEXT NOT NULL,
  PRIMARY KEY(source,observer,signature,slot_key,block_key,index_scope,index_key,success_key)
);
CREATE INDEX observations_sig ON observations(source,signature);
CREATE TABLE observer_state (
  source TEXT NOT NULL, observer TEXT NOT NULL, cursor_slot TEXT,
  last_receive_utc TEXT NOT NULL, PRIMARY KEY(source,observer)
);
CREATE TABLE gaps (
  id INTEGER PRIMARY KEY, source TEXT NOT NULL, observer TEXT NOT NULL,
  started_utc TEXT NOT NULL, ended_utc TEXT, reason TEXT NOT NULL
);
CREATE INDEX open_gaps ON gaps(source,observer) WHERE ended_utc IS NULL;
CREATE TABLE canaries (
  id TEXT PRIMARY KEY, source TEXT NOT NULL, signature TEXT, outcome TEXT,
  finalized INTEGER NOT NULL DEFAULT 0 CHECK(finalized IN (0,1)), payload_json TEXT NOT NULL
);
CREATE INDEX pending_canaries ON canaries(finalized) WHERE finalized=0;
CREATE TABLE resolution_history (
  id INTEGER PRIMARY KEY, canary_id TEXT NOT NULL REFERENCES canaries(id),
  at_utc TEXT NOT NULL, payload_json TEXT NOT NULL
);
CREATE TABLE budget_reservations (
  id TEXT PRIMARY KEY, source TEXT NOT NULL, route TEXT NOT NULL, day TEXT NOT NULL,
  created_ms INTEGER NOT NULL CHECK(created_ms >= 0),
  lamports INTEGER NOT NULL CHECK(lamports > 0)
);
CREATE INDEX budget_day ON budget_reservations(source,day);
CREATE INDEX budget_window ON budget_reservations(source,created_ms);
