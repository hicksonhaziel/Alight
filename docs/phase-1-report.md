# Phase 1 implementation and local deployment — 4 October 2026

Phase 1 engineering is implemented, and local deployment was verified on
Hickson's computer with real Solami observers, durable SQLite evidence, and a
governed live canary engine. **Collection is now paused at Hickson's request to
save internet data.** The image, database and private backup are preserved. **The full phase remains open:** the dedicated wallet has no SOL, so no
owned canary has been signed, sent, or landed. Funded route validation and the
live mid-flight restart gate cannot be claimed. Beam HTTP additionally needs a
reachable, verified provider endpoint.

## Implementation and acceptance

| Task | Result and evidence |
| --- | --- |
| 1.1 Store | SQLite WAL, migrations, atomic evidence/cursors, candidate-block separation, semantic deduplication, and persistence tests PASS |
| 1.2 Observers | Recorded gRPC/Mirage replay and a real forced-disconnect gap PASS; capped reconnect backoff and bounded queues; history replay stays disabled until verified |
| 1.3 Clock | Saved gRPC and independent RPC timestamps both measured **267.847837 ms/slot** over 1,919 slots; difference within the 1 ms tolerance |
| 1.4 Leaders | Recorded full epoch schedule, distinct upcoming leader rotations, stake/skip-rate terciles, missing-data and tie handling tests PASS; raw epoch evidence retained |
| 1.5 Builder | All **81 grid cells** have deterministic golden unsigned bytes; Small/Medium/Large compute budgets, memo, local fee buckets and published tip tiers; signing requires a governor permit |
| 1.6 Routes | Beam QUIC, Beam HTTP and RPC adapters implemented with typed rejections and uncertain-send tracking; **one landed canary per route pending** |
| 1.7 Assignment | Seeded stratified policy with a 30% uniform arm by default; marginal propensity recorded before assignment counts change; normalization, arm frequency and restart determinism tests PASS |
| 1.8 Governor | Exact lamport budgets, global daily/rolling-burst caps, route breakdown, conservative fee-plus-tip reservations and restart persistence; adversarial concurrency tests PASS |
| 1.9 Resolver | Late sightings prevent false expiry; disagreement remains UNRESOLVED; expiry requires last-valid-height and explicit RPC absence evidence; replay and recorded RPC probes PASS |
| 1.10 Restart | Simulated mid-flight canary reload and late resolution PASS; prepared signature, reservation and policy draw persist atomically before outbound IO; funded mid-flight kill remains pending |
| 1.11 Daemon | Observe/live modes, offline replay, read-only health/clock/leaders; **one continuous hour of real observe traffic PASS**, zero spend |
| 1.12 Deployment | Compose, Caddy, persistent host data, private key file mount, restricted loopback endpoints and redacted rotating logs; current deployment evidence below |

The live engine checks observer freshness, confirmed wallet funds, pending
liabilities, current leaders, published tip recipients, local fee samples and
RPC fee quotes. It reserves worst-case fees plus tips before signing. It stores
the canary, wire hash, assignment probability and policy draw before submission.
Prepared and uncertain submissions survive restart and are resolved from chain
evidence; they are never automatically re-broadcast. An ACK counts as an accepted
submission, not a landing. Only the canary crate holds the signing path.

## Runtime evidence

The saved continuous observe run reached **3,829 seconds** with healthy gRPC and
Mirage readers, **207,123 slot events**, **46,294 observer-specific candidate
blocks**, **313 ambient transaction observations**, and **zero open gaps**.
It had zero owned canaries and zero budget reservations. Candidate rows are
separate per observer; ambient filtered observations do not measure Alight route
performance. The snapshot is `data/fixtures/phase1_one_hour_observe.json`.

Deployment and crash-recovery evidence is recorded in
`data/phase-1-implementation-validation.json`. The earlier
`data/phase-1-validation.json` remains a historical observe-foundation receipt.
The container ran in live mode with healthy gRPC/Mirage readers, loaded current
leaders, and reported WAITING_FUNDS at zero wallet balance. A SIGKILL of the
daemon child triggered Docker's automatic restart. The previous run was marked
interrupted, stored counts were preserved, and healthy collection resumed. The
restricted proxy returned 200 for health, 404 for an unlisted path, 405 for a
write method, and 502 during a deliberate backend stop. Synthetic query and
credential markers were absent from both access and proxy error logs.

The accumulated database exposed a real retention bug: SQLite scanned metadata
indexes for every evidence reference while holding its single connection. That
starved startup and health checks. Cleanup now examines only references removed
by the bounded batch and uses index seeks. The saved query-plan check has no
scans; all four store tests and affected daemon Clippy passed after the fix.
Caddy's required binding capability and SQLite temporary workspace were also
corrected. Startup failures now include sanitized categories for diagnosis.

Both receipts are checked offline in CI, including fixture hashes. The implementation
receipt keeps `phase_complete: false` until the funded acceptance gates pass.

## Validation and operating limits

The relevant workspace suite passed **32 tests**, including golden grid bytes,
policy properties, budget safety, resolver replay, migrations and offline daemon
replay. The prepared-send atomicity test was rerun after its source/permit guard
was tightened. The four store tests were rerun after the retention fix. Formatting, workspace Clippy with warnings denied, repository
secret checks and existing evidence verification passed. The core and daemon
commits also passed GitHub CI. No web application exists yet, so web lint/tests
do not apply.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features alight-ingest/beam --locked -- -D warnings
cargo test --workspace --features alight-ingest/beam --locked
python3 scripts/check_repository.py
python3 scripts/verify_phase1_receipt.py
```

The dedicated canary wallet is unfunded. Default controls are 0.20 SOL/day,
0.05 SOL per rolling minute, a 120-second send interval, a 1,000,000-lamport
reserve and at most eight pending canaries. Budget amounts are worst-case
reservations, not a claim of actual transaction cost. No funds were moved from
another wallet. Once funded, the live engine can send within these limits.

The published `beam-http.solami.dev` hostname did not resolve during validation;
Google Public DNS also returned NXDOMAIN (status 3). The timestamped check is
`data/fixtures/phase1_provider_endpoint_check.json`.
The HTTP adapter uses the documented JSON-RPC shape, but its provider contract
and real landing remain unverified. A transport failure stays uncertain and
charged conservatively. The QUIC SDK's embedded tip list was older than the live
published list, so its local scan is bypassed after Alight validates the chosen
public recipient. These contradictions are recorded in `docs/decisions.md`.

Routine slot/block metadata have bounded 24-hour retention. Observations,
canaries, reservations, proof evidence and referenced candidate blocks remain;
legacy raw evidence is retained conservatively. Compose allows an 8 GiB main
database and fails closed at the cap. The computer must stay awake with Docker
running. This local HTTP endpoint is not a public TLS deployment. See
`docs/operations.md` for startup, mode changes, backups and rollback.

All work is committed and pushed on `main`. README was left untouched. Local
briefs, context, ENV, keys, runtime databases and backups remain excluded from
Git. No third-party notice was added.
