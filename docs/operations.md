# Operations

Run commands from the repository root. The [README](../README.md) covers the
short offline quick start; this guide covers configuration and ongoing use.
Use the pinned Rust toolchain, Python 3 and Node 22.22 or later.

## Offline Workbench

Build the binaries and examples, then install web/client dependencies:

```sh
cargo build -p alight -p alightd -p alight-client --bins --examples --locked
npm --prefix web ci --ignore-scripts --no-audit --no-fund
npm --prefix sdk/ts ci --ignore-scripts
npm --prefix sdk/ts run build
```

Create a private **API operator key**, separate from any wallet/provider key:

```sh
python3 - <<'PY'
from pathlib import Path
import secrets
p = Path('.alight/operator.key')
p.parent.mkdir(exist_ok=True)
if not p.exists():
    p.write_text(secrets.token_hex(32) + '\n')
    p.chmod(0o600)
PY
target/debug/alightd --mode sim --seed 42 --sim-canaries 900 \
  --sim-regimes true --db .alight/workbench-sim.db --bind 127.0.0.1:8081 \
  --operator-key-file .alight/operator.key
```

In another terminal:

```sh
ALIGHT_API_ORIGIN=http://127.0.0.1:8081 npm --prefix web run dev
```

Open `http://127.0.0.1:5173`. Sim loads no `.env`, provider configuration or
signing keys. Outcomes and regime history are synthetic and visibly labeled.
Omit `--operator-key-file` for public reads only. Use the same seed/database
on restart; a changed seed needs a fresh database. `--run-for 30` bounds a
daemon session to 30 seconds. Keep each daemon's database and bind port unique.

The Workbench includes Cockpit, Quote, Prove, Ledger, Regimes, Health and Tape/
Receipts. Hash routes restore issued forecasts and Prove runs after reload.
`Ctrl/Cmd K` opens navigation. Both themes support keyboard focus, table
alternatives and reduced motion. The optional operator key stays in page memory
and is cleared on reload/disconnect. Only quote freeze and Prove require it.
Evidence refreshes every 15 seconds; the stream pauses while the page is hidden.

## Observe and Live

Copy `.env.example` to ignored `.env` and fill in your own provider read access.
Use a dedicated capped-balance canary wallet for Live. Keep private values out of
URLs in screenshots, versioned files and exported evidence.

```sh
test -e .env || cp .env.example .env
target/debug/alight doctor --mode observe --network
target/debug/alight run --mode observe
```

Observe collects Live evidence without loading signing identities or starting
canary/anchor senders. Streams consume provider bandwidth. RPC/Webhook freshness
can age when owned transactions stop; probe requests do not refresh it. The
optional Blur worker runs when its read token is configured. It discovers up to
three pools, uses one filtered socket, and bounds each price series to 300
seconds/10,000 trades. Sparse/stale/gapped market evidence leaves quotes
probability-only. `ALIGHT_BLUR_ENABLED=false` disables that worker.

Live signs and sends through `alight-canary` after funding, observer, pending,
database and governor checks. Set explicit caps before starting a finite run:

| Setting | Default | Meaning |
| --- | --- | --- |
| `ALIGHT_DAILY_BUDGET_SOL` | `0.20` | Combined route reservations per UTC day |
| `ALIGHT_BURST_BUDGET_SOL` | `0.05` | Reservations per rolling 60 seconds |
| `ALIGHT_CANARY_INTERVAL_S` | `120` | Exploration send spacing, valid range 30–3,600 seconds |
| `ALIGHT_CANARY_RESERVE_LAMPORTS` | `1000000` | Minimum wallet balance retained |
| `ALIGHT_MAX_PENDING_CANARIES` | `8` | Pending cap, valid range 1–32 |
| `ALIGHT_GRID_PROFILE` | `full` | `full`, `lean` or `rpc-reference` |

For example, configure `0.003` SOL/day, `0.0006` SOL/burst and `lean` in your
local `.env`, then deliberately start a bounded session:

```sh
target/debug/alight run --mode live --run-for 1800
```

Prove schedules held-out attempts separately from ordinary exploration spacing;
its exact configuration and all governor/reserve/freshness checks still apply.
After a finite run, start Observe to continue read-only collection. Reservations
cover worst-case fees plus tips and survive uncertain responses/restarts; they
are distinct from audited on-chain spend. `WAITING_FUNDS`, `BUDGET_CAPPED`,
`PENDING_LIMIT` and other gate states are visible in health. ACK counts show
transport acceptance, not landing. Prepared/unknown attempts are resolved from
chain evidence and never automatically rebroadcast.

