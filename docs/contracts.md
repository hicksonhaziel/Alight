# Alight contracts v1

The shared Rust types in `crates/alight-types/src/contracts.rs` freeze the
Phase 0 wire shapes. `CONTRACT_VERSION = 1`. They describe the future collector
and API; this scaffold does not yet serve quotes or persist canaries.
Call `validate()` at API boundaries; serde decoding alone is not validation.

| Enum | JSON values |
| --- | --- |
| Source | `live`, `sim`, `replay` |
| Route | `beam_quic`, `beam_http`, `rpc` |
| Outcome | `LANDED_OK`, `LANDED_FAILED`, `LANDED_THEN_DROPPED`, `EXPIRED`, `REJECTED`, `UNRESOLVED` |
| Evidence | `MEASURED`, `INTERPOLATED`, `EXTRAPOLATED`, `INSUFFICIENT` |
| ForecastStatus | `PENDING`, `SCORED`, `VOIDED` |
| ObserverKind | `grpc`, `mirage`, `rpc`, `webhook` |
| SizeClass | `small`, `medium`, `large` |
| SignalVerdict | `DISCRIMINATING`, `FLAT`, `INCONCLUSIVE` |
| FeeBucket | `zero`, `local_median`, `local_p90` |
| TipTier | `none`, `x1`, `x2`, `x5`, `x10` |
| SlotStatus | `FIRST_SHRED`, `COMPLETED`, `CREATED_BANK`, `PROCESSED`, `CONFIRMED`, `FINALIZED`, `DEAD` |
| Tercile | `low`, `middle`, `high`, `unknown` |
| IndexScope | `provider_reported`, `rpc_block_list`, `unknown` |

Unsigned 64-bit fields are **decimal JSON strings**: slots, block heights,
lamports, micro-lamports per CU, and monotonic nanoseconds. Smaller u32 indexes,
CU limits, and horizons are numbers. USD values are decimal strings.
Provider envelopes can use other representations; adapters normalize from exact
bytes and preserve raw evidence. JS parsed previews can round large amounts.
Probability, age in seconds, and milliseconds are finite f64 values.

`CanaryConfig` records route, integer tip/fee, CU limit, fee bucket, tip tier,
and size class. Example shape, **simulated configuration, not a measured send**:

```json
{"route":"beam_quic","tip_lamports":"100000","cu_price_micro_lamports":"0","cu_limit":200000,"fee_bucket":"zero","tip_tier":"x1","size_class":"small"}
```

`Canary` includes id, source, config, policy id, assignment probability,
uniform-arm flag, regime id, sent slot, clock id, monotonic/UTC send time,
optional signature, blockhash, last valid block height, next-leader classes,
optional outcome/landed slot/block identity/index/index scope, observer first-seen map,
and optional UTC resolution time. Only the future governed canary engine may
produce signed attempts. Expiry requires **block height strictly greater than**
the last valid block height and signature absence at the required commitment.
A timeout cannot establish an outcome.

`ReceiveTime` has `clock_id`, `mono_ns` (decimal ns string), and `wall_utc`.
Monotonic times are comparable only within the same clock id. Restarts create
new clock ids; UTC differences across machines carry clock-skew uncertainty.
`observer_first_seen` maps each observer to a complete `ReceiveTime`.

`SlotEvent` has slot, optional block id, status, received time, optional leader,
source, and raw reference. `ObserverEvent` has observer, signature, optional
slot/block id/index/success, index scope, received time, raw reference, and source.
`ObserverGap` is separate: observer, source, start/end UTC, reason. Gaps never
fabricate signatures or outcomes. A candidate landing uses `(slot, block_id)`;
slot-only records cannot establish a unique fork identity.

Illustrative normalized observer shape, **simulated values**:

```json
{"observer":"grpc","signature":"example-signature","slot":"453096590","block_id":null,"index_in_block":205,"index_scope":"provider_reported","success":true,"received":{"clock_id":"example-process-id","mono_ns":"174197874","wall_utc":"2026-10-04T00:36:18.448Z"},"raw_ref":"sha256:example","source":"sim"}
```

`QuoteRequest` requires nonempty routes, finite target probability in (0,1], size
class, and exactly one positive horizon: `within_ms` or `within_slots`.
Optional pool, USD size, edge bps, and lambda provide economics context. Lambda
must be finite and nonnegative. A recommendation records config, region,
observation window, probability/95% interval, optional p50/p90/p99 latency in ms,
positive effective sample count, evidence, and nonnegative age in seconds.
Validation checks interval ordering, finite numbers, route/size agreement and
quantile ordering. Evidence classification/calibration require later models.

`QuoteResponse` carries version, source, quote id, echoed request, current regime
(id, measured slot ms, since UTC), evidence, recommendation or samples needed,
optional economics, and ledger hashes. `INSUFFICIENT` **omits recommendation**,
requires positive `samples_needed`, and prohibits economics. Example shape,
**simulation only, not a live forecast or computed ledger**:

