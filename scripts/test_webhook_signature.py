import hashlib
import hmac
import unittest
from spike_webhook import verify_signature


class SignatureTests(unittest.TestCase):
    def test_rejects_changed_body_and_missing_credentials(self):
        body = b'{"slot":123}'
        secret = "test-only-verifier-secret"
        signature = hmac.new(secret.encode(), body, hashlib.sha256).hexdigest()
        self.assertEqual(verify_signature(body, secret, signature), "hex")
        self.assertIsNone(verify_signature(body + b" ", secret, signature))
        self.assertIsNone(verify_signature(body, "other-test-secret", signature))
        self.assertIsNone(verify_signature(body, "", signature))
        self.assertIsNone(verify_signature(body, secret, ""))
        self.assertIsNone(verify_signature(body, secret, "é" * 64))


if __name__ == "__main__":
    unittest.main()
