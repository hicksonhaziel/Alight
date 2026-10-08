# Alight

Alight measures Solana canary landing, issues evidence-aware quotes, freezes
claims for held-out Prove runs, and keeps an immutable forecast ledger. It uses
Solami data and compares outcomes across observers. The workbench shows
uncertainty, source, sample size, age and unfavorable verdicts.

The verified path is offline Sim/Replay. Live collection is paused; funded owned
canaries, complete four-observer agreement and mainnet anchoring remain
unverified. The downloadable sample is **Sim**, not mainnet performance.

## Quick start: no keys or wallet

Use the pinned Rust 1.96.1 toolchain, Node 22.22 or later, and Python 3.

```sh
cargo build --locked -p alight -p alightd
mkdir -p .alight/judge
target/debug/alight doctor --mode sim
target/debug/alight sim --phase2 --canaries 243 --seed 61 \
  --database .alight/judge/sim.db --output .alight/judge/sim.json
target/debug/alight ledger verify --database .alight/judge/sim.db --source sim
target/debug/alight replay-model --input .alight/judge/sim.json \
  --database .alight/judge/replayed.db --output .alight/judge/replayed.json
```

This small deterministic run demonstrates the schema/ledger and honestly
reports insufficient cells. Use `--canaries 8100` for the larger model lifecycle
example. Sim/model replay make zero provider requests or transaction sends.

Run the workbench in two terminals:

```sh
# Terminal 1: isolated synthetic evidence and API; no ENV or signing keys.
target/debug/alight run --mode sim --seed 42 --sim-canaries 900 \
  --sim-regimes true --db .alight/judge/workbench.db --bind 127.0.0.1:8080

# Terminal 2
npm --prefix web ci --ignore-scripts --no-audit --no-fund
npm --prefix web run dev
```

Open `http://127.0.0.1:5173`; the source badge reads **SIMULATED**. For Sim
quote/lock/Prove writes, create a local API operator-key file (at least 16
characters), pass `--operator-key-file FILE` to the daemon, and enter the same
API key in the workbench. This is an API control key, not a wallet key.
The offline developer regression is `python3 scripts/test_phase3_developer.py`.

## Judge path: read access, no funded wallet

Observe needs your own read key and dashboard RPC/gRPC endpoints, without a
canary or SWQoS signing identity. Copy `.env.example` to `.env` only if no file
exists. Set `SOLAMI_RPC_URL`, `SOLAMI_RPC_TOKEN` when required,
`SOLAMI_GRPC_URL`, and `SOLAMI_GRPC_TOKEN` (or `SOLAMI_API_KEY`). Optional
Mirage/Blur/Webhook permissions are separate; missing coverage stays visible.

```sh
target/debug/alight doctor --mode observe
# Optional single bounded read-only RPC genesis check, no stream or sends:
target/debug/alight doctor --mode observe --network
# Start only when you intend to collect provider data:
target/debug/alight run --mode observe --db .alight/judge/observe.db --run-for 30

# Captured observer replay needs no key, wallet or network:
target/debug/alight doctor --mode replay
target/debug/alight run --mode replay \
  --replay data/fixtures/mirage_transactions_sample.jsonl \
  --db .alight/judge/observer-replay.db
```

Mode doctor reports prerequisites and never starts collection. Local presence
does not establish stream access; observe `--network` checks one RPC method
and leaves streams unchecked. General `doctor --network` is a larger read-only
integration probe. Exit codes: 0 success, 1 failed check/verdict, 2 configuration/
runtime error, 3 missing or inconclusive evidence where supported.

## Live setup and costs

Live mode can spend SOL. Configure a dedicated canary payer, authenticated
routes and provider read access; keep secrets in the ignored `.env`. Never use
a personal trading wallet. The sender requires reserve/balance gates, durable
daily/burst reservations and available observer evidence. Only `alight-canary`
signs/sends. To intentionally enable collection and governed sending later:
`alight run --mode live --db data/alight.db`. No live start is part of the
offline quick start; no funded performance is claimed.

Defaults: 0.20 SOL daily cap, 0.05 SOL burst cap, 120-second interval, 30%
minimum uniform exploration, 81-cell `full` profile. Optional
`ALIGHT_GRID_PROFILE=lean` explores 10 Small cells (X1/X5 Beam tips, Zero/
LocalP90 fees); RPC stays untipped. Lean omits Medium/Large and X2/X10 and
cannot support the registered four-tier Signal analysis alone.

