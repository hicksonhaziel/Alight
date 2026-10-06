# Local Phase 1 operations

Run these commands from the Alight directory. The container reads the ignored
`.env` as a read-only file; credentials are excluded from the image and build
context. Its SQLite database stays in the host's `data/` directory. Compose
starts the governed live engine by default. An unfunded wallet waits without
signing; observe mode excludes the signing identities entirely.

```sh
docker compose --env-file .env -f deploy/compose.yaml build --build-arg GIT_REVISION="$(git rev-parse HEAD)" collector
docker compose --env-file .env -f deploy/compose.yaml up -d --wait
docker compose --env-file .env -f deploy/compose.yaml ps
curl -fsS http://127.0.0.1:8080/v1/health
```

On this computer, the image was also built using its installed Rust toolchain
and cached public Cargo registry. This avoids downloading a second compiler;
the ordinary Compose build above remains the portable path. Named contexts
contain only the compiler and public dependency cache, never `.env` or local
briefs. The resulting runtime still uses Debian and the same locked Rust sources.

```sh
docker build --build-arg BUILD_IMAGE=node:22.22.0-bookworm-slim \
  --build-arg GIT_REVISION="$(git rev-parse HEAD)" --build-arg BUILD_JOBS=4 \
  --build-context local_toolchain="$HOME/.rustup/toolchains/1.96.1-x86_64-unknown-linux-gnu" \
  --build-context local_registry="$HOME/.cargo/registry" \
  --tag alight-collector:phase2 -f deploy/Dockerfile .
docker compose --env-file .env -f deploy/compose.yaml up -d --no-build --pull never --wait
```

The previous systemd observe collector must be stopped before starting Compose,
since both use the same database and localhost port. Keep a consistent SQLite
backup under ignored `.alight/backups/` before switching. Build the current
host binary too (`cargo build -p alightd --locked`) if it is needed for rollback:
older binaries cannot write after the retention migration adds table columns.

Caddy exposes the public GET allowlist and authenticated quote/Prove POST routes
on `127.0.0.1:8080`. Request headers, URI/query strings, and client addresses are
removed from its access logs. The backend shares a 20-request/s limiter and a
separate two-writes/s operator limit. An unset `ALIGHT_OPERATOR_KEY` disables writes. This
local deployment uses HTTP; a public hostname and TLS belong to a later VPS
configuration. The collector uses a private container network, an unprivileged
UID, a read-only root filesystem, and no additional Linux capabilities. Caddy
retains only NET_BIND_SERVICE, required by its official binary's file capability.
The collector has a bounded 256 MiB temporary workspace for SQLite index creation.
Full epoch schedule responses have a separate bounded 45-second deadline.

## Offline Phase 3 API

The saved/stopped collector image predates these routes. Rebuild from current
sources before an owner-authorized live resume; this development did not restart
collection. For a provider/key-free Sim server with API operator authentication:

```sh
python3 -m pip install jsonschema==4.23.0  # contract-test dependency
python3 - <<'PY'
from pathlib import Path
import secrets
p = Path('.alight/operator.key')
p.parent.mkdir(exist_ok=True)
if not p.exists():
    p.write_text(secrets.token_hex(32) + '\n')
    p.chmod(0o600)
PY
cargo run -p alightd --locked -- --mode sim --db .alight/sim-api.db \
  --bind 127.0.0.1:8080 --operator-key-file .alight/operator.key --seed 42
```

Sim reads no `.env` or provider/signing keys, uses synthetic Beam HTTP small
canaries to seed its model, and shows `source=sim` throughout. The optional key
file is an API credential, not a wallet key. Without it public reads work and
operator writes are disabled. Use the same seed/database on restart; experiment
clock and charged reservations remain durable. A changed seed needs a fresh Sim
database. `--run-for 30` provides a bounded local session. OpenAPI is available at
`/v1/openapi.json`; endpoint contracts and error/limit semantics are in
`docs/contracts.md`. API tests invoke the pinned Python JSON Schema validator;
its package must be installed before `cargo test` on a clean checkout.

Public quote preview uses a URL-encoded `QuoteServiceRequest` in the `request`
query parameter. Operator `POST /v1/quote` freezes the same request and returns a
forecast hash; `POST /v1/prove` uses that hash and an idempotent request ID. Keep
bearer keys in the `Authorization` header. Only live mode schedules real sends,
through the existing observer/funding/budget/prepared-send checks. Observe and
replay reject Prove. Sim can demonstrate N=40 without provider traffic or spend.

