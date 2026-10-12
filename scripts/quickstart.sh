#!/usr/bin/env bash
# One local, populated Sim session. Never changes .env or starts a Live sender.
set -euo pipefail
umask 077
if ! command -v python3 >/dev/null 2>&1; then
  printf '%s\n' 'Python 3 is required. See docs/quickstart.md.' >&2
  exit 1
fi
alight_script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec python3 - "$alight_script_dir/.." "$@" <<'PY'
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

root = Path(sys.argv[1]).resolve()
parser = argparse.ArgumentParser(prog="scripts/quickstart.sh", description="Build and start a populated local Sim Workbench. Never modifies .env or sends transactions.")
parser.add_argument("--skip-build", action="store_true", help="reuse existing debug binaries and web dependencies; skip Cargo/npm installation")
parser.add_argument("--run-for", type=int, metavar="SECONDS", help="stop both servers after this many seconds of ready runtime")
parser.add_argument("--api-port", type=int, default=8081, help="loopback API port (default: 8081)")
parser.add_argument("--web-port", type=int, default=5173, help="loopback Workbench port (default: 5173)")
args = parser.parse_args(sys.argv[2:])
if not all(1 <= port <= 65535 for port in (args.api_port, args.web_port)) or args.api_port == args.web_port:
    parser.error("choose two distinct ports in 1..65535")
if args.run_for is not None and args.run_for <= 0:
    parser.error("--run-for must be positive")

children = []
logs = []

class Stopped(Exception):
    def __init__(self, code):
        self.code = code

def stop(signum, _frame):
    raise Stopped(128 + signum)

def spawn(command, **options):
    child = subprocess.Popen(command, start_new_session=True, **options)
    children.append(child)
    return child

def run(command, **options):
    child = spawn(command, **options)
    code = child.wait()
    children.remove(child)
    if code:
        raise RuntimeError(f"{Path(command[0]).name} failed ({code}); see its output above")

def free_ports():
    for port in (args.api_port, args.web_port):
        with socket.socket() as sock:
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            try:
                sock.bind(("127.0.0.1", port))
            except OSError:
                raise RuntimeError(f"Port {port} is busy; stop the other process or choose --api-port/--web-port. No existing server will be reused.") from None

def wait_for_sim(url, child, name):
    deadline = time.monotonic() + 75
    while time.monotonic() < deadline:
        if child.poll() is not None:
            raise RuntimeError(f"{name} exited during startup; inspect .alight/quickstart/{name}.log")
        try:
            with urllib.request.urlopen(url + "/v1/health", timeout=1) as response:
                raw = response.read(65537)
            if len(raw) > 65536:
                raise RuntimeError("Unexpected oversized health response")
            health = json.loads(raw)
            if not isinstance(health, dict):
                raise RuntimeError("Unexpected health response; refusing to attach")
            if health.get("source") != "sim" or health.get("signing_enabled") is not False:
                raise RuntimeError("Expected a non-signing Sim server; refusing to attach to other evidence")
            if child.poll() is None:
                return
        except (OSError, ValueError, urllib.error.URLError):
            pass
        time.sleep(0.25)
    raise RuntimeError(f"{name} was not ready within 75 seconds; inspect .alight/quickstart/{name}.log")

