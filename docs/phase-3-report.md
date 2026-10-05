# Phase 3 — economics and persisted quote service

5 October 2026. Tasks **3.1–3.5** pass offline acceptance: Blur economics,
passive tape and immutable combined quotes. Phase 3 remains open: API, Prove,
SDKs, CLI completion and alerts (3.6–3.10) follow. No owned live outcomes or
market savings are established by these checks.

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
