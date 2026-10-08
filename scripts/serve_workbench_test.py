#!/usr/bin/env python3
"""Disposable, offline Sim daemon for browser acceptance. Never reads project ENV."""
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import json

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="alight-workbench-") as directory:
    key = Path(directory) / "operator.key"
    key.write_text("local-phase4-browser-test-only")
    key.chmod(0o600)
    child = subprocess.Popen([
        str(root / "target/debug/alightd"), "--mode", "sim", "--seed", "42",
        "--sim-canaries", "900", "--bind", "127.0.0.1:8082",
        "--sim-regimes", "true",
        "--db", str(Path(directory) / "sim.db"), "--operator-key-file", str(key),
    ], cwd=root, env={"PATH": os.environ["PATH"], "ALIGHT_SIGNING_KEY_FILE": "/must-not-load"})
    runtime = root / ".alight/phase6/browser-runtime.json"
    runtime.parent.mkdir(parents=True, exist_ok=True)
    runtime.write_text(json.dumps({"database":str(Path(directory)/"sim.db"),"source":"sim"}))
    def stop(_signum, _frame):
        child.terminate()
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        raise SystemExit(child.wait())
    finally:
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
