#!/usr/bin/env python3
"""Temporary bounded HTTP receiver. Serves no files and never sends transactions."""
import base64
import datetime as dt
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import secrets
import shlex
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
PROBES = ROOT / ".alight/probes/webhook"
MAX_BYTES = 512 * 1024


def environment():
    values = {}
    for line in (ROOT / ".env").read_text().splitlines():
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            fields = shlex.split(value, comments=True)
            values[key] = fields[0] if fields else ""
    values.update({k: v for k, v in os.environ.items() if k.startswith(("ALIGHT_", "SOLAMI_"))})
    return values


def verify_signature(body, secret, signature):
    """Verify the exact-body SHA-256 hex format observed on real Solami deliveries."""
    if not secret or len(signature) != 64:
        return None
    try:
        received = signature.encode("ascii")
    except UnicodeEncodeError:
        return None
    expected = hmac.new(secret.encode(), body, hashlib.sha256).hexdigest().encode("ascii")
    return "hex" if hmac.compare_digest(expected, received) else None


def scrub(value, values):
    if isinstance(value, str):
        for key, secret in values.items():
            if len(secret) >= 8 and any(x in key for x in ("TOKEN", "KEY", "SECRET", "CALLBACK_URL")):
                value = value.replace(secret, "[REDACTED]")
        return value
    if isinstance(value, list):
        return [scrub(v, values) for v in value]
    if isinstance(value, dict):
        return {k: "[REDACTED]" if k.lower() in {"secret", "token", "api_key", "authorization"}
                else scrub(v, values) for k, v in value.items()}
    return value


def run():
    PROBES.mkdir(parents=True, exist_ok=True, mode=0o700)
    config_path = PROBES / "receiver.json"
    config = json.loads(config_path.read_text()) if config_path.exists() else {
        "path": "/hook/" + secrets.token_hex(16), "port": 8787, "max_deliveries": 6}
    (PROBES / "receiver.json").write_text(json.dumps(config))
    (PROBES / "receiver.json").chmod(0o600)
    (PROBES / "receiver.pid").write_text(str(os.getpid()))
    deliveries = []
    attempts = 0
    start = time.monotonic_ns()

    class Receiver(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def respond(self, status):
            self.send_response(status)
            self.send_header("Content-Length", "0")
            self.end_headers()

        def do_GET(self):
            self.respond(200 if self.path == "/health" else 404)

        def do_POST(self):
            nonlocal attempts
            if self.path != config["path"]:
                self.respond(404)
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
            except ValueError:
                self.respond(400)
                return
            if not 0 < length <= MAX_BYTES or attempts >= 20 or len(deliveries) >= config["max_deliveries"]:
                self.respond(413)
                return
            self.connection.settimeout(5)
            body = self.rfile.read(length)
            if len(body) != length:
                self.respond(400)
                return
            attempts += 1
            values = environment()
            signature = self.headers.get("X-Webhook-Signature", "")
            encoding = verify_signature(body, values.get("SOLAMI_WEBHOOK_SECRET", ""), signature)
            received = dt.datetime.now(dt.timezone.utc).isoformat()
            raw = {"body_base64": base64.b64encode(body).decode(),
                   "signature": signature, "received_at": received,
                   "header_names": list(self.headers.keys())}
            raw_path = PROBES / f"attempt-{attempts}.json"
            raw_path.write_text(json.dumps(raw))
            raw_path.chmod(0o600)
            if not encoding:
                self.respond(503 if not values.get("SOLAMI_WEBHOOK_SECRET") else 401)
                print(json.dumps({"attempt": attempts, "verified": False,
                                  "signature_present": bool(signature)}), flush=True)
                return
            try:
                parsed = json.loads(body)
            except ValueError:
                self.respond(400)
                return
            deliveries.append({"source": "live", "observer": "webhook", "received_at": received,
                               "recv_mono_ns": str(time.monotonic_ns() - start),
                               "body_sha256": hashlib.sha256(body).hexdigest(),
                               "signature_verified": True, "signature_encoding": encoding,
                               "data": scrub(parsed, values)})
            fixture = ROOT / "data/fixtures/webhook_sample.json"
            fixture.parent.mkdir(parents=True, exist_ok=True)
            fixture.write_text(json.dumps({"schema_version": 1, "source": "live",
                                          "capture_scope": "payer_and_public_control_account",
                                          "canaries_sent": 0, "deliveries": deliveries}, indent=2) + "\n")
            self.respond(200)
            print(json.dumps({"verified_deliveries": len(deliveries), "signature_encoding": encoding}), flush=True)

    server = HTTPServer(("127.0.0.1", config["port"]), Receiver)
    timer = threading.Timer(600, server.shutdown)
    timer.daemon = True
    timer.start()
    print("Temporary receiver listening on loopback port 8787; shuts down within 10 minutes.", flush=True)
    try:
        server.serve_forever()
    finally:
        server.server_close()


if __name__ == "__main__":
    run()
