#!/usr/bin/env python3
"""Offline dataset regression: exact day/source, unknown outcomes, tampering, secrets and formats."""
import copy
from contextlib import closing, contextmanager
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

from export_dataset import export, safe
from verify_dataset import verify

ROOT=Path(__file__).resolve().parents[1]

@contextmanager
def connection(path):
    with closing(sqlite3.connect(path)) as db:
        with db:
            yield db


class DatasetTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(prefix="alight-dataset-test-")
        cls.root=Path(cls.temp.name)
        cls.db=cls.root/"sim.db"
        subprocess.run([str(ROOT/"target/debug/alight"),"sim","--phase2","--seed","61","--canaries","243","--database",str(cls.db),"--output",str(cls.root/"sim.json")],cwd=cls.root,check=True,stdout=subprocess.DEVNULL)
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()
    def database(self,name):
        target=self.root/(name+".db")
        with connection(self.db) as old,connection(target) as new:old.backup(new)
        return target
    def test_both_formats_witness_schema_and_read_only_reproducibility(self):
        original=hashlib.sha256(self.db.read_bytes()).hexdigest()
        a=export(self.db,"sim","2026-10-05",self.root/"both-a",["csv","parquet"])
        b=export(self.db,"sim","2026-10-05",self.root/"both-b",["csv","parquet"])
        verify(self.root/"both-a");verify(self.root/"both-b")
        self.assertEqual(a,b);self.assertEqual(a["tables"]["canaries"]["rows"],243)
        self.assertEqual(a["tables"]["forecasts"]["rows"],2)
        self.assertEqual(original,hashlib.sha256(self.db.read_bytes()).hexdigest())
        with self.assertRaises(ValueError):export(self.db,"sim","2026-10-05",self.root/"both-a",["csv"])
        with (self.root/"both-a"/"canaries.csv").open("a") as stream:stream.write("tampered\n")
        with self.assertRaises(AssertionError):verify(self.root/"both-a")
    def test_exact_utc_day_source_and_unresolved_records_are_preserved(self):
        db=self.database("boundaries")
        with connection(db) as db_connection:
            c=json.loads(db_connection.execute("SELECT payload_json FROM canaries LIMIT 1").fetchone()[0])
            for i,at in enumerate(["2026-10-04T23:59:59.999Z","2026-10-06T00:00:00Z","2026-10-06T01:00:00+01:00","2026-10-05T01:00:00+01:00"]):
                row=copy.deepcopy(c);row["id"]=f"boundary-{i}";row["send_wall_utc"]=at;row["outcome"]=None
                db_connection.execute("INSERT INTO canaries(id,source,finalized,payload_json) VALUES(?,?,0,?)",(row["id"],"sim",json.dumps(row)))
            replay=copy.deepcopy(c);replay["id"]="other-source";replay["source"]="replay"
            db_connection.execute("INSERT INTO canaries(id,source,finalized,payload_json) VALUES(?,?,0,?)",(replay["id"],"replay",json.dumps(replay)))
        m=export(db,"sim","2026-10-05",self.root/"boundary-export",["csv"])
        verify(self.root/"boundary-export")
        self.assertEqual(m["tables"]["canaries"]["rows"],244)
        import csv
        with (self.root/"boundary-export"/"canaries.csv").open(newline="") as stream:rows=list(csv.DictReader(stream))
        unknown=next(r for r in rows if r["id"]=="boundary-3")
        self.assertEqual(unknown["outcome"],"");self.assertEqual(unknown["finalized"],"False")
        empty=export(db,"live","2026-10-05",self.root/"empty-live",["csv","parquet"])
        verify(self.root/"empty-live");self.assertTrue(all(v["rows"]==0 for v in empty["tables"].values()))
    def test_frozen_model_corruption_and_secrets_fail_without_partial_publication(self):
        db=self.database("corrupt-model")
        with connection(db) as db_connection:db_connection.execute("UPDATE model_snapshots SET payload_json='[]'")
        with self.assertRaises(ValueError):export(db,"sim","2026-10-05",self.root/"corrupt-export",["csv"])
        self.assertFalse((self.root/"corrupt-export").exists())
        db=self.database("secret-evidence")
        with connection(db) as db_connection:
            id,payload=db_connection.execute("SELECT id,payload_json FROM canaries LIMIT 1").fetchone()
            c=json.loads(payload);c["policy_id"]="https://private.invalid/?token=test"
            db_connection.execute("UPDATE canaries SET payload_json=? WHERE id=?",(json.dumps(c),id))
        with self.assertRaises(ValueError):export(db,"sim","2026-10-05",self.root/"secret-export",["csv"])
        self.assertFalse((self.root/"secret-export").exists())
        for value in [{"api_key":"withheld"},"=cmd()","Bearer test","whsec_example"]:
            with self.assertRaises(ValueError):safe(value)

    def test_missing_frozen_witness_is_rejected_even_with_matching_checksums(self):
        directory=self.root/"missing-witness"
        export(self.db,"sim","2026-10-05",directory,["csv"])
        path=directory/"ledger-witness.json"
        witness=json.loads(path.read_text());witness["models"]={}
        path.write_text(json.dumps(witness))
        manifest_path=directory/"manifest.json"
        manifest=json.loads(manifest_path.read_text())
        manifest["files"][path.name]={"bytes":path.stat().st_size,"sha256":"sha256:"+hashlib.sha256(path.read_bytes()).hexdigest()}
        manifest_path.write_text(json.dumps(manifest))
        (directory/"SHA256SUMS").write_text("".join(hashlib.sha256(p.read_bytes()).hexdigest()+"  "+p.name+"\n" for p in sorted(directory.iterdir()) if p.name!="SHA256SUMS"))
        with self.assertRaises(AssertionError):verify(directory)


if __name__=="__main__":unittest.main()