```json
{
  "contract_version": 1, "source": "sim", "quote_id": "example",
  "request": {"route_set":["rpc"],"target_p":0.9,"within_ms":750,"within_slots":null,"size_class":"small","pool":null,"size_usd":null,"edge_bps":null,"lambda":null},
  "regime": {"id":"example","slot_ms":266.0,"since":"2026-10-04T00:00:00Z"},
  "evidence": "INSUFFICIENT", "samples_needed": 30, "economics": null,
  "ledger": {"hash":"example-hash","prev_hash":"example-previous-hash"}
}
```

Economics records conditional expected USD cost, explicit assumptions, age, and
staleness. Shape validation does not prove public keys, ledger integrity,
calibration or honest evidence: later adapters, models and resolver enforce them.

## Adjustments established by real payloads

- Yellowstone enum zero is `SLOT_PROCESSED`; protobuf JSON may omit that default.
  gRPC fixtures explicitly retain status name/code. Mirage preserves provider
  protobuf JSON; its adapter must restore defaults after decoding.
- gRPC transactions have slot, signature and index but no landing block identity.
  RPC confirmed nine public signatures finalized and sampled block identities
  matched gRPC metadata, but two indexes differed: 205 vs 167, and 887 vs 401.
  The latter RPC full-block list also placed the signature at 401. Keep provider
  indexes and RPC list positions in separate scopes; do not compare them or use
  them as a canonical position until their semantics are resolved. Joining
  metadata must retain provenance and alternative candidate identities.
- Current full-block RPC requests with max transaction version 0 returned
  -32015, requiring version 1. A retry with version 1 decoded a 598-transaction
  block (157 version-1 transactions); another retry failed at the network layer.
  Future full-block adapters must support version 1 and preserve decode errors.
- Mirage uses binary Yellowstone `SubscribeUpdate` on a saved subscription
  WebSocket. Byte fields become base64 in protobuf JSON; decode signature bytes
  to base58 when normalizing. Ping/control frames are not transactions.
  Long-run heartbeat cadence and reconnect gaps remain unmeasured.
- HTTP webhook `X-Webhook-Signature` is lowercase hex HMAC-SHA256 over **exact
  request bytes**, keyed by the literal issued secret. The fixture contains only
  verified deliveries and body hashes. Raw bytes/signatures remain local.
  Valid deliveries can repeat; adapters need idempotent storage.
- Webhook `received_at` is provider Unix seconds; outer UTC/monotonic fields are
  local. Their subtraction includes queue/retry, rounding and clock skew, and
  does not measure chain landing latency.
- Webhook WebSocket uses an enriched JSON envelope without the HTTP HMAC header.
  Its TLS connection/API authentication was tested. Preserve exact `raw_json`.
- Blur arrays contain decimal strings and large integer amounts. Exact REST
  Python integers and WebSocket `raw_json` are authoritative for normalization.

Actual payloads are in `data/fixtures/`, with hashes in the Phase 0 receipt.
Files marked live are historical captures, not active streams; a future replay
adapter emits `source=replay` while retaining original provenance.

## Migration from the local brief

No database or public API is deployed, so no data migration exists yet.
Compared with the starting shapes, `recv_mono_ns` becomes nested `received`,
`observer_first_seen_ns` becomes `observer_first_seen`, and every clock has an
identity/UTC reference. Quotes explicitly include size class; 64-bit wire numbers
become strings. Index scope is mandatory on observations and retained on resolved
canaries. Phase 1 storage must persist these fields and separate unknown
block slot events from keyed candidate blocks; the brief's nullable composite
primary key is not a completed persistence design.

## Phase 1 persistence additions (4 October)

`BlockMetaEvent` retains the candidate block id, parent identity, chain timestamp
in Unix seconds, optional height/transaction count, receive clock, source and raw
reference. `IngestEvent` tags slot, observation and block metadata records.
`RunMode` distinguishes observe/live/sim/replay. `BudgetLimits` and
`BudgetReservation` use exact lamport/ms strings and record route/source/day.
These are additive shared contracts; the existing v1 shapes stay compatible.

Migration `crates/alight-store/migrations/0001_observe.sql` adds WAL persistence.
Known-block and unknown-block keys are explicit non-null strings, so SQLite's
NULL uniqueness rules cannot collapse or duplicate forks. Slot columns use
20-digit zero padding internally for lossless numeric ordering; wire JSON still
uses ordinary decimal strings. First receive times are retained on duplicates.
Evidence, events and receive cursors commit atomically. Evidence hashes address
captured provider-field JSON subsets, not original protobuf bytes; wire digests
can be retained inside that subset. The store rejects mismatched references.
Restart gaps are recorded from previous observer receive times. A missing or
ambiguous block identity stays unknown; raw observations are never overwritten
when a unique same-observer candidate is used during resolution.
