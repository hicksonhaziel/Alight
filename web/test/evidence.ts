import assert from "node:assert/strict";
import { test } from "node:test";
import {
  sol,
  nominal,
  sendToSeenMs,
  proveMemberDisplay,
} from "../src/format.ts";
import { verifyBundle } from "../src/verification.ts";
import type {
  CanaryConfig,
  DecimalU64,
  TrainingCanary,
} from "../../sdk/ts/src/types.ts";

test("Prove member timing distinguishes on-target, late, adverse and unresolved outcomes", () => {
  const sample = {
    finalized: true,
    canary: {
      outcome: "LANDED_OK",
      sent_slot: "9007199254740993000",
      landed_slot: "9007199254740993004",
      resolved_at_utc: "2026-10-12T15:17:00Z",
      clock_id: "send",
      send_mono_ns: "9007199254740993000",
      observer_first_seen: {},
    },
  } as TrainingCanary;
  const target = {
    kind: "probability",
    target_p: 0.6,
    horizon_slots: 4,
  } as const;
  const asOf = "2026-10-12T15:18:00Z";
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "met");
  sample.canary.landed_slot = "9007199254740993005" as DecimalU64;
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "late");
  sample.canary.outcome = "LANDED_FAILED";
  assert.match(proveMemberDisplay(sample, target, asOf).label, /LANDED_FAILED/);
  sample.canary.landed_slot = "9007199254740993004" as DecimalU64;
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "met");
  sample.canary.outcome = "EXPIRED";
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "adverse");
  sample.finalized = false;
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "pending");
  sample.finalized = true;
  assert.equal(
    proveMemberDisplay(sample, target, "2026-10-12T15:16:00Z").state,
    "pending",
  );
  sample.canary.outcome = "UNRESOLVED";
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "pending");
});

test("Prove millisecond targets require comparable clocks and retain boundary precision", () => {
  const sample = {
    finalized: true,
    canary: {
      outcome: "LANDED_OK",
      sent_slot: "10",
      landed_slot: "11",
      resolved_at_utc: "2026-10-12T15:17:00Z",
      clock_id: "send",
      send_mono_ns: "1000000",
      observer_first_seen: { grpc: { clock_id: "other", mono_ns: "2000000" } },
    },
  } as TrainingCanary;
  const target = {
    kind: "latency_quantile",
    quantile: 0.9,
    max_ms: 1,
  } as const;
  const asOf = "2026-10-12T15:18:00Z";
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "pending");
  sample.canary.observer_first_seen.grpc!.clock_id = "send";
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "met");
  sample.canary.observer_first_seen.grpc!.mono_ns = "2000001" as DecimalU64;
  assert.equal(proveMemberDisplay(sample, target, asOf).state, "late");
});

test("chain amounts retain full u64 precision and priority fees round upward", () => {
  assert.equal(sol("18446744073709551615", 9), "18446744073.709551615");
  assert.equal(sol("1", 9), "0.000000001");
  const config = {
    tip_lamports: "100000",
    cu_price_micro_lamports: "1001",
    cu_limit: 25000,
  } as CanaryConfig;
  assert.equal(nominal(config), 105026n);
});
test("latency never subtracts unrelated or backward clock origins", () => {
  const send = {
    clock_id: "send",
    send_mono_ns: "9007199254740993000" as DecimalU64,
  };
  assert.equal(
    sendToSeenMs(send, {
      clock_id: "other",
      mono_ns: "9007199254742993000" as DecimalU64,
    }),
    null,
  );
  assert.equal(
    sendToSeenMs(send, {
      clock_id: "send",
      mono_ns: "9007199254740992999" as DecimalU64,
    }),
    null,
  );
  assert.equal(
    sendToSeenMs(send, {
      clock_id: "send",
      mono_ns: "9007199254742993000" as DecimalU64,
    }),
    2,
  );
});
test("empty ledger verification checks source, exact sequence and declared genesis head", async () => {
  const bundle = {
    schema_version: 1,
    kind: "alight.browser-ledger.v1",
    source: "sim",
    head_sequence: "0",
    head_hash: `sha256:${"0".repeat(64)}`,
    rows: [],
  };
  assert.deepEqual(await verifyBundle(bundle, "sim"), bundle);
  await assert.rejects(verifyBundle(bundle, "live"));
  await assert.rejects(verifyBundle({ ...bundle, head_sequence: "00" }, "sim"));
  await assert.rejects(
    verifyBundle({ ...bundle, head_sequence: "18446744073709551616" }, "sim"),
  );
  await assert.rejects(
    verifyBundle({ ...bundle, head_hash: `sha256:${"1".repeat(64)}` }, "sim"),
  );
  await assert.rejects(verifyBundle({ ...bundle, rows: [{}] }, "sim"));
});
