# Alight contracts v1

The shared Rust types in `crates/alight-types/src/contracts.rs` freeze the
Phase 0 wire shapes. `CONTRACT_VERSION = 1`. The collector persists events and canaries; quote APIs are still planned.
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

## Phase 3 economics contracts (tasks 3.1–3.3)

`MarketTrade` normalizes the captured Blur REST trades and authoritative WS
`raw_json`: pool/base mint, source, slot, Unix-second block time, provider-scoped
transaction/instruction indices (including signed `inner_ix_index: -1`), USD
price text and exact token quantities.
REST mint identity comes from the requested pool metadata; WS supplies it.
Replay always changes the source to `replay`. Connected/control frames cannot
refresh pool data. Candle prices remain separate; minute candles never supply
sub-second delay evidence. No database migration is required for this in-memory
slice; persisted economics/quote integration follows in task 3.5.

`DelayCostSnapshot` reports the rolling window, measured slot duration in ms,
trade/slot counts, market age, stale/sparse flags, and points with median/p90
absolute USD-price returns in bps. Pairs require an exact observed slot distance;
delay_ms is that distance times the supplied measured slot duration. This is a
clock conversion, not an exact sub-second trade timestamp. Disconnections split
the series; pairs cannot cross a gap. Split-half relative median change is a
descriptive stability metric, not a confidence interval or an independence claim.

`EconomicCandidate` combines a scoped `CurvePrediction` with unconditional
landing-latency masses and nonlanding mass. Masses sum to one and the mass within
the quote horizon must equal p_hat. `EconomicsInputs` shows size USD, edge bps,
lambda, SOL/USD conversion, base fee lamports and choice of median/p90 delay
cost. All USD values are decimal strings; estimates use floating-point arithmetic
only after preserving/validating provider price text and integer quantities.
`EconomicsQuote` retains the model quote, evaluated costs, Pareto frontier, knee
and named baseline differences. Stale/sparse/gapped/mismatched market evidence
preserves the probability-only quote with an explicit reason. The recommendation
must still meet the model's lower probability bound or upper latency bound.

Example normalized historical trade (abridged quantities):

```json
{"source":"replay","pool":"EgUV2hrWfsgfFyftxCx4ut82811ygWFbWYNPdv3cRWQA","mint":"CTMV7yXV1mpucM7svPEtmVFBB8g6hqHGVPczmbYrj9Jb","slot":"453068193","block_time_unix_s":1791066584,"tx_index":260,"ix_index":3,"inner_ix_index":3,"price_usd":"0.00013739913814689506","base_amount":"145561320614819","base_reserve":"210847950099700846"}
```

## Phase 3 passive tape and persisted quotes (3.4–3.5)

`PassiveTransaction` is a bounded normalized Yellowstone transaction subset:
resolved static+loaded account keys, compiled outer/inner instructions, actual
fee, execution success, receive clock and provider index scope. `PassiveTip`
aggregates explicit System transfers per allowlisted recipient; `tip_lamports`
is zero on failed execution, null when inner-transfer payment is unverified,
and `requested_tip_lamports` retains instruction
intent. Route is not inferred. `0006_phase3_quotes_tape.sql` adds bounded tape
storage and content-addressed market snapshots. Tape identity includes source,
observer, slot, block identity (when known), signature and recipient. Retention
defaults to 24 hours, 10,000 rows and 16 MiB per source; first receives survive duplicates.
Candidate-block ambiguities stay descriptive and never become landing labels.
These are gross committed transfer amounts, not recipient net-balance changes.
The transaction fee repeats in each recipient row; fee totals must deduplicate
transactions rather than sum recipient rows. Passive provider indices are decimal
u64 strings, preserving the native protocol width.

`Forecast.economics` and `requested_economics` are optional and omitted when absent, preserving historical
ledger bytes/hashes. Combined quote issuance freezes the models and economics
snapshot and appends the complete response atomically in the existing ledger.
The quote/economics model responses must match. Requested inputs survive fallback.
Saved market inputs remain
source/regime/pool-scoped and checked for freshness. Example tape row:

