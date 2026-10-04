#!/usr/bin/env python3
"""Manage the local observe collector through systemd's user service manager."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
STATE = ROOT / ".alight"
BINARY = STATE / "bin" / "alightd"
UNIT = "alight-observe.service"


def service(*args, required=True):
    result = subprocess.run(["systemctl", "--user", *args], capture_output=True, text=True)
    if required and result.returncode:
        raise SystemExit("user service operation failed; run this in the local host terminal")
    return result


def main():
    action = sys.argv[1] if len(sys.argv) == 2 else ""
    if action not in {"start", "stop", "status"}:
        raise SystemExit("usage: python3 scripts/collector.py start|stop|status")
    current = service("is-active", UNIT, required=False)
    if current.returncode not in {0, 3, 4}:
        raise SystemExit("user service manager unavailable; run this in the local host terminal")
    active = current.returncode == 0
    if action == "start" and not active:
        BINARY.parent.mkdir(parents=True, exist_ok=True)
        replacement = BINARY.with_name("alightd.next")
        shutil.copy2(ROOT / "target" / "debug" / "alightd", replacement)
        replacement.replace(BINARY)
        # Keep the executable separate from subsequent cargo builds.
        (STATE / "collector.log").touch(exist_ok=True)
        os.chmod(STATE / "collector.log", 0o600)
        service("link", str(ROOT / "deploy" / UNIT))
        service("daemon-reload")
        service("reset-failed", UNIT, required=False)
        service("enable", "--now", UNIT)
    if action == "stop":
        service("disable", "--now", UNIT)
    active = service("is-active", UNIT, required=False).returncode == 0
    pid = service("show", UNIT, "--property=MainPID", "--value", required=False).stdout.strip()
    print(json.dumps({"mode": "observe", "manager": "systemd_user", "status": "RUNNING" if active else "STOPPED", "pid": int(pid) if pid.isdigit() and pid != "0" else None}))


if __name__ == "__main__":
    try:
        main()
    except OSError:
        raise SystemExit("local collector operation failed; paths and values withheld")
