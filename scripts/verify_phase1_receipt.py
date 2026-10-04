#!/usr/bin/env python3
"""Verify recorded Phase 1 evidence offline; never reads ENV or contacts a provider."""
from hashlib import sha256
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def verify_implementation():
    receipt = json.loads((ROOT / "data/phase-1-implementation-validation.json").read_text())
    assert receipt["phase_complete"] is False
    assert receipt["canaries_sent"] == 0
    assert receipt["blocked_acceptance"]
    assert receipt["paused_after_checks"] is True
    for name, expected in receipt["fixtures"].items():
        assert sha256((ROOT / name).read_bytes()).hexdigest() == expected, name
    hour = json.loads((ROOT / "data/fixtures/phase1_one_hour_observe.json").read_text())
    assert int(hour["health"]["uptime_s"]) >= 3600
    assert hour["health"]["mode"] == "observe"
    assert hour["health"]["status"] == "PASS"
    assert hour["health"]["signing_enabled"] is False
    assert hour["health"]["counts"]["budget_reserved_lamports"] == 0
    deploy = receipt["deployment"]
    assert deploy["health"]["collector_status"] == "PASS"
    assert deploy["health"]["mode"] == "live"
    assert deploy["health"]["canary_engine"]["status"] == "WAITING_FUNDS"
    assert deploy["health"]["canary_engine"]["balance_lamports"] == "0"
    assert deploy["health"]["counts"]["canaries"] == 0
    assert deploy["health"]["counts"]["budget_reserved_lamports"] == 0
    assert deploy["clock"]["mean_slot_ms"] > 0
    assert deploy["leaders"]["status"] == "PASS"
    assert deploy["healthcheck"] == "healthy"
    assert deploy["port_binding"] == "127.0.0.1:8080"
    assert deploy["image_revision"] == receipt["source_commit"][:7]
    assert deploy["legacy_user_service_disabled"]
    assert deploy["private_consistent_backup"]
    restart = receipt["container_restart"]
    assert restart["previous_run_status"] == "interrupted"
    assert restart["before"]["run_id"] != restart["after"]["run_id"]
    assert restart["after"]["status"] == "PASS"
    assert restart["automatic_restarts"] >= 1
    for table in ("slot_events", "blocks", "observations"):
        assert restart["after"]["counts"][table] >= restart["before"]["counts"][table]
    for snapshot in (restart["before"], restart["after"]):
        assert snapshot["counts"]["budget_reserved_lamports"] == 0
        assert snapshot["counts"]["canaries"] == 0
    redaction = receipt["log_redaction"]
    assert redaction["synthetic_marker_absent"]
    assert redaction["request_fields_removed"]
    assert redaction["health_request_status"] == 200
    assert redaction["unlisted_path_status"] == 404
    assert redaction["write_method_status"] == 405
    assert redaction["upstream_error_status"] == 502
    plan = json.loads((ROOT / "data/fixtures/phase1_retention_query_plan.json").read_text())
    assert plan["result"] == "PASS"
    assert all("SCAN" not in detail for detail in plan["plan"])


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
    assert receipt["local_service"]["automatic_crash_recovery_passed"]
    assert receipt["local_service"]["health"]["signing_enabled"] is False
    assert receipt["local_service"]["health"]["canaries_sent"] == 0
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
    verify_implementation()
    print("PASS: Phase 1 recorded proofs, fixture hashes, one-hour observe run, local live deployment, restart recovery, log redaction, and zero spend verified; funded acceptance remains open.")


if __name__ == "__main__":
    main()