The full grid has 81 cells. Lean has ten Small cells: Beam QUIC/HTTP at X1/X5
tips and Zero/LocalP90 fees, plus two untipped RPC fee cells. `rpc-reference`
selects one Small, untipped, zero-priority RPC cell. Policy IDs record the
profile and assignment propensities. The adaptive uniform arm remains at least
30% of the selected grid. Narrow profiles cannot establish omitted route,
workload, fee or tip effects.

Beam HTTP uses `SOLAMI_RPC_URL`/`SOLAMI_RPC_TOKEN` with a published recipient tip;
plain RPC uses that same endpoint without the tip. QUIC uses the pinned official
Solami SDK and current published recipient list. Real owned landings have been
validated on the three routes; the [Live Prove receipt](../data/live-prove-validation.json)
tests only one RPC configuration. It does not establish cross-route calibration.

## CLI and SDK

The SDK examples construct a request from current health/curve data, freeze a
forecast, run 40 governed held-out Sim canaries and verify the ledger:

```sh
target/debug/examples/quote_then_send http://127.0.0.1:8081 .alight/operator.key
node sdk/ts/examples/quote_then_send.mjs http://127.0.0.1:8081 .alight/operator.key
```

These are canary examples, not swap builders. `quote_then_send`/`quoteThenSend`
also expose that flow as SDK helpers. Public reads omit Authorization. SDKs
require HTTPS except loopback HTTP, reject credentials/query strings in base
URLs, cap responses at 8 MiB and use a 30-second request timeout. Uncertain
writes require reconciliation using the printed forecast hash/Prove ID; never
repeat the whole convenience flow automatically.

```sh
target/debug/alight doctor --api http://127.0.0.1:8081 --source sim
target/debug/alight quote --api http://127.0.0.1:8081 --source sim --request quote.json
target/debug/alight quote --api http://127.0.0.1:8081 --source sim --request quote.json \
  --freeze --operator-key-file .alight/operator.key
target/debug/alight prove --api http://127.0.0.1:8081 --source sim \
  --request prove.json --operator-key-file .alight/operator.key
target/debug/alight prove --api http://127.0.0.1:8081 --source sim --id PROVE_ID
target/debug/alight ledger verify --api http://127.0.0.1:8081 --source sim
```

`quote.json` is the served OpenAPI `QuoteServiceRequest`; `prove.json` is
`{"request_id":"example","forecast_hash":"sha256:…","n":40,"seed":"42"}`
in Sim. Live omits `seed`. The returned `lock.id` identifies progress reads.
Observe/Replay reject new Prove runs. An unsupported quote returns no
recommendation. [Contracts](contracts.md) documents schemas and endpoint limits.

| Exit | Meaning |
| --- | --- |
| `0` | Successful check, supported quote or CONSISTENT Prove |
| `1` | Failed check or INCONSISTENT Prove |
| `2` | Configuration, transport or runtime error |
| `3` | Insufficient quote, degraded doctor or INCONCLUSIVE Prove |

`export --api` produces a bounded forecast-only JSON subset. `export --database`
produces the full CSV/Parquet dataset; see [datasets](dataset.md). Wallet capture
and evaluation commands are in [receipts](receipts.md); source-labeled Memo
commitments and mainnet controls are in [anchoring](anchoring.md).

## Diagnostics, Webhooks and alerts

Offline finite jobs use isolated databases and no provider keys:

```sh
target/debug/alight regime-sim --database .alight/regime-sim.db --seed 42 \
  --output .alight/regime-sim.json
target/debug/alight backfill --database .alight/history.db --input sanitized-history.json \
  --output .alight/history-report.json
target/debug/alight diagnostics --api http://127.0.0.1:8081 --source sim
```

Observe/Live diagnostics run inside the daemon: sealed clock windows, finalized
block sampling, observer comparison, regime detection and local alert history.
Optional `ALIGHT_SLOT_COMPUTE_LIMIT` and its `..._PROVENANCE` must be configured
together from verified evidence; otherwise block fullness stays unavailable.
Regime changes void crossing forecasts, reset old effective sample support and
enable uniform exploration for 30 minutes under unchanged spending limits.
Calendar dates alone never establish an upgrade. See the
[registered regime method](regime-methodology.md).

An explicitly requested `backfill --network` uses a separate Replay database,
42–56 requested days and 64–1,024 bounded RPC reads. It preserves retention gaps
and null timestamps; sparse averages cannot resolve every short event.

