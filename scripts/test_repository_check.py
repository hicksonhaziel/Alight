#!/usr/bin/env python3
"""Public payer receipts are allowed; signing/provider credentials still fail."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

CHECKER = Path(__file__).with_name("check_repository.py")


class RepositoryCheck(unittest.TestCase):
    def test_public_payer_exception_is_narrow(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            shutil.copy(CHECKER, root / "scripts/check_repository.py")
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            (root / ".gitignore").write_text(".env\nALIGHT_PLAN.md\nALIGHT_BUILD.md\ncontext.txt\n")
            (root / ".env").write_text("ALIGHT_CANARY_PUBKEY=publicpayer123456789\nALIGHT_CANARY_KEYPAIR=privatekey123456789\nSOLAMI_API_KEY=providerkey123456789\nSOLAMI_RPC_URL=https://example.test/?api_key=urlsecret123456789\n")

            def check(value):
                (root / "receipt.json").write_text(value)
                result = subprocess.run(["python3", "scripts/check_repository.py"], cwd=root,
                                        capture_output=True, text=True, check=False)
                self.assertNotIn(value, result.stdout)
                return result.returncode

            self.assertEqual(check("publicpayer123456789"), 0)
            for value in ["privatekey123456789", "providerkey123456789", "urlsecret123456789",
                          "whsec_" + "abcdefghijklmnopqrstuvw"]:
                self.assertEqual(check(value), 1)


if __name__ == "__main__":
    unittest.main()
