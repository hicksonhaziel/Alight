#!/usr/bin/env python3
"""Verify Phase 5's offline evidence and the code snapshot that published it."""
import hashlib
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
name = "data/phase-5-validation.json"
receipt = (root / name).read_bytes()
r = json.loads(receipt)
published = subprocess.check_output(
    ["git", "log", "-1", "--format=%H", "--", name], cwd=root, text=True
).strip()
if published:
    historical = subprocess.check_output(["git", "show", f"{published}:{name}"], cwd=root)
    if historical != receipt:
        published = ""

assert r["schema_version"] == 1 and r["phase"] == 5
assert r["status"] == "OFFLINE_PASS_LIVE_PENDING"
assert r["phase_complete"] is False
assert set(r["tasks"]) == {f"5.{i}" for i in range(1, 10)}
assert all(t["offline"] == "PASS" for t in r["tasks"].values())
assert all(r["tasks"][t]["live_acceptance"] == "PENDING"
           for t in ("5.3", "5.5", "5.9"))
assert r["runtime"]["collection_resumed"] is False
assert r["runtime"]["owned_live_transactions_sent"] == 0
assert r["runtime"]["provider_requests"] == 0
assert r["checks"]["rust_workspace"] == {"passed": 97, "failed": 0}
assert r["checks"]["browser"]["passed"] == 7
assert r["checks"]["browser"]["failed"] == 0
assert r["checks"]["browser"]["axe_audits"] == 15
assert r["checks"]["browser"]["axe_violations"] == 0
assert r["checks"]["browser"]["held_out_resolved"] == 40
assert r["checks"]["browser"]["tampered_export_rejected"] is True
assert r["checks"]["browser"]["copied_typescript_compiles"] is True
assert r["checks"]["sdk_tests"]["passed"] == 4
assert r["checks"]["web_unit_tests"]["passed"] == 3
assert all(r["checks"][k] == "PASS" for k in
           ("format", "all_target_clippy", "sdk_lint", "sdk_generator", "web_lint",
            "web_build", "openapi_snapshot", "repository_scan", "historical_phase4_receipt",
            "historical_phase3_receipt", "compose_config", "developer_regression"))
assert r["checks"]["production_proxy"]["status"] == "PASS"
assert r["checks"]["collector_image"]["status"] == "PASS"
assert r["checks"]["web_image"]["status"] == "PASS"
detector = r["detector"]
assert detector["source"] == "sim" and detector["changes"] == 2
assert all(0 <= delay <= 8 for delay in detector["delay_windows"])
assert detector["stationary_windows"] == 12000
assert detector["stationary_false_changes"] == 0
assert len(r["code_sha256"]) >= 60
for path, digest in r["code_sha256"].items():
    assert not path.startswith("/") and ".." not in Path(path).parts
    assert path not in {"ALIGHT_PLAN.md", "ALIGHT_BUILD.md", "context.txt", ".env", "README.md"}
    data = (subprocess.check_output(["git", "show", f"{published}:{path}"], cwd=root)
            if published else (root / path).read_bytes())
    assert hashlib.sha256(data).hexdigest() == digest, path
print(f"PASS: Phase 5 offline receipt and {len(r['code_sha256'])} file hashes; live acceptance remains pending.")