`SOLAMI_WEBHOOK_SECRET` enables exact-body HMAC-verified owned transaction ingress
at `/v1/webhook`. Registration is separate from daemon startup. The optional
[`alight-webhook.service`](../deploy/alight-webhook.service) runs
[`webhook_bridge.py`](../scripts/webhook_bridge.py) with installed official
`cloudflared`. Adjust checkout paths before installing the template. It manages
one payer-only/all-event-types registration, a random callback path and private
0600 `.env` values. Restart Observe once after first registration to load its
secret. The tunnel exposes only the bounded callback, never the operator API.
The API independently verifies HMAC and ignores unowned signatures. Private
audit hashes/statuses persist under `.alight/webhook/`; payloads and credentials
are not logged. [Owned delivery](../data/webhook-owned-validation.json) is
validated. Temporary tunnel addresses and laptop uptime are not production
availability guarantees. The bridge receives evidence; it sends no transactions
or chat messages.

Local alerts are persisted every ten seconds. Set a private
`ALIGHT_ALERT_WEBHOOK_URL` and `ALIGHT_ALERT_FORMAT=webhook|discord|slack` only
when outbound delivery is intended. An unset URL records `NO_ENDPOINT`; Sim
does not load it. Five rules cover route degradation, quote drift, observer
disagreement, regime changes and budget pressure. Each active condition emits
once across restarts and rearms after recovery. Delivery makes one bounded
five-second attempt without redirects or uncertain-response retries; it is
not guaranteed. Chat formatters disable interpreted mentions.

## Deployment and retention

The local host service template is
[`alight-observe.service`](../deploy/alight-observe.service). It expects the
installed daemon at `.alight/bin/alightd`, starts Observe and retries failed
startup after 30 seconds. API/Workbench availability and provider freshness are
separate measurements. Collection requires the computer and internet to stay on.

Optional Docker Compose uses a read-only `.env` mount and persistent host
`data/`. Stop any host daemon using the same database/port before switching,
and keep a consistent SQLite backup under ignored `.alight/backups/`. Compose
defaults to Live; the explicit override below starts read-only Observe:

```sh
docker compose --env-file .env -f deploy/compose.yaml build \
  --build-arg GIT_REVISION="$(git rev-parse HEAD)"
ALIGHT_RUNTIME_MODE=observe docker compose --env-file .env -f deploy/compose.yaml up -d --wait
docker compose --env-file .env -f deploy/compose.yaml ps
curl -fsS http://127.0.0.1:8080/v1/health
docker compose --env-file .env -f deploy/compose.yaml stop
```

Caddy serves `web/dist` and the API allowlist from one loopback origin; public
hosting needs a configured hostname/TLS. Provider credentials stay in the
daemon. Logs remove request headers/query strings and rotate. Collector
containers run unprivileged with read-only root filesystems and bounded scratch
space. Rebuild both images when backend/web contracts change. A build alone
does not start collection; `restart: unless-stopped` preserves explicit stops.

Routine slot/block metadata are pruned after 24 hours, at most 250 rows per
table/transaction and 100,000 per table/pass, yielding the connection between
batches. Owned observations, canaries, reservations, resolution evidence and
referenced block candidates remain durable. Legacy raw evidence is retained
conservatively. Compose caps the main database at 8 GiB; reaching the cap stops
writes and sends. Export/backup durable evidence before growth reaches it.

The read-only [`alight-stability.service`](../deploy/alight-stability.service)
runs a finite 48-hour sampled check of health, Workbench, known IDs and budgets.
Progress and failures persist in `.alight/stability/` across monitor restarts.
Do not reset a running window or hide failures. A pass requires the full measured
window; it cannot establish availability between samples or validate later
binary changes. Read-only monitoring never signs or sends.

## Verification

```sh
python3 -m pip install jsonschema==4.23.0
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix web run lint
npm --prefix web test
npm --prefix web run build
ALIGHT_BROWSER_EXECUTABLE=/usr/bin/google-chrome npm --prefix web run test:e2e
```

The Python validator is required by API contract tests. Browser tests start
temporary Sim/Vite servers on 8082/5180; keep those ports free. Use an installed
Chrome path, or install Playwright Chromium for your environment. They verify
quote → freeze → held-out Prove → ledger verification, recovery, tamper rejection,
accessibility and honest unavailable states. `scripts/test_phase3_developer.py`
checks the SDK/CLI lifecycle in temporary offline daemons after building examples.
Use focused local checks while collecting; full Rust builds may compete for
memory, so full workspace validation can run in CI.

The [security receipt](../data/hardening-security-validation.json) records the
12 October rustls fix, retained `rsa`/`bincode` notices and bounded boundary checks.
It is dated evidence, not certification. Repeat dependency auditing before
release. The [Live Prove receipt](../data/live-prove-validation.json) retains the
warm-up cursor qualification and pending 48-hour review; completed functional
checks do not establish an uninterrupted long run.