Hickson requested that collection be paused after deployment verification to
save internet data. Both containers are explicitly stopped; the prior systemd
observe service is disabled. Local data and the verified image are preserved.
To resume using cached images, then inspect health:

```sh
docker compose --env-file .env -f deploy/compose.yaml up -d --no-build --pull never --wait
curl -fsS http://127.0.0.1:8080/v1/health
```

The observer streams consume internet data whenever the collector runs, even
with an empty wallet or in observe mode. Compilation and cached-image startup
are local. Public route checks can use the network; they never imply funding.

To restart or stop the deployment:

```sh
docker compose --env-file .env -f deploy/compose.yaml restart collector
docker compose --env-file .env -f deploy/compose.yaml stop
```

`restart: unless-stopped` recovers a failed process and restarts the deployment
when Docker starts. Explicitly stopped containers stay stopped. Collection
requires this computer to be awake and Docker running. Database/WAL files and
private keys stay local. Container logs rotate at 30 MiB for the collector and
10 MiB for Caddy. Routine slot/block metadata are pruned after 24 hours in bounded
batches; observations, canaries, budget reservations, resolution evidence, and
block candidates referenced by canaries are retained. Legacy raw evidence from
before the retention migration is conservatively retained. The main database
cap is 8 GiB in Compose; reaching it stops writes and sends safely. Back up or
export durable evidence before its long-term growth reaches that cap.

Live controls in `.env` are exact daily/burst SOL caps, a 30–3,600 second send
interval (120 by default), a seeded policy with a uniform fraction, a wallet
reserve of at least 1,000,000 lamports, and a pending limit of 1–32 (8 by default).
The default governor caps are 0.20 SOL/day and 0.05 SOL per rolling minute;
reservations are conservative worst-case fees plus tips, not reported actual
spend. Funding checks, cap checks, and database persistence precede any send.
`ALIGHT_RUNTIME_MODE=observe` starts Compose without signing access; restart with
`up -d` after changing mode. `ALIGHT_MODE` remains the host daemon's default.

Health exposes reader freshness separately as `collector_status`; the
`canary_engine` field states `WAITING_FUNDS`, `BUDGET_CAPPED`, `PENDING_LIMIT`, or
an explicit preflight/attempt status. `canaries_sent` counts transport ACKs;
unknown submission attempts remain separate in `send_attempts`. An ACK alone
never establishes a landing. Prepared/uncertain sends remain pending across
restart and are resolved through chain evidence; they are never re-broadcast.

Beam QUIC uses the pinned official SDK and the live published tip list. The SDK's
embedded list is older, so its local scan is bypassed after Alight validates the
recipient itself. Beam HTTP uses the configured Solami RPC URL and RPC
authentication, per the team's reply relayed by Hickson on 5 October: use the
RPC endpoint with a tip.
There is no separate HTTP URL/token setting. Its governed transaction carries a
published Beam-recipient tip; plain RPC carries none. These arms share an endpoint.
The earlier hostname failure is historical. Funded tipped submission and landing
remain unverified; a connection failure is retained as uncertainty. Do not report
a successful live route until a real canary lands and observers confirm it.

## Phase 3 developer clients and CLI

Build both binaries and the Rust canary example with `cargo build -p alight -p
alightd -p alight-client --bins --examples --locked`. The TypeScript client needs
`npm --prefix sdk/ts ci --ignore-scripts` and `npm --prefix sdk/ts run build`.
Start the key-free Sim daemon using the offline API instructions above (or
`alight run --mode sim` with the same daemon flags), then run either:

```sh
target/debug/examples/quote_then_send http://127.0.0.1:8080 .alight/operator.key
node sdk/ts/examples/quote_then_send.mjs http://127.0.0.1:8080 .alight/operator.key
```

Each example reads a supported synthetic curve, previews and freezes its quote,
prints the forecast hash/request ID before sending, runs 40 governed held-out
Sim canaries and verifies the ledger. These are canary bots; they neither build
swaps nor handle a wallet/provider key. `quote_then_send` / `quoteThenSend` also
provide that convenience in each SDK. On an uncertain write response, reconcile
the saved forecast hash and Prove ID; never repeat the entire convenience flow.
No write retries are automatic. SDK endpoints require HTTPS except loopback HTTP,
reject URL credentials/query parameters, enforce source labels, cap responses at
8 MiB and use a 30-second request timeout. Public reads omit the operator key.

CLI API commands take explicit `--api URL --source live|sim|replay`:

