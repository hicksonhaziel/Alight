#!/usr/bin/env python3
"""Verify recorded Phase 1 evidence offline; never reads ENV or contacts a provider."""
from hashlib import sha256
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    receipt = json.loads((ROOT / "data/phase-1-validation.json").read_text())
    assert receipt["phase_complete"] is False
    assert receipt["canaries_sent"] == 0
    assert receipt["health"]["signing_enabled"] is False
    assert receipt["health"]["status"] == "PASS"
    assert receipt["health"]["counts"]["budget_reserved_lamports"] == 0
    assert receipt["restart"]["preserved_and_increased"]
    assert receipt["restart"]["previous_run_interrupted"]
    assert receipt["restart"]["new_run_started"]
    assert receipt["rate_limit"]["passed"]
    assert receipt["grpc_connection_clocks"] >= 2
    for name, expected in receipt["fixtures"].items():
        assert sha256((ROOT / name).read_bytes()).hexdigest() == expected, name
    clock = json.loads((ROOT / "data/fixtures/clock_rpc_validation.json").read_text())
    assert abs(clock["grpc_mean_slot_ms"] - clock["rpc_mean_slot_ms"]) <= clock["tolerance_ms"]
    for name, absent in [("rpc_live_corroboration.json", False), ("rpc_absence_probe.json", True)]:
        fixture = json.loads((ROOT / "data/fixtures" / name).read_text())
        assert fixture["verdict"] == "PASS"
        assert fixture["canary_created"] is False
        assert fixture["canaries_sent"] == 0
        assert (fixture["rpc"]["landing"] is None) == absent
        canonical = json.dumps(fixture["captured_fields"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
        assert fixture["rpc"]["raw_ref"] == "sha256:" + sha256(canonical).hexdigest()
    print("PASS: Phase 1 fixture hashes, RPC proofs, measured clock, reconnect/restart evidence, and zero spend verified.")


if __name__ == "__main__":
    main()