```json
{"source":"sim","observer":"grpc","signature":"synthetic-signature","slot":"1000","block_id":null,"index_in_block":"5","index_scope":"provider_reported","recipient":"synthetic-recipient","tip_lamports":"0","requested_tip_lamports":"100000","fee_lamports":"5000","cu_price_micro_lamports":"0","cu_limit":20000,"success":false,"received":{"clock_id":"sim-clock","mono_ns":"1000","wall_utc":"2026-10-05T00:00:00Z"}}
```

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

## Phase 1 persistence and observer daemon

Migration `0001_observe.sql` adds SQLite WAL runs, evidence, slot events, candidate
blocks, observations, observer cursors, gaps, canaries, resolution history, and
budget reservations. Unsigned slots remain padded decimal TEXT inside SQLite;
JSON contracts retain decimal strings. Events and cursors commit atomically.
Duplicates preserve the first receive timestamp; forks and index scopes remain
separate. A disconnect gap closes only when a normalized frame is committed.

`BlockMetaEvent` records provider candidate identity, parent identity, block height,
Unix-second chain time, and the observer receive clock. Missing transaction block
identity stays unknown; queries can associate a single same-observer candidate
without changing the raw stored observation. Multiple candidates forbid that
association. `raw_ref` hashes the selected canonical JSON provider fields, not the
original protobuf bytes.

Example normalized event (replay):

```json
{"kind":"block_meta","data":{"slot":"453062837","block_id":"AtrHDcMBCCaWf91ZzMRg1JPoBzD34nfGx3aKKoA7FLvu","parent_slot":"453062836","parent_block_id":null,"block_time_unix_s":1791065153,"block_height":"431101321","executed_transactions":"1045","received":{"clock_id":"fixture","mono_ns":"252072361","wall_utc":"2026-10-03T22:05:53.951Z"},"source":"replay","raw_ref":"sha256:captured-subset-hash"}}
```

The daemon currently accepts `observe` and offline `replay`. Observe never loads
a signing identity. Its loopback-only `/v1/health` and `/v1/clock` endpoints share
a 10-request/s limit. Health distinguishes durable data freshness from merely
having an open connection. Provider slot replay is disabled unless explicitly
configured; unsupported replay falls back to a durable gap and live subscription.

## Budget and resolution evidence

`BudgetLimits` and `BudgetReservation` use decimal lamport strings. Defaults are
200,000,000 lamports/day and 50,000,000 lamports per rolling 60 seconds. The
reservation includes UTC epoch milliseconds, UTC day, source, route, and unique
ID. SQLite checks the day against the timestamp, aggregates every route, rejects
backward clocks and duplicate IDs, and retains uncertain spend across restarts.
Example: `{"id":"simulated-reservation","source":"sim","route":"rpc","day":"2026-10-04","created_ms":"1791108000000","lamports":"5000"}`.
Observe/replay cannot receive a governor permit. No signer or sender exists yet.

`ResolutionEvidence` contains observations, optional `RpcCheck`, canonical block
membership, and a typed explicit route rejection. RPC proof names the exact
signature and source, required/checked commitment, checked block height, history
search flag, timestamp, optional landing candidate, and captured evidence hash.
Example absence proof: `{"signature":"simulation-signature","source":"sim","required_commitment":"confirmed","checked_commitment":"confirmed","checked_block_height":"201","searched_history":true,"history_covers_sent_slot":true,"context_slot":"210","checked_at_utc":"2026-10-04T10:00:00Z","landing":null,"raw_ref":"sha256:simulation-evidence"}`.
A proof predating the send is rejected. A sighting blocks expiry even when block
identity is incomplete. Confirmed outcomes remain provisional; finalized RPC
evidence retires landing/expiry records, while definitive route rejection can
retire an unsent attempt. Dropping requires canonical block
exclusion plus history absence. Transport disagreement remains UNRESOLVED.
These are additive v1 types; `0001_observe.sql` already supplies their storage.

