# Quick start

Explore how Alight turns landing measurements into a quote, freezes the claim
and tests it on held-out canaries. Start with synthetic evidence, then optionally
connect your own Solami read key. Sim needs no provider account, wallet or SOL.
Dependency installation downloads packages; the Sim runtime makes no network
requests.

## 1. Prepare

Use a Bash/Unix environment with Git, the pinned **Rust 1.96.1** toolchain,
**Node 22.22+**, and **Python 3**.
Native builds also need a C/C++ compiler, Make, CMake, `pkg-config` and CA
certificates. On Debian/Ubuntu the native packages are:

```sh
sudo apt-get install cmake make g++ pkg-config ca-certificates
```

Use the equivalent packages on your platform. Clone the repository if needed:

```sh
git clone https://github.com/hicksonhaziel/Alight.git
cd Alight
```

Already cloned? Start from your existing repository directory and skip `git clone`
and `cd Alight`. All commands below run from that directory.

## 2. Start everything

From the repository root, run:

```sh
./scripts/quickstart.sh
```

The launcher checks prerequisites and free ports, builds with one Cargo job,
installs pinned web dependencies when needed, and starts a populated seeded Sim
API plus the Workbench. Open [localhost:5173](http://localhost:5173); the badge
reads **SIMULATED**. Press `Ctrl+C` to stop both child processes.

The API binds `127.0.0.1:8081`; the web binds `127.0.0.1:5173`. The launcher
refuses occupied ports and never attaches to an existing Live server. It does
not modify `.env` or start a funded sender. Its private logs, seed-42 database
and local API key stay under ignored `.alight/quickstart/`.

For custom ports or a finite session with already-built dependencies:

```sh
./scripts/quickstart.sh --help
./scripts/quickstart.sh --skip-build --api-port 8083 --web-port 5174 --run-for 30
```

`--skip-build` is for local verification: it requires existing debug binaries
and web dependencies and does not check whether they reflect changed sources.
The normal command checks the build every time. Seed and workload settings are
fixed for the launcher's database; use [manual operations](operations.md) for
other experiments.

## 3. Optionally freeze a quote and run synthetic Prove

The launcher creates a private **API operator key** at
`.alight/quickstart/operator.key`, preserving an existing private key. This
grants local API actions; it is separate from wallet/provider credentials and
is never printed in the launcher's output.

Open that file locally and enter its contents in **Operator access** in the
Workbench. Open **Quote & Prove**, choose a supported target, lock the forecast and
start Prove. The claim and model are frozen before its held-out outcomes; the
result may be CONSISTENT, INCONSISTENT or INCONCLUSIVE. All evidence and outcomes
in this session remain synthetic. The browser clears operator access on reload
or disconnect.

## 4. Optionally use your own Solami key

Stop the launcher first. Copy the environment template without replacing an
existing configuration:

```sh
test -e .env || cp .env.example .env
```

Fill in these values in ignored `.env` from your Solami dashboard:

| Variable | Value |
| --- | --- |
| `SOLAMI_RPC_URL` | Your HTTPS mainnet RPC endpoint |
| `SOLAMI_RPC_TOKEN` | RPC read credential, when required by the endpoint |
| `SOLAMI_GRPC_URL` | Your HTTPS gRPC endpoint |
| `SOLAMI_GRPC_TOKEN` or `SOLAMI_API_KEY` | Credential with gRPC read access |
| `SOLAMI_DATA_API_URL` | Keep the template's metadata API base URL |

Start in **Observe**, with its own database:

```sh
target/debug/alight doctor --mode observe --network
target/debug/alight run --mode observe --db .alight/local/observe.db --bind 127.0.0.1:8081
```

The doctor performs one read-only RPC check; it does not verify a working stream.
In a second terminal, start the web with the same API port:

```sh
ALIGHT_API_ORIGIN=http://127.0.0.1:8081 npm --prefix web run dev
```

Open the page. The badge reads **LIVE**
because observations come from mainnet; Observe loads no signing identity and
sends no transactions. Provider bandwidth/access charges can still apply.
Mirage, Blur, Data API wallet history and Webhook permissions are configured
separately in [operations](operations.md).

A fresh Observe database has no owned canary training evidence. It can show
healthy streams while quotes remain INSUFFICIENT. Live quotes require enough
recent governed canaries; passive collection does not refresh that support.
Read the [budget and Live controls](operations.md#observe-and-live) before
deliberately enabling sends with a dedicated funded wallet.

## If something is unavailable

- **Port already in use:** stop the duplicate process, or choose the launcher's
  `--api-port` and `--web-port` options. It prints the actual Workbench URL.
- **Database locked:** run only one daemon per database; choose a separate path
  for another session.
- **Operator access disabled:** reconnect the local API key in the Workbench;
  the launcher enables it only for its own Sim server.
- **Live evidence missing:** inspect **Network health** and provider permissions.
  A connection or successful doctor check does not guarantee fresh observations.

Continue with [CLI/SDK commands and deterministic replay](operations.md),
[architecture](architecture.md), or the [documentation index](README.md).
