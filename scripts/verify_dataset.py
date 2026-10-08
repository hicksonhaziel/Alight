#!/usr/bin/env python3
"""Validate checksums, schema, CSV/Parquet equality and the original forecast chain."""
import csv
from datetime import date, datetime, timedelta, timezone
import hashlib
import json
import math
from pathlib import Path
import sys


def verify(directory):
    root=Path(directory)
    manifest=json.loads((root/"manifest.json").read_text())
    schema=json.loads((root/"schema.json").read_text())
    assert manifest["kind"]=="alight_daily_dataset" and schema["schema_version"]==1
    assert manifest["source"]==schema["source"] and manifest["day"]==schema["day"]
    assert manifest["source"] in ("live","sim","replay")
    assert set(manifest["formats"]) in ({"csv"},{"parquet"},{"csv","parquet"})
    assert set(schema["tables"])==set(manifest["tables"])=={"canaries","curve_snapshots","regimes","forecasts","grades"}
    start=datetime.combine(date.fromisoformat(manifest["day"]),datetime.min.time(),timezone.utc)
    end=start+timedelta(days=1)
    table_rows={}
    expected=set(manifest["files"])|{"manifest.json"}
    checks={}
    for line in (root/"SHA256SUMS").read_text().splitlines():
        digest,name=line.split("  ",1)
        assert Path(name).name==name and name not in checks
        checks[name]=digest
        assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest
    assert set(checks)==expected
    assert {p.name for p in root.iterdir()}==expected|{"SHA256SUMS"}
    for name,properties in manifest["files"].items():
        assert properties["sha256"]=="sha256:"+checks[name]
        assert properties["bytes"]==(root/name).stat().st_size
    for table,fields in schema["tables"].items():
        csv_rows=parquet_rows=None
        if "csv" in manifest["formats"]:
            with (root/(table+".csv")).open(newline="") as stream:
                reader=csv.DictReader(stream)
                assert reader.fieldnames==list(fields)
                csv_rows=[]
                for raw in reader:
                    row={}
                    for field,kind in fields.items():
                        value=raw[field]
                        if kind=="nullable_string" and value=="":value=None
                        elif kind=="boolean":
                            assert value in ("True","False")
                            value=value=="True"
                        elif kind=="integer":value=int(value)
                        elif kind=="number":value=float(value)
                        row[field]=value
                    assert row["source"]==manifest["source"]
                    json.loads(row["payload_json"])
                    csv_rows.append(row)
            assert len(csv_rows)==manifest["tables"][table]["rows"]
        if "parquet" in manifest["formats"]:
            import pyarrow.parquet as pq
            data=pq.read_table(root/(table+".parquet"))
            assert data.column_names==list(fields)
            parquet_rows=data.to_pylist()
            assert len(parquet_rows)==manifest["tables"][table]["rows"]
            assert all(row["source"]==manifest["source"] for row in parquet_rows)
        if csv_rows is not None and parquet_rows is not None:assert csv_rows==parquet_rows
        rows=csv_rows if csv_rows is not None else parquet_rows
        assert rows is not None
        for row in rows:
            for field,kind in fields.items():
                value=row[field]
                if kind=="nullable_string":assert value is None or isinstance(value,str)
                elif kind=="string":assert isinstance(value,str)
                elif kind=="boolean":assert isinstance(value,bool)
                elif kind=="integer":assert type(value) is int
                elif kind=="number":assert type(value) in (int,float) and math.isfinite(value)
                else:raise AssertionError("Unknown schema type")
            assert row["source"]==manifest["source"]
            time_field={"canaries":"sent_at_utc","curve_snapshots":"as_of_utc","regimes":"detected_at_utc","forecasts":"created_at_utc","grades":"graded_at_utc"}[table]
            at=datetime.fromisoformat(row[time_field].replace("Z","+00:00"))
            assert at.tzinfo is not None and start<=at<end
            payload=json.loads(row["payload_json"])
            if table=="curve_snapshots":
                assert row["snapshot_id"]=="sha256:"+hashlib.sha256(row["payload_json"].encode()).hexdigest()
                assert payload["context"]["source"]==row["source"]
            elif table!="grades":assert payload["source"]==row["source"]
        table_rows[table]=rows
    witness=json.loads((root/"ledger-witness.json").read_text())
    assert witness["source"]==manifest["source"]
    previous="sha256:"+"0"*64
    for sequence,e in enumerate(witness["entries"],1):
        assert e["sequence"]==str(sequence) and e["prev_hash"]==previous
        body=f"alight.forecast.v1\0{witness['source']}\0{sequence}\0{previous}\0{e['canonical_json']}"
        assert e["hash"]=="sha256:"+hashlib.sha256(body.encode()).hexdigest()
        assert json.loads(e["canonical_json"])["source"]==witness["source"]
        forecast=json.loads(e["canonical_json"])
        referenced=[forecast["model_snapshot_hash"],*[b["model_snapshot_hash"] for b in forecast["baselines"] if b.get("model_snapshot_hash")]]
        assert all(digest in witness["models"] for digest in referenced)
        market=(forecast.get("economics") or {}).get("economics")
        if market:assert any(json.loads(document)==market["market"] for document in witness["markets"].values())
        previous=e["hash"]
    for table in ("models","markets"):
        for digest,document in witness[table].items():assert digest=="sha256:"+hashlib.sha256(document.encode()).hexdigest()
    assert witness["sequence"]==str(len(witness["entries"])) and witness["head"]==previous
    assert manifest["ledger"]["head"]==previous and manifest["ledger"]["sequence"]==witness["sequence"]
    assert manifest["ledger"]["verified"] is True
    entries={e["hash"]:e for e in witness["entries"]}
    for row in table_rows["forecasts"]:
        entry=entries[row["hash"]]
        assert row["payload_json"]==entry["canonical_json"] and row["sequence"]==entry["sequence"] and row["prev_hash"]==entry["prev_hash"]
    assert all(row["forecast_hash"] in entries for row in table_rows["grades"])
    return manifest


if __name__=="__main__":
    try:
        m=verify(sys.argv[1])
        print(f"PASS: {m['source']} {m['day']}; checksums, schema, rows and forecast witness verified.")
    except (AssertionError,KeyError,ValueError,OSError,ImportError,IndexError):
        print("FAIL: dataset verification rejected the artifact.",file=sys.stderr)
        raise SystemExit(1)
