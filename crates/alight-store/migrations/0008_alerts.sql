CREATE TABLE alert_state(source TEXT PRIMARY KEY, hash TEXT NOT NULL, payload_json TEXT NOT NULL);
CREATE TABLE alerts(id TEXT PRIMARY KEY, source TEXT NOT NULL, at_utc TEXT NOT NULL, payload_json TEXT NOT NULL, delivery_status TEXT NOT NULL DEFAULT 'PENDING');
CREATE INDEX alerts_source_time ON alerts(source,at_utc);