No owned live transaction cost has been measured in this paused run. A landed
canary pays the executed tip plus the RPC-estimated fee; priority fee is in
micro-lamports per CU. Assuming a 5,000-lamport base fee and zero priority fee,
uniform mean nominal costs are 405,000 (`full`) / 245,000 (`lean`) lamports.
At 720 sends/day that is 0.2916 / 0.1764 SOL **before priority fees**, assuming
every send lands. These are planning arithmetic, not observed spend; adaptive
mix and caps change rate. Failed execution pays fees while transfers roll back.
Reservations are worst-case exposure, not balance reconciliation.

## Solami's work in Alight

| Product | Implemented use and evidence limit |
| --- | --- |
| Beam QUIC | Governed tipped canary submission; funded landings unverified |
| Beam HTTP | Tipped submission through the Solami RPC endpoint; not an independent HTTP backend |
| Yellowstone gRPC | Filtered primary observation, clock/block identity and tip tape |
| Mirage | Secondary transport via captured Yellowstone-compatible frames |
| RPC | Leaders, commitment/absence checks, block sampling and untipped submission |
| Blur | Captured pool/trade/candle shapes, conditional delay-cost economics; stale/sparse fallback |
| Webhooks | Exact-body HMAC observation; complete live registration/coverage unverified |
| Data API | Wallet-history adapter pending a verified payload; receipts use bounded Sim/Replay captures |

Quotes refuse unsupported recommendations and carry intervals, effective n,
age, workload/route/region/window and evidence class. Dollar estimates assume
the workload behaves like the measured canary class. Prove freezes the exact
configuration and grades later linked outcomes; all verdicts stay visible.
Regime changes append VOIDED grades without altering forecasts. A timeout is
never a final outcome.

## SDK, receipts, datasets and commitments

Rust: `crates/alight-client`; TypeScript: `sdk/ts`. Build TS with
`npm --prefix sdk/ts ci && npm --prefix sdk/ts run build`. Examples in
`sdk/ts/examples/quote_then_send.mjs` and `crates/alight-client/examples/`
use governed held-out canaries. Public endpoints are read-only and rate-limited;
operator quote/Prove writes require an API bearer key and are not retried.

```sh
python3 -m pip install -r scripts/dataset-requirements.txt
target/debug/alight export --database .alight/judge/sim.db --source sim \
  --day 2026-10-05 --output .alight/judge/dataset
python3 scripts/verify_dataset.py .alight/judge/dataset
target/debug/alight anchor prepare --database .alight/judge/sim.db --source sim \
  --output .alight/judge/anchor.json
target/debug/alight anchor verify --database .alight/judge/sim.db \
  --input .alight/judge/anchor.json
```

Datasets include CSV/Parquet, schema, checksums and the original full forecast
chain witness. `--format csv` needs no PyArrow. `export --api` keeps the earlier
forecast-only JSON behavior. The Datasets page serves reviewed downloads with
their own source labels. Data uses CC BY 4.0; private wallet captures are
excluded. Unsigned memo preparation sends nothing; mainnet acceptance is pending.

Receipts deduplicate fees, preserve unknown payments, and compare supported
historical Beam frontiers. Missing route/time/frontier evidence stays descriptive.
Spend above a threshold is conditional arithmetic, not proven waste.
See [receipts](docs/receipts.md), [datasets](docs/dataset.md),
[anchoring](docs/anchoring.md), [methodology](docs/methodology.md),
[limitations](docs/limitations.md), [operations](docs/operations.md),
[contracts](docs/contracts.md) and the [160-second demo script](docs/demo.md).

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features alight-ingest/beam --locked -- -D warnings
cargo test --workspace --features alight-ingest/beam --locked
npm --prefix sdk/ts run lint && npm --prefix sdk/ts test
npm --prefix web run lint && npm --prefix web test
npm --prefix web run test:e2e
python3 scripts/test_dataset.py
python3 scripts/check_repository.py
```

Rust/Tokio + SQLite WAL form the source-scoped core; axum serves the API,
React/Vite the workbench, and Docker Compose/Caddy the deployment. Keep private
ENV, handoff and planning briefs local. Repository licensing is separate from
the exported-data license; no repository-wide software license is declared here.
