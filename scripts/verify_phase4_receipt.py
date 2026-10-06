#!/usr/bin/env python3
"""Validate the bounded Phase 4 receipt and its publishing code snapshot."""
import hashlib
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
name = "data/phase-4-validation.json"
receipt = (root / name).read_bytes()
r = json.loads(receipt)
published = subprocess.check_output(
    ["git", "log", "-1", "--format=%H", "--", name], cwd=root, text=True
).strip()
if published:
    historical = subprocess.check_output(["git", "show", f"{published}:{name}"], cwd=root)
    if historical != receipt:
        published = ""

assert r["schema_version"] == 1 and r["phase"] == 4
assert r["status"] == "WORKBENCH_OFFLINE_PASS_DEPENDENCIES_PENDING"
assert r["phase_complete"] is False
assert len(r["tasks"]) == 10
assert r["tasks"]["4.8"]["real_wallet_receipt"] == "PENDING"
assert r["tasks"]["4.6"]["detector_backfill"] == "PENDING"
assert r["tasks"]["4.7"]["observer_aggregates"] == "PENDING"
assert r["runtime"]["collection_resumed"] is False
assert r["runtime"]["owned_live_transactions_sent"] == 0
assert r["runtime"]["provider_requests"] == 0
assert r["checks"]["rust_workspace"]["passed"] == 85
assert r["checks"]["rust_workspace"]["failed"] == 0
assert r["checks"]["browser"]["passed"] == 5
assert r["checks"]["browser"]["failed"] == 0
assert r["checks"]["browser"]["held_out_resolved"] == 40
assert r["checks"]["browser"]["axe_audits"] == 14
assert r["checks"]["browser"]["axe_violations"] == 0
assert r["checks"]["browser"]["tampered_export_rejected"] is True
assert r["checks"]["browser"]["copied_typescript_compiles"] is True
assert all(r["checks"][k] == "PASS" for k in
           ("format", "all_target_clippy", "sdk_lint", "sdk_generator", "web_lint",
            "web_build", "repository_scan", "historical_phase3_receipt", "compose_config"))
assert r["checks"]["web_unit_tests"]["passed"] == 3
assert r["checks"]["developer_regression"]["status"] == "PASS"
assert r["checks"]["production_proxy"]["status"] == "PASS"
assert len(r["code_sha256"]) >= 40
for path, digest in r["code_sha256"].items():
    assert not path.startswith("/") and ".." not in Path(path).parts
    assert path not in {"ALIGHT_PLAN.md", "ALIGHT_BUILD.md", "context.txt", ".env", "README.md"}
    data = (subprocess.check_output(["git", "show", f"{published}:{path}"], cwd=root)
            if published else (root / path).read_bytes())
    assert hashlib.sha256(data).hexdigest() == digest, path
print(f"PASS: Phase 4 workbench receipt and {len(r['code_sha256'])} file hashes; live and later-phase acceptance remain pending.")
