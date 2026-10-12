#!/usr/bin/env python3
"""Finite, read-only local soak; persists progress across monitor restarts.

No keys, signing, mutations, provider requests or outbound alerts. A missing
known canary or unavailable endpoint is recorded as a failure, never an outcome.
"""
import argparse
from contextlib import closing
from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import time
from urllib.request import urlopen


def utc():
    return datetime.now(timezone.utc)


def atomic_json(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.chmod(0o600)
    temporary.replace(path)


def sample(root, known):
    failures = []
    item = {"checked_at_utc": utc().isoformat()}
    try:
        with urlopen("http://127.0.0.1:8080/v1/health", timeout=5) as response:
            health = json.loads(response.read(262144))
        item.update({key: health[key] for key in
                     ("mode", "source", "collector_status", "signing_enabled", "canaries_sent")})
        item["observers"] = [{key: observer.get(key) for key in ("observer", "status", "age_ms")}
                             for observer in health["observers"]]
        if health["collector_status"] != "PASS":
            failures.append("collector_not_healthy")
    except Exception:
        failures.append("api_unavailable")
    try:
        with urlopen("http://127.0.0.1:8181", timeout=5) as response:
            if response.status != 200 or b'<div id="root"' not in response.read(262144):
                failures.append("workbench_document_invalid")
    except Exception:
        failures.append("workbench_unavailable")
    try:
        with closing(sqlite3.connect(f"file:{root / 'data/alight.db'}?mode=ro", uri=True, timeout=5)) as db:
            current = {row[0] for row in db.execute("SELECT id FROM canaries WHERE source='live'")}
            missing = known - current
            item["known_canaries"] = len(current)
            item["missing_canaries"] = len(missing)
            if missing:
                failures.append("known_canary_missing")
            item["reserved_lamports"] = db.execute(
                "SELECT coalesce(sum(lamports),0) FROM budget_reservations WHERE source='live'"
            ).fetchone()[0]
            item["outcomes"] = db.execute(
                "SELECT outcome,finalized,count(*) FROM canaries WHERE source='live' GROUP BY outcome,finalized"
            ).fetchall()
        known |= current
    except Exception:
        failures.append("database_unavailable")
    item["failures"] = failures
    return item


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--hours", type=float, default=48)
    parser.add_argument("--interval", type=int, default=60)
    args = parser.parse_args()
    if not 0 < args.hours <= 168 or not 10 <= args.interval <= 3600:
        parser.error("hours must be (0,168]; interval must be 10–3600 seconds")
    output = args.root / ".alight/stability"
    output.mkdir(parents=True, exist_ok=True, mode=0o700)
    state_path = output / "status.json"
    state = json.loads(state_path.read_text()) if state_path.exists() else {
        "schema_version": 1, "started_at_utc": utc().isoformat(),
        "required_seconds": round(args.hours * 3600), "samples": 0,
        "failed_samples": 0, "known_ids": [], "status": "RUNNING",
    }
    start = datetime.fromisoformat(state["started_at_utc"])
    known = set(state["known_ids"])
    while state["status"] == "RUNNING":
        item = sample(args.root, known)
        previous = state.get("checked_at_utc")
        if previous and (utc() - datetime.fromisoformat(previous)).total_seconds() > args.interval * 2 + 20:
            item["failures"].append("monitor_sampling_gap")
        elapsed = (utc() - start).total_seconds()
        state.update(checked_at_utc=item["checked_at_utc"], elapsed_seconds=round(elapsed),
                     known_ids=sorted(known), latest=item, samples=state["samples"] + 1,
                     failed_samples=state["failed_samples"] + bool(item["failures"]))
        with (output / "samples.jsonl").open("a") as file:
            file.write(json.dumps(item) + "\n")
        if elapsed >= state["required_seconds"]:
            state["status"] = "PASS" if state["failed_samples"] == 0 else "REVIEW_REQUIRED"
        atomic_json(state_path, state)
        if state["status"] == "RUNNING":
            time.sleep(args.interval)


if __name__ == "__main__":
    main()
