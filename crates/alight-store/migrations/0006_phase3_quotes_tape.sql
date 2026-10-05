CREATE TABLE passive_tips (
  source TEXT NOT NULL, observer TEXT NOT NULL, signature TEXT NOT NULL,
  slot TEXT NOT NULL, block_key TEXT NOT NULL, recipient TEXT NOT NULL,
  received_ms INTEGER NOT NULL, bytes INTEGER NOT NULL CHECK(bytes > 0 AND bytes <= 4096),
  payload_json TEXT NOT NULL,
  PRIMARY KEY(source,observer,signature,slot,block_key,recipient)
);
CREATE INDEX tape_retention ON passive_tips(source,received_ms);
CREATE TABLE market_snapshots (
  hash TEXT PRIMARY KEY, source TEXT NOT NULL, regime_id TEXT NOT NULL,
  pool TEXT NOT NULL, as_of_ms INTEGER NOT NULL, payload_json TEXT NOT NULL
);
CREATE INDEX market_lookup ON market_snapshots(source,regime_id,pool,as_of_ms);
CREATE TRIGGER market_no_update BEFORE UPDATE ON market_snapshots BEGIN SELECT RAISE(ABORT,'immutable market snapshot'); END;
CREATE TRIGGER market_no_delete BEFORE DELETE ON market_snapshots BEGIN SELECT RAISE(ABORT,'immutable market snapshot'); END;
