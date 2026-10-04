#!/usr/bin/env python3
"""Verify committed capture hashes and keep unfunded evidence claims honest."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    receipt = json.loads((ROOT / "data/phase-0-receipt.json").read_text())
    for item in receipt["fixtures"]:
        path = (ROOT / item["path"]).resolve()
        if ROOT not in path.parents:
            raise ValueError("fixture path outside repository")
        body = path.read_bytes()
        if len(body) != item["bytes"] or hashlib.sha256(body).hexdigest() != item["sha256"]:
            raise ValueError(f"fixture hash/size mismatch: {item['path']}")
    if receipt["canaries_sent"] == 0:
        if receipt["canaries_signed"] != 0 or receipt["spend_lamports"] != 0:
            raise ValueError("zero-send receipt has inconsistent spend/signing")
        if receipt["gates"]["G0"].startswith("PASS"):
            raise ValueError("connection-only evidence cannot pass G0")
    webhook = json.loads((ROOT / "data/fixtures/webhook_sample.json").read_text())
    deliveries = webhook["deliveries"]
    if not deliveries or not all(d["signature_verified"] for d in deliveries):
        raise ValueError("publish only verified webhook deliveries")
    if len({d["data"]["signature"] for d in deliveries}) != receipt["webhook"]["unique_signatures"]:
        raise ValueError("webhook signature count mismatch")
    print(f"PASS: {len(receipt['fixtures'])} capture hashes; verified deliveries; unfunded G0 remains pending.")


if __name__ == "__main__":
    main()