signal.signal(signal.SIGINT, stop)
signal.signal(signal.SIGTERM, stop)
exit_code = 0
try:
    for name in ("node", "npm"):
        if not shutil.which(name):
            raise RuntimeError(f"{name} is required; see docs/quickstart.md")
    node_version = subprocess.check_output(["node", "-p", "process.versions.node"], text=True).strip()
    if tuple(map(int, node_version.split(".")[:2])) < (22, 22):
        raise RuntimeError("Node 22.22 or later is required; see docs/quickstart.md")
    if not args.skip_build:
        for name in ("cargo", "cmake", "make", "pkg-config"):
            if not shutil.which(name):
                raise RuntimeError(f"{name} is required; see docs/quickstart.md")
        if not any(shutil.which(name) for name in ("c++", "g++", "clang++")):
            raise RuntimeError("A native C++ compiler is required; see docs/quickstart.md")
    free_ports()
    state = root / ".alight/quickstart"
    state.mkdir(parents=True, exist_ok=True)
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    if not target.is_absolute():
        target = root / target
    binary = target / "debug/alightd"
    cli = target / "debug/alight"
    vite = root / "web/node_modules/vite/bin/vite.js"
    if not args.skip_build:
        print("Building Alight with one Cargo job…", flush=True)
        run(["cargo", "build", "--locked", "--jobs", "1", "-p", "alight", "-p", "alightd"], cwd=root)
        digest = hashlib.sha256((root / "web/package.json").read_bytes() + (root / "web/package-lock.json").read_bytes()).hexdigest()
        stamp = state / "web-dependencies.sha256"
        if not vite.is_file() or not stamp.is_file() or stamp.read_text().strip() != digest:
            print("Installing pinned Workbench dependencies…", flush=True)
            run(["npm", "--prefix", "web", "ci", "--ignore-scripts", "--no-audit", "--no-fund"], cwd=root)
            stamp.write_text(digest + "\n")
    if not all(path.is_file() and os.access(path, os.X_OK) for path in (binary, cli)) or not vite.is_file():
        raise RuntimeError("Built alight/alightd and web dependencies are required; run without --skip-build")

    # The same predev asset copy as npm run dev; no provider or wallet values.
    run(["node", str(root / "web/scripts/copy-methodology.mjs")], cwd=root)
    key = state / "operator.key"
    if not key.exists():
        import secrets
        with key.open("x") as output:
            output.write(secrets.token_hex(32) + "\n")
        key.chmod(0o600)
    if key.is_symlink() or not key.is_file() or key.stat().st_mode & 0o077:
        raise RuntimeError("The local API operator key must be a private regular file; inspect .alight/quickstart/operator.key")
    free_ports()
    # Sim ignores provider/signing configuration; omit those values from child environments too.
    env = {name: value for name, value in os.environ.items() if not name.startswith(("ALIGHT_", "SOLAMI_"))}
    api_url = f"http://127.0.0.1:{args.api_port}"
    web_url = f"http://127.0.0.1:{args.web_port}"
    for name in ("api", "web"):
        logs.append((state / f"{name}.log").open("w"))
    api = spawn([str(binary), "--mode", "sim", "--seed", "42", "--sim-canaries", "900", "--sim-regimes", "true",
                 "--db", str(state / "sim.db"), "--bind", f"127.0.0.1:{args.api_port}", "--operator-key-file", str(key)],
                cwd=root, env=env, stdout=logs[0], stderr=subprocess.STDOUT)
    print("Starting seeded Sim API…", flush=True)
    wait_for_sim(api_url, api, "api")
    web = spawn(["node", str(vite), "--host", "127.0.0.1", "--port", str(args.web_port), "--strictPort"],
                cwd=root / "web", env={**env, "ALIGHT_API_ORIGIN": api_url}, stdout=logs[1], stderr=subprocess.STDOUT)
    wait_for_sim(web_url, web, "web")
    print(f"\nAlight is ready: {web_url}\nEvidence: SIMULATED · no provider traffic or SOL spend.\nOptional Operator access: open .alight/quickstart/operator.key locally.\n.env was not modified. Press Ctrl+C to stop both servers.\n", flush=True)
    deadline = time.monotonic() + args.run_for if args.run_for else None
    while deadline is None or time.monotonic() < deadline:
        if api.poll() is not None or web.poll() is not None:
            raise RuntimeError("A local server stopped; inspect .alight/quickstart/api.log and web.log")
        time.sleep(0.25)
except Stopped as stopped:
    exit_code = stopped.code
except (OSError, RuntimeError, subprocess.SubprocessError) as error:
    print(f"Quick start stopped: {error}", file=sys.stderr)
    exit_code = 1
finally:
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    for child in reversed(children):
        if child.poll() is None:
            try:
                os.killpg(child.pid, signal.SIGTERM)
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
            except ProcessLookupError:
                pass
    for output in logs:
        output.close()
sys.exit(exit_code)
PY
