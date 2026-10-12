-- Exact, transactionally maintained counts for frequent health/stream reads.
-- Backfill once; INSERT OR IGNORE duplicates never fire these AFTER triggers.
CREATE TABLE source_event_counts (
  source TEXT PRIMARY KEY,
  slot_events INTEGER NOT NULL DEFAULT 0 CHECK(slot_events >= 0),
  blocks INTEGER NOT NULL DEFAULT 0 CHECK(blocks >= 0),
  observations INTEGER NOT NULL DEFAULT 0 CHECK(observations >= 0)
);
INSERT INTO source_event_counts(source,slot_events)
SELECT source,COUNT(*) FROM slot_events GROUP BY source;
INSERT INTO source_event_counts(source,blocks)
SELECT source,COUNT(*) FROM blocks GROUP BY source
ON CONFLICT(source) DO UPDATE SET blocks=excluded.blocks;
INSERT INTO source_event_counts(source,observations)
SELECT source,COUNT(*) FROM observations GROUP BY source
ON CONFLICT(source) DO UPDATE SET observations=excluded.observations;

CREATE TRIGGER health_slot_insert AFTER INSERT ON slot_events BEGIN
  INSERT INTO source_event_counts(source,slot_events) VALUES(NEW.source,1)
  ON CONFLICT(source) DO UPDATE SET slot_events=slot_events+1;
END;
CREATE TRIGGER health_slot_delete AFTER DELETE ON slot_events BEGIN
  UPDATE source_event_counts SET slot_events=slot_events-1 WHERE source=OLD.source;
END;
CREATE TRIGGER health_block_insert AFTER INSERT ON blocks BEGIN
  INSERT INTO source_event_counts(source,blocks) VALUES(NEW.source,1)
  ON CONFLICT(source) DO UPDATE SET blocks=blocks+1;
END;
CREATE TRIGGER health_block_delete AFTER DELETE ON blocks BEGIN
  UPDATE source_event_counts SET blocks=blocks-1 WHERE source=OLD.source;
END;
CREATE TRIGGER health_observation_insert AFTER INSERT ON observations BEGIN
  INSERT INTO source_event_counts(source,observations) VALUES(NEW.source,1)
  ON CONFLICT(source) DO UPDATE SET observations=observations+1;
END;
CREATE TRIGGER health_observation_delete AFTER DELETE ON observations BEGIN
  UPDATE source_event_counts SET observations=observations-1 WHERE source=OLD.source;
END;
