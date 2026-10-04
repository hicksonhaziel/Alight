# Phase 1 observe foundation — 4 October 2026

The local Rust collector now reads real Solami gRPC/Mirage traffic into SQLite
WAL, exposes loopback health, records reconnect/restart gaps, and contains a
tested budget governor and outcome resolver. This is the observe foundation,
not the full Phase 1 exit gate. No transaction was signed or sent; spend is zero.

| Work | Evidence and result |
| --- | --- |
| 1.1 Store | Migration, lossless slot ordering, evidence/cursor atomicity, semantic deduplication, fork separation; persistence tests PASS |
| 1.2 Native readers | Recorded gRPC/Mirage replay PASS; live forced gRPC disconnect produced a gap and another receive-clock ID; capped reconnect backoff; bounded queue of 128 |
| 1.3 Clock | 1,919-slot saved window: gRPC and independent RPC timestamps both yield **267.847837 ms/slot**, within the 1 ms tolerance; DEAD/conflicting candidate tests PASS |
| 1.8 Governor | Exact lamport decimals, global daily/rolling-burst caps, per-route breakdown, duplicate/backward-clock refusal, conservative unknown spend; 100 concurrent requests across two SQLite pools cannot exceed caps; restart preserves spend |
| 1.9 Resolver | Late sightings block expiry; disagreement stays UNRESOLVED; expiry needs exact signature/source, post-send checked height, retained history/context, and RPC absence; actual positive/negative RPC probes PASS |
| 1.10 Restart | A simulated mid-flight canary reloads and resolves a late landing after reopening SQLite; actual collector SIGKILL/restart preserved rows and resumed healthy readers |
| 1.11 Observe | Native `alightd --mode observe`, offline replay, `/v1/health`, `/v1/clock`; loopback only, 10 requests/s, signing identities excluded |

The crash snapshot contained 40,812 slot events, 9,121 candidate-block rows, and
32 transaction observations. After restart these reached 45,844 / 10,240 / 36;
the prior run was recorded as interrupted, the new run started, and both
observers recovered. Candidate-block rows are separate per observer: their count
is not a count of unique canonical blocks. Transaction filters select the public
canary payer and published tip accounts; these are ambient transactions, not
Alight canaries or route performance measurements.

The initial test collector ran approximately 21 minutes before the intentional
kill. A later attached development process stopped gracefully after about three
minutes; its exact signal was not captured. The binary now records its shutdown
reason. The current collector runs detached with its own process session and a
pinned executable under ignored `.alight/`; it remains local to this computer.
Continuous one-hour validation and host deployment are still pending.

Use `python3 scripts/collector.py status`, `stop`, or `start` to manage the local
observe collector. Build `cargo build -p alightd --locked` before starting a new
version. The helper never starts live mode. Logs and PID stay under `.alight/`.
Health is at `http://127.0.0.1:8080/v1/health`; runtime evidence is in the ignored
`data/alight.db`. The default main database cap is 512 MiB; writes fail and the
daemon stops when the cap is reached. This is bounded local collection, not a
deployed service with automated storage retention or reboot recovery.

Acceptance commands:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features alight-ingest/beam --locked -- -D warnings
cargo test --workspace --features alight-ingest/beam --locked
python3 scripts/check_repository.py
python3 scripts/verify_phase1_receipt.py
```

All local checks passed. There is no web application yet, so web lint/tests do
not apply. The sanitized receipt is `data/phase-1-validation.json`; its fixture
hashes and evidence invariants are checked in CI.

Still required: recorded leader schedule/classification, deterministic canary
builder and assignment policy, route senders with cost-capped live smoke tests,
funding-dependent G0/pilot checks, live-mode integration, the one-hour runtime
gate, and persistent host deployment. Provider slot replay remains off by
default until its support is verified; a gap never claims that missed history
was recovered. gRPC/Mirage transport agreement does not prove validator
independence, and provider indexes remain separate from RPC list positions.

GitHub now uses `main` as its only project branch. Coherent batches are committed
and pushed directly there. README and the local briefs/context were preserved;
ENV, database files, and raw evidence remain ignored.
