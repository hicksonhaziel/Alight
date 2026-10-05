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

Caddy exposes only read-only health, clock, and leader endpoints on
`127.0.0.1:8080`. Request headers, URI/query strings, and client addresses are
removed from its access logs. The backend shares a 10-request/s limiter. This
local deployment uses HTTP; a public hostname and TLS belong to a later VPS
configuration. The collector uses a private container network, an unprivileged
UID, a read-only root filesystem, and no additional Linux capabilities. Caddy
retains only NET_BIND_SERVICE, required by its official binary's file capability.
The collector has a bounded 256 MiB temporary workspace for SQLite index creation.
Full epoch schedule responses have a separate bounded 45-second deadline.

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
recipient itself. The published Beam HTTP hostname failed DNS resolution during
setup; its JSON-RPC adapter remains unverified by a funded provider send. A
connection failure is retained as uncertainty. Do not use it as a successful
route in reports until a real canary lands and observers confirm it.
