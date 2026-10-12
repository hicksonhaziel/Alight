#!/usr/bin/env python3
"""Expose only HMAC-authenticated Webhook ingress through a managed quick tunnel.

No signer, transaction sender, provider payload log, or public API proxy. The
provider callback is updated when the tunnel hostname changes after a restart.
Local .env and runtime state remain private. A named tunnel is preferable for
production availability; quick tunnels have no availability guarantee.
"""
import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import shutil
import signal
import subprocess
import threading
import time
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.error import HTTPError
from urllib.request import Request, urlopen

LABEL = "Alight canary observer"
MAX_BODY = 65536


def now():
    return datetime.now(timezone.utc).isoformat()


def environment(root):
    values = {}
    for line in (root / ".env").read_text().splitlines():
        parts = shlex.split(line, comments=True)
        if parts and parts[0] == "export":
            parts = parts[1:]
        if len(parts) == 1 and "=" in parts[0]:
            key, value = parts[0].split("=", 1)
            values[key] = value
    return values


def private_write(path, data):
    temp = path.with_name(path.name + ".tmp")
    with os.fdopen(os.open(temp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "w") as file:
        os.fchmod(file.fileno(), 0o600)
        file.write(data)
        file.flush()
        os.fsync(file.fileno())
    os.replace(temp, path)
    path.chmod(0o600)


def update_environment(root, updates):
    path = root / ".env"
    lines = path.read_text().splitlines()
    for key, value in updates.items():
        lines = [line for line in lines if not re.match(r"^(?:export\s+)?" + re.escape(key) + "=", line)]
        lines.append(key + "=" + shlex.quote(value))
    private_write(path, "\n".join(lines) + "\n")


def provider(values, operation, body):
    request = Request("https://api.solami.dev/webhooks/" + operation,
                      data=json.dumps(body).encode(),
                      headers={"x-api-key": values["SOLAMI_API_KEY"], "Content-Type": "application/json"})
    with urlopen(request, timeout=15) as response:
        raw = response.read(262145)
    if len(raw) > 262144:
        raise ValueError("provider response exceeds bounds")
    return json.loads(raw)


def configure(root, callback):
    values = environment(root)
    payer = values["ALIGHT_CANARY_PUBKEY"]
    records = provider(values, "list", {})
    owned = [row for row in records if row["id"] == values.get("SOLAMI_WEBHOOK_ID")]
    if not owned:
        owned = [row for row in records if row.get("label") == LABEL and row.get("addresses") == [payer]]
    if len(owned) > 1:
        raise ValueError("ambiguous observer registration")
    fields = {"label": LABEL, "url": callback, "addresses": [payer],
              "event_types": ["memo", "transfer"], "payload_kind": "enriched"}
    if owned:
        record = provider(values, "update", {"id": owned[0]["id"], **fields, "enabled": True})
    else:
        # Never retry this mutation. After an uncertain result, a restart lists
        # existing records first and recovers the matching registration.
        record = provider(values, "create", {**fields, "region": "fra", "stream": False})
    if (not record.get("enabled") or record.get("addresses") != [payer]
            or record.get("url") != callback or record.get("payload_kind") != "enriched"):
        raise ValueError("provider registration did not match")
    secret = record.get("secret") or values.get("SOLAMI_WEBHOOK_SECRET")
    if not secret:
        raise ValueError("provider signing secret unavailable")
    update_environment(root, {"SOLAMI_WEBHOOK_ID": record["id"],
                             "SOLAMI_WEBHOOK_SECRET": secret,
                             "ALIGHT_WEBHOOK_CALLBACK_URL": callback})


class Bridge:
    def __init__(self, root, state_path, hook_path, backend="http://127.0.0.1:8080/v1/webhook"):
        self.root, self.state_path, self.hook_path, self.backend = root, state_path, hook_path, backend
        self.lock = threading.Lock()
        try:
            retained = json.loads(state_path.read_text())["deliveries"][-200:]
        except (FileNotFoundError, ValueError, KeyError, TypeError):
            retained = []
        if not isinstance(retained, list):
            retained = []
        self.state = {"started_at_utc": now(), "configured": False, "deliveries": retained}
        self.requests = []

    def record(self, **changes):
        with self.lock:
            self.state.update(changes)
            private_write(self.state_path, json.dumps(self.state, indent=2) + "\n")

    def delivery(self, body, code, status):
        try:
            payload = json.loads(body)
            signature = payload.get("signature", "")
            slot = payload.get("slot")
        except (ValueError, AttributeError):
            signature, slot = "", None
        if not isinstance(signature, str) or len(signature) > 90:
            signature = ""
        if type(slot) is not int or not 0 <= slot <= 18446744073709551615:
            slot = None
        with self.lock:
            self.state["deliveries"] = (self.state["deliveries"] + [{
                "at_utc": now(), "hmac_verified": True, "http_status": code,
                "backend_status": status, "slot": slot,
                "signature_sha256": hashlib.sha256(signature.encode()).hexdigest(),
                "body_sha256": hashlib.sha256(body).hexdigest(), "body_bytes": len(body),
            }])[-200:]
            private_write(self.state_path, json.dumps(self.state, indent=2) + "\n")

    def handler(self):
        bridge = self

        class Handler(BaseHTTPRequestHandler):
            def setup(self):
                super().setup()
                self.connection.settimeout(3)

            def log_message(self, *args):
                pass

            def reply(self, code, body=b"{}"):
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self):
                self.reply(404)

            def do_POST(self):
                if self.path != bridge.hook_path:
                    self.reply(404)
                    return
                cutoff = time.monotonic() - 60
                bridge.requests = [stamp for stamp in bridge.requests if stamp > cutoff]
                if len(bridge.requests) >= 60:
                    self.reply(429)
                    return
                bridge.requests.append(time.monotonic())
                length = self.headers.get("Content-Length", "")
                if self.headers.get("Transfer-Encoding") or not length.isdecimal():
                    self.reply(411)
                    return
                size = int(length)
                if not 0 < size <= MAX_BODY:
                    self.reply(413)
                    return
                body = self.rfile.read(size)
                signature = self.headers.get("X-Webhook-Signature", "")
                secret = environment(bridge.root).get("SOLAMI_WEBHOOK_SECRET", "")
                if (len(body) != size or not secret or not re.fullmatch("[0-9a-f]{64}", signature)
                        or not hmac.compare_digest(signature, hmac.new(secret.encode(), body, hashlib.sha256).hexdigest())):
                    self.reply(401)
                    return
                request = Request(bridge.backend, data=body, headers={
                    "Content-Type": "application/json", "X-Webhook-Signature": signature})
                try:
                    with urlopen(request, timeout=7) as response:
                        code, raw = response.status, response.read(4096)
                    status = json.loads(raw).get("status", "UNKNOWN")
                except HTTPError as error:
                    code, status = error.code, "BACKEND_REJECTED"
                    error.close()
                except Exception:
                    code, status = 503, "BACKEND_UNAVAILABLE"
                bridge.delivery(body, code, status)
                self.reply(code, json.dumps({"status": status}).encode())

        return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--port", type=int, default=8787)
    args = parser.parse_args()
    root = args.root.resolve()
    runtime = root / ".alight/webhook"
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    values = environment(root)
    hook_path = values.get("ALIGHT_WEBHOOK_PATH") or "/hook/" + secrets.token_hex(24)
    if not re.fullmatch(r"/hook/[0-9a-f]{48}", hook_path):
        raise ValueError("invalid callback path")
    update_environment(root, {"ALIGHT_WEBHOOK_PATH": hook_path})
    bridge = Bridge(root, runtime / "status.json", hook_path)
    bridge.record()
    server = HTTPServer(("127.0.0.1", args.port), bridge.handler())
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    executable = shutil.which("cloudflared") or str(Path.home() / ".local/bin/cloudflared")
    process = subprocess.Popen([executable, "tunnel", "--no-autoupdate", "--url",
                                "http://127.0.0.1:" + str(args.port)],
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        for line in process.stderr:
            match = re.search(r"https://[a-z0-9-]+\.trycloudflare\.com", line)
            if match and not bridge.state["configured"]:
                configure(root, match[0] + hook_path)
                bridge.record(configured=True, configured_at_utc=now())
                print("Webhook callback configured; ingress only; values withheld.", flush=True)
        raise RuntimeError("tunnel exited")
    finally:
        process.terminate()
        process.wait(timeout=10)
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
    except Exception as error:
        # Exception messages may contain authenticated URLs. Print type only.
        print("Webhook bridge stopped: " + type(error).__name__, flush=True)
        raise SystemExit(1)
