#!/usr/bin/env python3
"""Read-only daily Alight datasets. No ENV loading, network, private URLs or signing."""
import argparse
import csv
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import sys
import tempfile
from datetime import date, datetime, timedelta, timezone

MAX_ROWS = 100_000
MAX_BYTES = 64 * 1024 * 1024
GENESIS = "sha256:" + "0" * 64
LICENSE = "CC-BY-4.0"
TABLES = {
    "canaries": {"source": "string", "id": "string", "sent_at_utc": "string", "route": "string", "size_class": "string", "tip_lamports": "string", "cu_price_micro_lamports": "string", "policy_id": "string", "assignment_prob": "number", "uniform_arm": "boolean", "regime_id": "string", "outcome": "nullable_string", "finalized": "boolean", "payload_json": "string"},
    "curve_snapshots": {"source": "string", "snapshot_id": "string", "as_of_utc": "string", "region": "string", "regime_id": "string", "route": "string", "size_class": "string", "horizon_slots": "integer", "n_effective": "number", "evidence": "string", "payload_json": "string"},
    "regimes": {"source": "string", "id": "string", "detected_at_utc": "string", "regime_id": "string", "payload_json": "string"},
    "forecasts": {"source": "string", "sequence": "string", "created_at_utc": "string", "regime_id": "string", "prev_hash": "string", "hash": "string", "payload_json": "string"},
    "grades": {"source": "string", "forecast_hash": "string", "graded_at_utc": "string", "status": "string", "payload_json": "string"},
}