```sh
target/debug/alight doctor --api http://127.0.0.1:8080 --source sim
target/debug/alight quote --api http://127.0.0.1:8080 --source sim --request quote.json
target/debug/alight quote --api http://127.0.0.1:8080 --source sim --request quote.json --freeze --operator-key-file .alight/operator.key
target/debug/alight prove --api http://127.0.0.1:8080 --source sim --request prove.json --operator-key-file .alight/operator.key
target/debug/alight prove --api http://127.0.0.1:8080 --source sim --id prove-sim-example
target/debug/alight ledger verify --api http://127.0.0.1:8080 --source sim
target/debug/alight export --api http://127.0.0.1:8080 --source sim --day 2026-10-05 --output .alight/forecasts.json
```

`quote.json` is a `QuoteServiceRequest` from the served OpenAPI contract;
`prove.json` is `{"request_id":"example","forecast_hash":"sha256:…","n":40,
"seed":"42"}` in Sim. Live omits the seed. Prove IDs are returned in `lock.id`.
Examples construct a request from actual health/curve data, avoiding stale clocks.
The existing database-based model commands and seeded `sim`/replay remain available.
Exit codes are **0** successful/supported/CONSISTENT, **1** failed check or
INCONSISTENT, **2** configuration/transport/runtime error, **3** insufficient
quote evidence, degraded doctor health or INCONCLUSIVE. Live POST can return a
queued/inconclusive report; use `prove --id` to read progress without resubmitting.

Export verifies the source's whole ledger before paging to that verified head,
then selects forecast issue dates in UTC. It is bounded to 10,000 rows / 64 MiB,
creates a new output file and refuses overwrite. This Phase 3 export contains
forecasts; the Phase 6 dataset exporter will add canaries, regimes and Parquet/CSV.
A date-filtered subset cannot independently verify the complete chain or models.
Run `python3 scripts/test_phase3_developer.py` after building: it starts/stops only
temporary Sim daemons and checks both examples, CLI lifecycle, all four exit codes,
export/overwrite protection, and `run` without provider credentials.

## Phase 3 alerts

Observe/live daemons evaluate stored evidence every ten seconds and persist local
alerts. Outbound delivery is explicitly configured with the private
`ALIGHT_ALERT_WEBHOOK_URL` and `ALIGHT_ALERT_FORMAT=webhook|discord|slack`; unset URL
records `NO_ENDPOINT`. Sim does not load these environment values. No real
endpoint was configured or contacted during acceptance.

Rules use fresh same-cell canary evidence with effective n >= 30: route degradation
requires non-overlapping 95% intervals; quote drift compares a later same-regime
estimate with the frozen interval before expiry. Observer disagreement requires
three distinct recent canaries with explicit latest resolver disagreement evidence
in a five-minute window. A recorded regime transition fires once; budget alerts
fire at 90% of the exact configured daily cap. These report estimates/recorded
transitions and do not claim a new network upgrade or real-swap performance.

Migration 0008 adds source-scoped rule state/events. One atomic transition emits
one event per active condition; restart preserves suppression and recovery rearms
it. The event history is capped at 10,000 rows per source. Webhook delivery makes
one bounded, five-second attempt with redirects disabled. Delivery errors do not
retry an uncertain response. The dispatcher claims `PENDING` events as `ATTEMPTING` before delivery. A crash
then leaves an uncertain attempt that is not automatically retried; external
delivery is not guaranteed. New regime events use the same dispatcher. Discord disables
mentions; Slack uses plain-text blocks. The induced test delivers all five rules
to a temporary loopback receiver and checks restart suppression.

## Phase 4 workbench

Use Node 22.22 or later. Install the pinned web dependencies with
`npm --prefix web ci --ignore-scripts --no-audit --no-fund`. The workbench imports
the checked-in TypeScript SDK source; it needs no provider credentials.

Start an isolated offline daemon in one terminal:

```sh
target/debug/alightd --mode sim --seed 42 --sim-canaries 900 --bind 127.0.0.1:8080 --db .alight/workbench-sim.db --operator-key-file .alight/operator.key
```

The operator key file must contain an existing local API operator key, be private
to its owner, and contain no wallet/provider key. Omit the key flag for public
read-only use. Sim does not load project ENV or use providers/signing access.
In another terminal, run `npm --prefix web run dev`, then open
`http://127.0.0.1:5173`. For a different daemon port, set `ALIGHT_API_ORIGIN` on
the Vite process. Vite binds loopback and allows only `web/` and SDK source files.

Cockpit, Quote, Prove, Ledger, Regimes, Health, and Tape all retain the source
badge. Hash routes can recover an issued forecast or Prove run on reload.
Operator access is optional, held only in page memory, and cleared on reload or
disconnect. Public SDK reads omit its Authorization header. Only quote freeze
and governed Prove send authenticated POSTs; uncertain responses require history
reconciliation rather than automatic retries. Sim Prove is synchronous: dots
represent actual members, without staged synthetic progress animation.

