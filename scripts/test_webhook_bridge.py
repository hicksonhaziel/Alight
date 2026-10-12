#!/usr/bin/env python3
"""Offline boundary tests: tunnel ingress cannot expose API/operator routes."""
import hashlib
import hmac
import json
from pathlib import Path
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.request import Request, urlopen
import webhook_bridge as bridge


class Ingress(unittest.TestCase):
    def test_exact_hmac_body_limits_and_route_isolation(self):
        calls = []

        class Backend(BaseHTTPRequestHandler):
            def do_POST(self):
                calls.append((self.path, self.rfile.read(int(self.headers["Content-Length"])),
                              self.headers["X-Webhook-Signature"]))
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b'{"source":"live","status":"IGNORED_UNOWNED"}')

            def log_message(self, *args):
                pass

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".env").write_text("SOLAMI_WEBHOOK_SECRET=offline-test-only\n")
            backend = HTTPServer(("127.0.0.1", 0), Backend)
            instance = bridge.Bridge(root, root / "status.json", "/hook/test",
                                    f"http://127.0.0.1:{backend.server_port}/v1/webhook")
            server = HTTPServer(("127.0.0.1", 0), instance.handler())
            threads = [threading.Thread(target=s.serve_forever, daemon=True) for s in [backend, server]]
            for thread in threads:
                thread.start()

            def send(path, body=None, signature=None):
                headers = {"X-Webhook-Signature": signature} if signature else {}
                request = Request(f"http://127.0.0.1:{server.server_port}" + path, data=body, headers=headers)
                try:
                    with urlopen(request, timeout=5) as response:
                        return response.status
                except HTTPError as error:
                    code = error.code
                    error.close()
                    return code

            try:
                body = b'{ "signature": "offline", "slot": 1, "status": "succeeded" }'
                signature = hmac.new(b"offline-test-only", body, hashlib.sha256).hexdigest()
                self.assertEqual(send("/v1/health"), 404)
                self.assertEqual(send("/v1/prove", body, signature), 404)
                self.assertEqual(send("/hook/test?bypass=1", body, signature), 404)
                self.assertEqual(send("/hook/test", body, "0" * 64), 401)
                self.assertEqual(send("/hook/test", body + b" ", signature), 401)
                self.assertEqual(send("/hook/test", b"x" * 65537, signature), 413)
                self.assertEqual(calls, [])
                self.assertEqual(send("/hook/test", body, signature), 200)
                self.assertEqual(calls, [("/v1/webhook", body, signature)])
                state = json.loads((root / "status.json").read_text())
                self.assertEqual(state["deliveries"][0]["backend_status"], "IGNORED_UNOWNED")
                self.assertNotIn("offline-test-only", (root / "status.json").read_text())
                self.assertEqual((root / "status.json").stat().st_mode & 0o777, 0o600)
                restarted = bridge.Bridge(root, root / "status.json", "/hook/test")
                self.assertEqual(restarted.state["deliveries"], state["deliveries"])
                self.assertFalse(restarted.state["configured"])
                instance.delivery(b'{"signature":123,"slot":{"private":"never log this"}}', 400, "BACKEND_REJECTED")
                self.assertIsNone(instance.state["deliveries"][-1]["slot"])
                self.assertNotIn("never log this", (root / "status.json").read_text())
            finally:
                for s in [server, backend]:
                    s.shutdown()
                    s.server_close()
                for thread in threads:
                    thread.join()

    def test_restart_recovers_existing_registration_and_keeps_other_env(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".env").write_text("ALIGHT_CANARY_PUBKEY=public-test-address\nSOLAMI_API_KEY=offline-key\nALIGHT_MODE=observe\n")
            record = {"id": "existing", "label": bridge.LABEL, "addresses": ["public-test-address"],
                      "secret": "offline-signing-secret", "enabled": True, "payload_kind": "enriched"}
            operations = []

            def provider(values, operation, body):
                operations.append(operation)
                return [record] if operation == "list" else {**record, "url": body["url"]}

            with patch.object(bridge, "provider", provider):
                bridge.configure(root, "https://example.test/hook/test")
                bridge.configure(root, "https://restarted.example.test/hook/test")
            self.assertEqual(operations, ["list", "update", "list", "update"])
            values = bridge.environment(root)
            self.assertEqual(values["ALIGHT_MODE"], "observe")
            self.assertEqual(values["SOLAMI_WEBHOOK_ID"], "existing")
            self.assertEqual(values["ALIGHT_WEBHOOK_CALLBACK_URL"], "https://restarted.example.test/hook/test")
            self.assertEqual((root / ".env").stat().st_mode & 0o777, 0o600)


if __name__ == "__main__":
    unittest.main()