def digest(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def utc(text):
    t = datetime.fromisoformat(text.replace("Z", "+00:00"))
    if t.tzinfo is None:
        raise ValueError("UTC timestamp required")
    return t.astimezone(timezone.utc)


def safe(value):
    """Fail closed on unsafe artifacts instead of silently changing immutable evidence."""
    if isinstance(value, dict):
        for key, v in value.items():
            if re.search(r"(?i)(api.?key|private.?key|secret|authorization|access.?token|password|keypair)", key):
                raise ValueError("Secret field rejected")
            safe(v)
    elif isinstance(value, list):
        for v in value:
            safe(v)
    elif isinstance(value, str):
        if re.search(r"(?i)(https?://|wss?://|bearer\s|secret_key_|whsec_|-----BEGIN)", value):
            raise ValueError("Private URL or credential pattern rejected")
        if value.startswith(("=", "+", "-", "@", "\t", "\r", "\n")) and not re.fullmatch(r"-?[0-9]+(?:\.[0-9]+)?",value):
            raise ValueError("Spreadsheet formula/control text rejected")
        for key, secret in os.environ.items():
            if len(secret) >= 16 and re.search(r"(?i)(token|secret|private.?key|keypair|operator.?key)", key) and secret in value:
                raise ValueError("Known environment credential rejected")
    elif isinstance(value, float) and (value != value or abs(value) == float("inf")):
        raise ValueError("Nonfinite number rejected")


def bounded(connection, query, params=()):
    rows = connection.execute(query, params).fetchmany(MAX_ROWS + 1)
    if len(rows) > MAX_ROWS or sum(sum(len(v) for v in row if isinstance(v, str)) for row in rows) > MAX_BYTES:
        raise ValueError("Dataset evidence exceeds bounds")
    return rows


def ledger(connection, source):
    previous = GENESIS
    rows = bounded(connection, "SELECT sequence,prev_hash,hash,payload_json FROM forecast_ledger WHERE source=? ORDER BY sequence", (source,))
    models = {}
    markets = {}
    entries = []
    for sequence, prev, hash_value, payload in rows:
        f = json.loads(payload)
        safe(f)
        expected = digest(f"alight.forecast.v1\0{source}\0{sequence}\0{previous}\0{payload}".encode())
        if sequence != len(entries) + 1 or prev != previous or hash_value != expected or f["source"] != source:
            raise ValueError("Ledger verification failed")
        for model in [f["model_snapshot_hash"], *[b["model_snapshot_hash"] for b in f["baselines"] if b.get("model_snapshot_hash")]]:
            if model not in models:
                row = connection.execute("SELECT payload_json FROM model_snapshots WHERE hash=?", (model,)).fetchone()
                if row is None or digest(row[0].encode()) != model:
                    raise ValueError("Frozen model verification failed")
                safe(json.loads(row[0]))
                models[model] = row[0]
        summary = (f.get("economics") or {}).get("economics")
        if summary:
            # Market content hash is stored independently; find its canonical document.
            candidates = bounded(connection, "SELECT hash,payload_json FROM market_snapshots WHERE source=? AND regime_id=? AND pool=? AND as_of_ms<=?", (source, summary["market"]["regime_id"], summary["market"]["pool"], int(utc(summary["market"]["as_of_utc"]).timestamp() * 1000)))
            matching = [(h, p) for h, p in candidates if json.loads(p) == summary["market"]]
            if len(matching) != 1 or digest(matching[0][1].encode()) != matching[0][0]:
                raise ValueError("Market snapshot verification failed")
            h, p = matching[0]
            safe(json.loads(p))
            markets[h] = p
        entries.append({"sequence": str(sequence), "prev_hash": prev, "hash": hash_value, "canonical_json": payload})
        previous = hash_value
    head = connection.execute("SELECT sequence,hash FROM forecast_heads WHERE source=?", (source,)).fetchone()
    if head is None and rows or head is not None and tuple(head) != (len(rows), previous):
        raise ValueError("Persisted ledger head mismatch")
    return {"source": source, "entries": entries, "models": models, "markets": markets, "head": previous, "sequence": str(len(rows))}


def collect(connection, source, day):
    start = datetime.combine(date.fromisoformat(day), datetime.min.time(), timezone.utc)
    end = start + timedelta(days=1)
    lo, hi = start.isoformat(), end.isoformat()
    result = {name: [] for name in TABLES}
    rows = bounded(connection, "SELECT payload_json,finalized FROM canaries WHERE source=? AND julianday(json_extract(payload_json,'$.send_wall_utc'))>=julianday(?) AND julianday(json_extract(payload_json,'$.send_wall_utc'))<julianday(?) ORDER BY json_extract(payload_json,'$.send_wall_utc'),id", (source,lo,hi))
    for payload, finalized in rows:
        c = json.loads(payload); safe(c)
        if c["source"] != source or not start <= utc(c["send_wall_utc"]) < end:
            raise ValueError("Canary source/time mismatch")
        result["canaries"].append({"source":source,"id":c["id"],"sent_at_utc":c["send_wall_utc"],"route":c["config"]["route"],"size_class":c["config"]["size_class"],"tip_lamports":c["config"]["tip_lamports"],"cu_price_micro_lamports":c["config"]["cu_price_micro_lamports"],"policy_id":c["policy_id"],"assignment_prob":c["assignment_prob"],"uniform_arm":c["uniform_arm"],"regime_id":c["regime_id"],"outcome":c["outcome"],"finalized":bool(finalized),"payload_json":payload})
    rows = bounded(connection,"SELECT snapshot_id,payload_json FROM curve_snapshots WHERE source=? AND as_of_ms>=? AND as_of_ms<? ORDER BY as_of_ms,snapshot_id", (source,int(start.timestamp()*1000),int(end.timestamp()*1000)))
    for snapshot_id, payload in rows:
        c=json.loads(payload);safe(c)
        if digest(payload.encode()) != snapshot_id or c["context"]["source"] != source or not start <= utc(c["context"]["as_of_utc"]) < end:
            raise ValueError("Curve integrity/source/time mismatch")
        result["curve_snapshots"].append({"source":source,"snapshot_id":snapshot_id,"as_of_utc":c["context"]["as_of_utc"],"region":c["context"]["region"],"regime_id":c["context"]["regime_id"],"route":c["config"]["route"],"size_class":c["config"]["size_class"],"horizon_slots":c["horizon_slots"],"n_effective":c["n_effective"],"evidence":c["evidence"],"payload_json":payload})
    for payload, in bounded(connection,"SELECT payload_json FROM regime_changes WHERE source=? AND julianday(detected_at_utc)>=julianday(?) AND julianday(detected_at_utc)<julianday(?) ORDER BY julianday(detected_at_utc),id",(source,lo,hi)):
        r=json.loads(payload);safe(r)
        if r["source"]!=source or not start<=utc(r["detected_at_utc"])<end: raise ValueError("Regime source/time mismatch")
        result["regimes"].append({"source":source,"id":r["id"],"detected_at_utc":r["detected_at_utc"],"regime_id":r["regime_id"],"payload_json":payload})
    witness = ledger(connection,source)
    for e in witness["entries"]:
        f=json.loads(e["canonical_json"])
        if start<=utc(f["created_at_utc"])<end:
            result["forecasts"].append({"source":source,"sequence":e["sequence"],"created_at_utc":f["created_at_utc"],"regime_id":f["regime_id"],"prev_hash":e["prev_hash"],"hash":e["hash"],"payload_json":e["canonical_json"]})
    for payload, in bounded(connection,"SELECT g.payload_json FROM forecast_grades g JOIN forecast_ledger f ON f.hash=g.forecast_hash WHERE f.source=? AND julianday(g.graded_at_utc)>=julianday(?) AND julianday(g.graded_at_utc)<julianday(?) ORDER BY g.id",(source,lo,hi)):
        g=json.loads(payload);safe(g)
        if not start<=utc(g["graded_at_utc"])<end:raise ValueError("Grade time mismatch")
        result["grades"].append({"source":source,"forecast_hash":g["forecast_hash"],"graded_at_utc":g["graded_at_utc"],"status":g["status"],"payload_json":payload})
    return result,witness


def export(database, source, day, output, formats):
    # Preserve application DBs: no migrations, pragmas changing journaling, or mutable connection.
    path=Path(database).resolve(strict=True)
    out=Path(output)
    if out.exists() or not out.parent.is_dir(): raise ValueError("Output must be a new directory with an existing parent")
    with closing(sqlite3.connect(path.as_uri()+"?mode=ro",uri=True,timeout=5)) as connection:
        connection.execute("PRAGMA query_only=ON")
        connection.execute("BEGIN")
        tables,witness=collect(connection,source,day)
        connection.rollback()
    schema={"schema_version":1,"source":source,"day":day,"tables":TABLES,"null_csv":"Empty cell; quoted payload_json preserves original nulls","units":"Lamports and micro-lamports/CU are decimal strings. Times UTC. Horizon slots. Effective n discounted sample mass.","row_limit_per_table":MAX_ROWS}
    pa=pq=None
    if "parquet" in formats:
        import pyarrow as pa
        import pyarrow.parquet as pq
    temp=Path(tempfile.mkdtemp(prefix=".alight-export-",dir=out.parent))
    try:
        (temp/"schema.json").write_text(json.dumps(schema,indent=2)+"\n")
        (temp/"ledger-witness.json").write_text(json.dumps(witness,indent=2)+"\n")
        (temp/"LICENSE.txt").write_text("Alight datasets: Creative Commons Attribution 4.0 International (CC BY 4.0).\nLicense terms: https://creativecommons.org/licenses/by/4.0/legalcode\nAttribute Hickson / Alight, preserve source, date, vantage and methodology labels, and identify modifications.\nLicense applies to these exported records, not provider software or arbitrary private wallet captures.\n")
        for name,fields in TABLES.items():
            safe(tables[name])
            if "csv" in formats:
                with (temp/(name+".csv")).open("w",newline="") as stream:
                    writer=csv.DictWriter(stream,fieldnames=fields,lineterminator="\n")
                    writer.writeheader();writer.writerows(tables[name])
            if "parquet" in formats:
                types={"string":pa.string(),"nullable_string":pa.string(),"number":pa.float64(),"integer":pa.int64(),"boolean":pa.bool_()}
                arrow=pa.schema([pa.field(k,types[v],nullable=v=="nullable_string") for k,v in fields.items()])
                pq.write_table(pa.Table.from_pylist(tables[name],schema=arrow),temp/(name+".parquet"),compression="zstd")
        files={p.name:{"bytes":p.stat().st_size,"sha256":digest(p.read_bytes())} for p in sorted(temp.iterdir())}
        if sum(f["bytes"] for f in files.values())>MAX_BYTES:raise ValueError("Bundle exceeds 64 MiB")
        manifest={"schema_version":1,"kind":"alight_daily_dataset","source":source,"day":day,"license":LICENSE,"formats":formats,"tables":{name:{"rows":len(rows)} for name,rows in tables.items()},"regions":sorted({r["region"] for r in tables["curve_snapshots"]}),"ledger":{"verified":True,"head":witness["head"],"sequence":witness["sequence"],"witness_scope":"Full source chain, including forecasts outside selected day"},"coverage":"Rows retained at export snapshot; daily canaries selected by send time, curves by as-of, regimes by detection, forecasts by issue and grades by grading time. This is not complete chain or wallet history.","vantage":"Curve region labels are retained. Canary region is the collection host; no region is inferred when absent from history.","limits":["Sim/replay are not Live evidence.","Unresolved and unfinalized outcomes remain visible; no timeout labels.","Original payload JSON and immutable forecast hashes are preserved; suspicious secret-bearing evidence rejects the export.","Detected regime events only; a workload regime label does not fabricate a detection."],"files":files}
        (temp/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n")
        checks="".join(hashlib.sha256(p.read_bytes()).hexdigest()+"  "+p.name+"\n" for p in sorted(temp.iterdir()))
        (temp/"SHA256SUMS").write_text(checks)
        if out.exists():raise ValueError("Output already exists")
        temp.rename(out)
        return manifest
    finally:
        if temp.exists():shutil.rmtree(temp)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--database",required=True);p.add_argument("--source",choices=["live","sim","replay"],required=True)
    p.add_argument("--day",required=True);p.add_argument("--output",required=True)
    p.add_argument("--format",choices=["csv","parquet","both"],default="both")
    a=p.parse_args()
    try:
        if date.fromisoformat(a.day).isoformat()!=a.day:raise ValueError("Invalid UTC day")
        manifest=export(a.database,a.source,a.day,a.output,["csv","parquet"] if a.format=="both" else [a.format])
        print(json.dumps({"source":a.source,"day":a.day,"tables":manifest["tables"],"output":a.output}))
        return 0
    except (ValueError,KeyError,TypeError,sqlite3.Error,OSError,ImportError):
        print("Dataset export failed: invalid/missing evidence, output, dependency or bounds. No evidence values printed.",file=sys.stderr)
        return 2


if __name__=="__main__":
    raise SystemExit(main())