RPC absence additionally requires `history_covers_sent_slot=true` and a
`context_slot` at least as recent as the send. Older serialized proof fields
default to false/zero, so they cannot grant expiry. The reader samples finalized
slot/height before history status, requires status context to cover that slot,
and checks `getFirstAvailableBlock` before accepting null history. Missing or
processed status, pruned history, unsupported blocks, and network failures keep
canaries pending. Actual `getSignatureStatuses` has no commitment argument; the
proof is the paired finalized height/context and history search, or a confirmed/
finalized status corroborated by candidate-block membership. See the official
[status method](https://solana.com/docs/rpc/http/getsignaturestatuses) and
[block method](https://solana.com/docs/rpc/http/getblock).

The observe daemon checks one pending live canary every five seconds, rotating
IDs. With no live canaries, this worker makes no RPC requests. It persists proof
subsets and outcome changes, and reloads pending records after restart.
The clock excludes conflicting candidate timestamps and DEAD slots, and reports
observed candidate-parent skips separately from a finalized network skip rate.

## Phase 1 send pipeline

The canary engine builds legacy transactions with integer compute limits, integer
CU prices, a memo, and an explicit published tip recipient. Small/Medium/Large
request 25,000/100,000/300,000 CU. Medium adds four read-only sysvars through a
zero-value system self-transfer. Large adds a 384-byte memo and four zero-value
transfers to addresses derived from the dedicated wallet's private seed. These
addresses add writable locks without allocating accounts or transferring funds.
They are synthetic workload shapes; no real-swap equivalence is claimed.

Policy v0 has 81 cells. An inverse-assignment-count stratified arm is mixed with
30% uniform exploration by default. Its logged propensity is the marginal
probability across both arms, computed before updating counts. A SplitMix64 seed
and draw, the chosen arm, exact assignment, and policy configuration key are
persisted by migration `0002_send_attempts.sql`. Restart reconstructs the stream
from the next committed draw. Example assignment:
`{"policy_id":"stratified-v0-splitmix64","seed":"1","draw":"0","assignment_prob":0.012345679012345678,"uniform_arm":true,"config":{"route":"rpc","tip_lamports":"0","cu_price_micro_lamports":"0","cu_limit":25000,"fee_bucket":"zero","tip_tier":"none","size_class":"small"}}`.

Balance, observer freshness, pending limits, recent local fees, and the cluster's
fee-for-message quote precede reservation. The governor reserves the entire
quoted fee plus tip before signing. The signed identity and assignment commit
atomically before network I/O. `PREPARED`, `ACCEPTED`, `REJECTED`, and `UNKNOWN`
are send-attempt states, distinct from landing outcomes. ACK means transport
acceptance; uncertainty remains charged and pending. Restart never broadcasts
old prepared transactions. Typed rejections add `RATE_LIMITED` and `TIP_TOO_LOW`
to the additive v1 enum; no existing payload changes are required.

Complete epoch schedules and stake/production recordings use bounded
`gzip+base64` evidence; the evidence-save cap is 8 MiB, while individual streamed
frames remain capped at 1 MiB. Classes use the scheduled validator cohort,
combine all vote-account stake per identity, and require at least 16 assigned
slots for a skip-rate class. Missing/other-epoch metrics are unknown. Equal
metric values receive the same tercile; an entirely tied cohort is middle.

Live and observe are now accepted by `alightd`; replay remains offline. Health
includes source-filtered send-attempt counts, today's budget reservations, the
engine gate/status, and upcoming leader classes. `/v1/leaders` is read-only and
shares the endpoint limiter. `canaries_sent` counts transport ACKs, with unknown
submissions separately visible. Migration `0003_metadata_retention.sql` indexes
receive times and transient raw evidence for bounded cleanup. It retains all
transaction observations, permanent proof bundles, and any block candidate
referenced by an owned canary. Existing raw evidence defaults to permanent.

## Phase 2 model and simulation foundation

Additive v1 types: `TrainingCanary` wraps the unchanged Canary schema plus an
explicit `finalized` boolean. Only owned source-filtered canaries enter training;
passive transaction observations do not. A finalized record also needs a resolved
outcome and resolution timestamp at or before the snapshot's evaluation time.

`CurveContext` contains source, regime_id, region and as_of_utc. `CurveSnapshot`
contains this context, exact CanaryConfig, a positive horizon_slots, estimator and
methodology hash, half-life in seconds, Beta parameters, mean, equal-tail posterior
interval, discounted n_effective, assignment/resolved/unresolved counts, unresolved
share, latest eligible data age in seconds and eligible send window. All amount
fields follow existing decimal-string conventions. `INSUFFICIENT` retains the
posterior as a diagnostic, includes samples_needed and explicit reasons, and does
not authorize a recommendation. M0 emits only MEASURED or INSUFFICIENT. Interpolation
and extrapolation require later pooled estimators. See
[registered methodology](methodology.md) for precise endpoint and eligibility rules.

Example shape (the actual registered hash and numeric output are in
`data/fixtures/phase2_curve_snapshot.json`):

```json
{"context":{"source":"sim","regime_id":"sim-r0","region":"synthetic-local","as_of_utc":"2026-10-05T00:34:19.000Z"},"config":{"route":"beam_quic","tip_lamports":"100000","cu_price_micro_lamports":"0","cu_limit":25000,"fee_bucket":"zero","tip_tier":"x1","size_class":"small"},"horizon_slots":1,"evidence":"MEASURED","n_effective":80.0,"data_age_s":40.0}
```

Migration `0004_curve_snapshots.sql` adds source/regime-filtered history with UTC
epoch milliseconds, methodology hash, full JSON and SHA-256 content ID. Identical
inserts are idempotent. Reads verify stored payload bytes against the ID. This is
curve history; the forecast hash chain is a separate, later Phase 2 component.
The offline commands use a separate database and do not migrate the paused live
database. Periodic daemon snapshots remain to be integrated.

`alight sim` writes a deterministic report with source=sim, explicit environment
parameters, normal Canary records, artificial ground-truth probabilities, exact
registered methodology hash, and curve snapshots. Synthetic signatures are null;
no transaction is signed or submitted. `replay-model` recalculates those same
snapshots and rejects disagreement or methodology-version drift. Replay execution
preserves the dataset's original sim source rather than relabeling synthetic
evidence as live. Shared JSON enables exact float round trips for reproducibility.

## Phase 2 forecasting contracts

`TrainingCanary.covariates` is an additive field with unknown defaults. Congestion
is dimensionless in [0,5], and absent values remain unknown. Simulator parameters
can independently disable fee effects and failure mass for null/coverage checks;
an optional continuous receive-latency mode retains integer landing slots.

`ModelFit` freezes coefficients, covariance, support, evaluation context and held-out
M0/M1 losses. M1 uses nonnegative tip/fee coefficients, global leader/congestion
effects and penalized route deviations. M2's M1 weight is logistic(50 times the
held-out M0 minus M1 loss), computed using an earlier fit and later outcomes.
`CurvePrediction` adds interpolation/extrapolation flags; extrapolated intervals
are [0,1] and cannot authorize a recommendation. Sparse, stale and unresolved
evidence remains insufficient. Blend intervals enclose both component intervals.

`ModelQuoteRequest` carries candidate configurations, source/regime/region/as-of,
congestion, upcoming leader classes and a tagged probability or latency-quantile
target. Probability targets use a positive slot horizon. Latency targets specify
exactly one positive slot or millisecond ceiling. `ModelQuote` recommends the
lowest nominal fee-plus-tip candidate whose lower probability bound or upper
latency bound meets the target. Economics remain separate. An insufficient result
has no recommendation and states a sample requirement/reason. `LatencyEstimate`
uses null for infinity or unidentifiable intervals; it never drops failure mass.
Milliseconds require valid same-clock observer measurements, not a slot conversion.

Migration 0005 adds frozen model documents, per-source forecast chains and heads,
append-only grade APIs, daily Signal documents and owned-canary covariates. The
chain hashes a version domain, source, sequence, previous hash and canonical sorted
JSON. The stored head detects missing tails; SQL triggers reject forecast updates
and deletions. An externally published/anchored head is needed to protect against
someone rewriting the entire database. A grade never alters a forecast.

`Forecast` contains the complete request/quote, expiry, source/regime, methodology
hash, frozen model hash and B1–B4 alternatives. `ForecastGrade` includes reliability,
Brier, natural-log loss, rate interval coverage and separate through-change scores.
Only finalized same-source/config outcomes sent strictly later than creation and
before expiry are scored. No-claim forecasts and straddling regimes are voided;
pending/late outcomes remain visible. Expired forecasts with no later matched
outcomes are voided with that reason. B3 takes distinct same-source passive tape
observations from the previous five minutes, ignores future/stale observations,
and stores the window, count and integer median. Without these inputs it stays
unavailable; simulations supply a clearly artificial tape. Verification also
checks the referenced frozen model documents.

`SignalReport` carries source/day/as-of, methodology hash and nine route/size
cohorts and `completed_utc_day`; manual same-day reports are provisional. Each cohort publishes uniform-arm primary and IPW sensitivity effects,
99% family bands, exclusions, and DISCRIMINATING/FLAT/INCONCLUSIVE. Regression
slopes are unrestricted; identified hour/congestion interactions are included.
See the prospective v2 clarification in the existing methodology document.

The sender and all observers now share `process_clock_origin()` in one process.
Restart changes its UUID. Historical records retain their old independent clocks
and cannot retrospectively become comparable millisecond samples. Policy v1's
fixed RNG consumption permits O(1) seeded restart at the durable draw; it never
re-sends a prepared transaction. Live local fees are measured after configuration
selection, so its pre-selection nominal cost uses zero for the unknown fee price.
The governor still reserves the actual quoted fee plus tip before any signing.

The CLI accepts these JSON requests using `alight quote --database PATH --request
PATH`; this command writes an immutable forecast but sends no transaction. For
example, a small RPC candidate with unknown congestion and leaders:

```json
{"context":{"source":"live","region":"local","regime_id":"phase1-unclassified","as_of_utc":"2026-10-05T06:00:00Z"},"candidates":[{"route":"rpc","tip_lamports":"0","cu_price_micro_lamports":"0","cu_limit":25000,"fee_bucket":"zero","tip_tier":"none","size_class":"small"}],"covariates":{"congestion":null},"leader_class_next":[],"target":{"kind":"probability","target_p":0.9,"horizon_slots":2}}
```

Empty live training returns `INSUFFICIENT`, no recommendation and a sample
requirement. A latency target replaces `target` with
`{"kind":"latency_quantile","quantile":0.9,"max_slots":4.0,"max_ms":null}`.
`ledger verify`, `grade`, `model-tick`, and `signal` take an explicit database and
source. Tick/grading/Signal take an explicit as-of UTC timestamp. They never load
`.env`. `sim --phase2` runs quotes, frozen baselines, grading, history and Signal
against later synthetic canaries; replay verifies those outputs as well as M0.
Outputs refuse to overwrite a different historical report.

Beam HTTP transport clarification (5 October): `beam_http` submits a transaction
with a Beam tip through the same configured Solami RPC endpoint/authentication
used by `rpc`; the latter carries no Beam tip. The enum values stay unchanged,
including historical simulation and forecast records. They identify experimental
submission treatments, not independent HTTP endpoints. QUIC remains separate.
