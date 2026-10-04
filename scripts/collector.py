#!/usr/bin/env python3
"""Start/stop a detached local observe collector. No keys or endpoints are printed."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
STATE = ROOT / ".alight"
PIDFILE = STATE / "collector.pid"
BINARY = STATE / "bin" / "alightd"


def running_pid():
    try:
        pid = int(PIDFILE.read_text().strip())
        args = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
        if args[0].decode() == str(BINARY):
            os.kill(pid, 0)
            return pid
    except (OSError, ValueError):
        pass
    return None


def main():
    action = sys.argv[1] if len(sys.argv) == 2 else ""
    if action not in {"start", "stop", "status"}:
        raise SystemExit("usage: python3 scripts/collector.py start|stop|status")
    pid = running_pid()
    if action == "start" and pid is None:
        BINARY.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / "target" / "debug" / "alightd", BINARY)
        # Keep the executable separate from subsequent cargo builds.
        with open(STATE / "collector.log", "ab") as log:
            os.chmod(STATE / "collector.log", 0o600)
            child = subprocess.Popen([str(BINARY), "--mode", "observe"], cwd=ROOT,
                                     stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                     start_new_session=True)
        PIDFILE.write_text(str(child.pid) + "\n")
        os.chmod(PIDFILE, 0o600)
        print(json.dumps({"mode": "observe", "status": "STARTING", "pid": child.pid}))
        return
    if action == "stop" and pid is not None:
        os.kill(pid, signal.SIGTERM)
        for _ in range(100):
            if running_pid() is None:
                break
            time.sleep(0.1)
        else:
            raise SystemExit("collector is still draining; inspect local log")
        PIDFILE.unlink(missing_ok=True)
        pid = None
    print(json.dumps({"mode": "observe", "status": "RUNNING" if pid else "STOPPED", "pid": pid}))


if __name__ == "__main__":
    try:
        main()
    except OSError:
        raise SystemExit("local collector operation failed; paths and values withheld")
