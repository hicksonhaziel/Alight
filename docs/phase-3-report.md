# Phase 3 — economics, API and Prove

5 October 2026. Tasks **3.1–3.6** pass offline acceptance: Blur economics,
passive tape, immutable combined quotes and REST/WS API. Prove passes Sim
acceptance through its HTTP endpoint. Phase 3 remains open: SDKs, CLI completion,
alerts and funded live acceptance follow. No owned live outcomes or market
savings are established by these checks.

The Blur REST/WS client follows the recorded paths, queries and payloads, uses
bounded responses and pool filters, retries disconnects with capped backoff,
and exposes gaps. Credentials/URLs are withheld from typed errors. WS replay
reads authoritative `raw_json`; the captured large reserve
`210847950099700846` and signed inner-instruction sentinel `-1` survive intact.
Historical replay always emits `source=replay`.

The rolling price series is bounded to 10,000 trades per pool by default. It
deduplicates swaps, keeps the last observed provider-ordered USD price per slot,
and requires exact slot-distance pairs. It reports median/p90 absolute returns
in bps, pair counts, data age and split-half stability. The measured slot clock
converts slot distance to ms; minute candles and provider indexing timestamps
do not create sub-second trade timestamps. Default gates are 30 pairs per
positive delay, a 300-second window and 30-second price freshness. Gaps split
the series, excluding prior coverage from current estimates.

The optimizer evaluates nominal tip/base/priority fees, unconditional delay
loss and missed-horizon edge. It shows the inputs and approximation, both median
and p90 delay objectives, a Pareto frontier, a descriptive knee and named
baseline differences. Its recommendation still meets the model's lower
probability or upper latency bound. Incomplete delay/latency evidence, stale or
sparse prices, gaps and scope mismatches preserve the probability-only quote.
Combining market returns and canary landing assumes the user's transaction
behaves like the named canary size class; no validated real-swap claim follows.

## Acceptance

The original foundation receipt is `data/phase-3-economics-validation.json`; its
59-test record remains historical. Raw local check
logs and replay output are in ignored `.alight/phase3/`.

- Historical REST replay: three pools, nine trades, ten separate minute candles.
- Historical WS replay: one connection frame, eleven swaps; all parsed exactly.
- Repeated offline replay: byte-identical output. All three small REST snapshots
  report sparse delay evidence; later timestamps report stale evidence.
- Known inputs recover geometric returns, median/p90 tails and split-half shifts;
  bounds, duplicates, conflicts and disconnects are exercised.
- Synthetic optimizer golden: central costs $21.1105, $11.0705 and $9.8705;
  recommendation's modeled difference versus B1 is $11.24. These are arithmetic
  checks with artificial probabilities/prices, not observed savings.
- Loopback REST and forced WS disconnect tests validate the live client paths
  without contacting Solami. Source labels inside those adapter tests do not
  establish live provider evidence.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --features alight-ingest/beam --locked
  --offline -- -D warnings`: PASS.
- `cargo test --workspace --features alight-ingest/beam --locked --offline`:
  **59 passed, 0 failed**, including 12 economics tests.
- Repository credential/private-path scan and historical Phase 2 receipt
  verification: PASS. No web package exists yet; npm web checks do not apply.

Run the historical example without keys or network:

```bash
cargo run -p alight-econ --example replay --locked --offline -- \
  250 2026-10-03T22:14:40Z