The stream reconnects with bounded backoff and pauses when the document is hidden.
Evidence refreshes every 15 seconds without overlapping requests. Source or
schema mismatches clear evidence and disable actions. Both themes, native
dialogs, command navigation (`Ctrl/Cmd K`), keyboard focus, data-table alternatives,
and reduced-motion preferences are supported. Fonts and the Aftershock logo are
served locally. Charts show stored evidence; missing market/history services
produce explicit unavailable states.

`npm --prefix web run build` produces `web/dist`. Compose's Caddy service now
builds `deploy/Web.Dockerfile` to serve these assets and the allowed API routes
from one origin, with CSP and no external font/script dependencies. Build only
the web image with `docker compose -f deploy/compose.yaml build caddy` when
needed; this command does not resume collection. Starting the stack or resuming
the collector remains a separate live operation under the standing pause.
Rebuild the collector image as well when deploying backend changes; the new web
contracts require the matching daemon. Building images does not start services.

Verification:

```sh
npm --prefix web run lint
npm --prefix web test
npm --prefix web run build
cd web
npx playwright install --with-deps chromium
npm run test:e2e
```

For an already installed Chrome, set `ALIGHT_BROWSER_EXECUTABLE` to its absolute
path on the E2E command. Browser tests create/stop their own temporary Sim daemon
on 8082 and Vite on 5180; both ports must be free. No real key, project ENV,
provider request, wallet funding, or chain send is needed. Reports and screenshots
are local under `.alight/phase5/`; CI uses bundled Chromium. The test checks
actual API values, a frozen forecast, 40 held-out members, restart recovery,
browser SHA-256 verification/tamper rejection, 15 Axe audits (14 screen/theme
audits and the diagnostics disclosure), mobile
keyboard navigation, reduced motion, offline/source mismatch, and probability-only
market fallback. See `docs/phase-4-report.md` for measured scope and remaining
live/dependency acceptance.

## Phase 5 diagnostics and finite history jobs

Collection remains paused. These commands implement offline acceptance without
loading signing keys or making provider requests:

```sh
cargo run -p alight -- regime-sim --database .alight/phase5/regime-sim.db --seed 42 --output .alight/phase5/regime-sim.json
cargo run -p alight -- backfill --database .alight/phase5/history.db --input sanitized-history.json --output .alight/phase5/history-report.json
```

`regime-sim` stores 240 explicit synthetic signal windows with two injected changes.
It is separate from default canary training. For the complete offline UI demo use
an isolated database and `alightd --mode sim --sim-regimes true`: historical
synthetic changes precede freshly generated, separately identified training
samples. It creates no provider frames or market tape. Do not reuse the Live DB.
`alight diagnostics --api http://127.0.0.1:8081 --source sim` reads saved diagnostics;
the Rust and TypeScript SDKs expose the same read-only operation.

The observe/live daemon now runs diagnostics inside its existing supervised
lifetime: sealed clock bins, current reference-cell landing rates, observer
comparison, bounded finalized block sampling, checkpointed detection and alerts.
It does not run until the operator explicitly resumes collection. Optional
`SOLAMI_WEBHOOK_SECRET` enables authenticated ingress; no webhook is registered
by this code. Optional `ALIGHT_SLOT_COMPUTE_LIMIT` and
`ALIGHT_SLOT_COMPUTE_LIMIT_PROVENANCE` must be supplied together with a verified
capacity and its evidence. With neither set, fullness is unavailable.

An explicitly requested future network backfill uses `alight backfill --network`
with `--database`, `--days 42` through `56`, `--requests 64` through `1024`, and
optional `--output`. That finite read-only job checks mainnet, retained first
block and finalized tip, then samples chain block timestamps within its request
budget. It does not sign, send, register webhooks or leave streams running. Its
200 ms scheduling assumption selects candidate slots only; timestamp evidence
determines actual coverage. Null/error responses and candidate conflicts remain
missing. Sparse averages cannot resolve every short event. Use a separate Replay
database. No such network backfill has been executed for Phase 5 offline acceptance.

Regime actions reset old effective sample mass and enable uniform exploration
for 30 minutes. The daily/burst governor and wallet reserve remain unchanged.
An empty new regime must collect enough fresh support to quote again. Named
upgrades require independent technical evidence; epoch dates alone never establish
Alpenglow or a slot-time step. See `docs/phase5-methodology.md` for thresholds.
