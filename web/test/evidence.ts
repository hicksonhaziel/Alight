import assert from "node:assert/strict";
import { test } from "node:test";
import { sol, nominal, sendToSeenMs } from "../src/format.ts";
import { verifyBundle } from "../src/verification.ts";
import type { CanaryConfig, DecimalU64 } from "../../sdk/ts/src/types.ts";

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
