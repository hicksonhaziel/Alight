#!/usr/bin/env python3
"""Corroborate recorded public signatures with bounded read-only mainnet RPC."""
from datetime import datetime, timezone
from pathlib import Path
import json
import time
import urllib.request
import urllib.parse

from spike_blur import environment


def main():
    env = environment()
    url = env.get("SOLAMI_RPC_URL", "")
    token = env.get("SOLAMI_RPC_TOKEN", "")
    if urllib.parse.urlsplit(url).scheme != "https":
        print("RPC corroboration: HTTPS configuration required; values withheld.")
        return 2
    headers = {"Content-Type": "application/json"}
    if token:
        headers["x-api-key"] = token
    fixtures = Path("data/fixtures")
    transactions = [r for r in map(json.loads, (fixtures / "grpc_transactions_sample.jsonl")
                                  .read_text().splitlines()) if r.get("kind") == "transaction"][:3]
    selected_slots = sorted({int(r["slot"]) for r in transactions})[:2]
    transactions = [r for r in transactions if int(r["slot"]) in selected_slots]
    hooks = json.loads((fixtures / "webhook_sample.json").read_text())["deliveries"]
    # Public chain signatures only; never construct or serialize a transaction.
    signatures = list(dict.fromkeys([r["signature"] for r in transactions]
                                   + [r["data"]["signature"] for r in hooks]))[:9]
    report = {"schema_version": 1, "source": "live", "canaries_sent": 0,
              "requested_at": datetime.now(timezone.utc).isoformat(), "checks": []}

    def rpc(method, params):
        # This function is deliberately restricted to two observation methods.
        if method not in {"getSignatureStatuses", "getBlock"}:
            raise ValueError("read-only method required")
        started = time.monotonic()
        request = urllib.request.Request(url, headers=headers, data=json.dumps({
            "jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode())
        record = {"method": method, "verdict": "FAIL"}
        try:
            with urllib.request.urlopen(request, timeout=12) as response:
                body = response.read(2 * 1024 * 1024 + 1)
            if len(body) > 2 * 1024 * 1024:
                raise ValueError("byte cap")
            parsed = json.loads(body)
            if parsed.get("error"):
                # Provider messages may echo URLs or tokens; retain only the error code.
                record["rpc_error_code"] = parsed["error"].get("code")
                result = None
            else:
                result = parsed.get("result")
                record["verdict"] = "PASS" if result is not None else "MISSING"
        except Exception as error:
            record["error_category"] = type(error).__name__
            result = None
        record["elapsed_ms"] = round((time.monotonic() - started) * 1000)
        report["checks"].append(record)
        return result, record

    statuses, record = rpc("getSignatureStatuses", [signatures, {"searchTransactionHistory": True}])
    record["observations"] = [] if not statuses else [
        {"signature": signature, "status": status}
        for signature, status in zip(signatures, statuses.get("value", []))]
    matched = 0
    for slot in selected_slots:
        block, record = rpc("getBlock", [slot, {"commitment": "finalized",
            "transactionDetails": "signatures", "rewards": False, "maxSupportedTransactionVersion": 0}])
        record["slot"] = str(slot)
        record["matches"] = []
        if not block:
            continue
        record["block_id"] = block["blockhash"]
        record["signature_count"] = len(block.get("signatures", []))
        record["parent_slot"] = str(block["parentSlot"])
        positions = {signature: index for index, signature in enumerate(block.get("signatures", []))}
        for observation in (r for r in transactions if int(r["slot"]) == slot):
            actual = positions.get(observation["signature"])
            agrees = actual is not None and actual == int(observation["index"])
            matched += agrees
            record["matches"].append({"signature": observation["signature"],
                "grpc_index": str(observation["index"]), "rpc_index": actual, "agrees": agrees})
    report["grpc_indices_corroborated"] = matched
    report["verdict"] = "PASS" if matched == len(transactions) and transactions and statuses and len(
        statuses.get("value", [])) == len(signatures) and all(
        s and s.get("confirmationStatus") == "finalized" for s in statuses.get("value", [])) else "INCONCLUSIVE"
    path = Path("data/rpc-observer-validation.json")
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"verdict": report["verdict"], "grpc_indices_corroborated": matched,
        "signatures_checked": len(signatures), "canaries_sent": 0, "evidence": str(path)}))
    return 0 if report["verdict"] == "PASS" else 3


if __name__ == "__main__":
    raise SystemExit(main())