```

Here 250 ms is an explicit replay conversion input, not a current slot-time
measurement. The emitted source and timestamps remain historical.
Pipe this output into `python3 scripts/verify_phase3_receipt.py` to verify the
receipt, fixture hashes, exact replay bytes and hand-computed optimizer costs.

## Tape and combined quotes

`alight-tape` resolves static and loaded keys, parses outer/inner native SOL
transfer intent, CU limit and micro-lamport price, and preserves fee and provider
index scope. The recorded full Mirage/Yellowstone capture yields three parsed
transactions; a controlled recipient in the second identifies a 9,897-lamport
transfer and 5,000-lamport fee at provider index 672. That recipient is a parser
control, not evidence of a published Beam tip or transport. The reduced gRPC
fixture contains no instructions and cannot establish amounts. A native protobuf
test exercises the collector mapper, loaded keys and full-width integer fields.
The existing filtered subscription feeds the tape; no second gRPC stream is added.

Failed transactions have paid tip zero while retaining requested intent. Inner
payment is unknown without per-instruction execution evidence; caught CPI failure
cannot become a paid amount. Self transfers pay zero. Rows stay scoped by source,
observer, signature, slot, known block identity and recipient. Default per-source
caps are 24 hours, 10,000 rows and 16 MiB of JSON payload, with idle pruning.
These payload caps exclude SQLite/index overhead; the existing database page cap
still applies. Adversarial quota, duplicate, fork, source and restart tests pass.
Passive data never enter owned training tables. B3 counts known positive payments
once per signature and excludes uncertain payments, conflicts and ambiguous forks.

`issue_combined` returns and saves the complete model/economics response, including
requested inputs on fallback and the content-addressed market document when used.
Same-source/regime/pool lookup cannot read future market snapshots. Exact-cell
decayed conditional delay bins are projected onto the model's horizon probability;
this assumption and residual nonlanding mass are printed with the estimate.
Missing shapes and stale/sparse market evidence retain probability-only quotes.
Model fitting uses a blocking worker so it does not run on ingest executor tasks.

End-to-end tests recover a supported economics recommendation, aligned baseline
costs, unchanged fallback probability recommendations, distinct forecasts for
different fallback inputs, concurrent retry idempotence and identical persisted
responses after restart. Mismatched model/economics responses cannot append.
Ledger verification also checks the saved market documents. Optional new fields
are omitted for historical forecasts, preserving existing hashes and replay bytes.

`data/phase-3-service-validation.json` records this batch. Formatting and all-target
workspace Clippy with Beam pass; **69 workspace tests pass, 0 fail**, including ten
new tape/combined-service tests. Both historical Phase 2 and economics receipts
verify. Collection remains paused, the live database and README remain untouched,
and no transactions were signed or sent. API/Prove/SDK/CLI/alerts and funded live
acceptance remain pending.

## Prove backend and read-only preview

`alight-prove` locks an existing supported forecast, model/methodology hashes,
exact configuration, source/regime and requested N. Membership is committed
before broadcast and permanently excluded from fitting. The existing governed
canary engine can schedule the frozen live configuration; this path has not been
run on mainnet. The methodology in `docs/prove-methodology.md` was registered
before funded Prove results. Wilson intervals and all three verdicts are tested;
partial runs, missing clocks and unresolved outcomes remain inconclusive.

The 40-attempt Sim test covers quote, lock, durable budget, held-out membership,
grading, ledger verification and restart. It retains 100 training samples and
40 held-out members, reserving exactly 8,201,040 synthetic lamports. Two crash
windows resume without duplicate reservations or prepared rebroadcast. Smaller
caps stop at two members, and expiry never creates the missing 38 outcomes.
Stale reports cannot undo progress or revive a voided claim. These are synthetic
and persistence checks; they establish no live landing probability.

`preview_combined` computes the complete response without saving model snapshots
or forecasts; its bytes match the frozen operator response. Source-scoped ledger
pagination is bounded. Shared contracts now derive JSON schemas with lossless
u64 string fields. The HTTP/WS routes, operator authentication and schema contract
tests are still task 3.6, so none of those endpoints are claimed as served.

`data/phase-3-prove-validation.json` records this batch. Formatting and all-target
workspace Clippy with Beam pass; **76 workspace tests pass, 0 fail**. The final
Prove changes also pass the six focused tests. Repository scanning, historical
Phase 2 receipts and exact economics replay pass. No web package exists yet.
Collection remains paused; no live transaction was signed or sent. Phase 3's
CLI/SDK quote and funded live N=40 Prove exit condition remains open.

## REST/WS API and Sim daemon

`alight-api` serves source-scoped health/clock/leaders, curve history, read-only
quote preview, authenticated forecast freeze/Prove, reports, ledger pages/full
verification, observer freshness, passive tape and generated OpenAPI 3.1. Defaults
are 20 requests/s globally, two operator writes/s, two model/verification workers
and 32 WS connections. Bodies/queries, row counts and stream frame sizes are
bounded. Keys are hashed and compared in constant time; unset keys disable writes.
Fixed errors never echo a request, credential or provider response. WS snapshots
are read-only; input controls are rejected and slow clients disconnect.

Three integration tests validate actual REST and loopback WS captures against the
served schemas, including full-width integer strings and malformed/overflow
cases. They cover authenticated quote -> N=40 held-out Prove -> ledger verification,
restart without duplicate spend, source isolation, disabled/incorrect keys,
observe refusal, public/operator rate caps, body/query bounds and WS connection
limits. Python `jsonschema==4.23.0` is pinned for those checks in CI.

The actual `alightd --mode sim` test starts with deliberately invalid provider
and signing configuration, quotes its synthetic dataset, completes N=40, kills
the process and restarts against the same DB. It recovers the same report, clock
and reserved amount with zero sends. Sim returns before reading `.env` or creating
provider/wallet clients; its optional operator key file is an API credential.
Caddy now forwards the GET allowlist and two authenticated POST paths. Its
configuration validates in a temporary network-disabled container; existing
collector/Caddy services remain stopped. The old image still needs rebuilding
before a future owner-authorized live resume.

`data/phase-3-api-validation.json` records this batch. The focused API, daemon and
shared-type checks pass (11 tests), along with formatting, all-target workspace
Clippy, repository scanning and Caddy validation. The earlier 76-test workspace
receipt remains historical; the final full Phase 3 sweep follows SDK/CLI/alerts.
