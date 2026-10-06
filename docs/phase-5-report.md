# Phase 5 — regimes, observers and alerts

Implementation and offline acceptance passed on 6–7 October 2026. Full Phase 5
acceptance remains pending Live measurements. Collection stayed paused throughout
this work: zero provider requests, zero owned Live transactions and no webhook
registration. The isolated Sim preview is at `http://127.0.0.1:8180/#regimes`.

## What changed

The new diagnostics crate stores timestamped signal windows, detector checkpoints,
regime changes, observer comparisons, disagreement events and history reports.
Migration `0010_phase5_diagnostics.sql` extends the existing SQLite store. Public
reads use `GET /v1/diagnostics`; Rust, TypeScript, the CLI and generated OpenAPI
share its source-scoped contract. Reads return saved evidence without contacting
providers or running analysis on demand.

| Task | Implemented behavior | Verified evidence and remaining acceptance |
|---|---|---|
| 5.1 | Clock mean/p95, skip/dead rate, reference landing rate, sampled fullness, non-vote share and observer lag, each with UTC window, sample count, provenance and unavailable reason | Extractor tests cover coarse chain seconds, missing metadata, fork exclusion and verified-capacity requirements. No Live signal population was collected. |
| 5.2 | Bounded BOCPD, CUSUM baseline, independent-family voting and epoch annotations | Seed 42 detected both injected shifts after two windows each. Twenty stationary seeds × 600 windows produced zero false changes. Checkpoint restart, duplicate delivery and observer-only perturbation checks passed. |
| 5.3 | Finite 42–56 day read-only history job, normalized-file replay and explicit retention/sampling report | Synthetic chain timestamps spanning about 53 days produced all three 400→350→300→250 ms changes. The report correctly marked incomplete 56-day coverage. Real mainnet history and retention remain unmeasured. |
| 5.4 | Atomic regime record, old effective sample mass cap, bounded uniform exploration, append-only forecast voiding and alert persistence | A frozen claim and Prove run crossed a synthetic change: both became VOIDED; original claim bytes and ledger chain remained valid; old sample support became zero; uniform exploration and the alert were present. |
| 5.5 | Existing gRPC/Mirage/RPC evidence plus authenticated Webhook ingress | Captured Webhook payload replay, exact-body HMAC verification, ownership filtering and API authentication tests passed. Four-observer Live agreement remains pending. |
| 5.6 | Persisted conflicts/missing evidence, comparable receive/send lag, pair agreement, incomplete counts and alerts | Dropping one observer from three owned fixture canaries produced visible persisted missing-observer events and a disagreement alert. Different clock origins produced unavailable lag. |
| 5.7 | Candidate identity keyed by slot and block; conservative LANDED_THEN_DROPPED finalization | Duplicate-slot/different-block replay passed. Finalization requires canonical exclusion of all candidates, finalized covered RPC absence and expiry of the transaction's validity window. |
| 5.8 | Supervised evidence-driven watch with firing signals in every regime record | Synthetic clock-only detection passed; observer-only movement did not rename a network regime. Epochs annotate events and do not identify Alpenglow or any upgrade. No new Live upgrade claim was made. |
| 5.9 | Same-hour, equal-paid-tip and identical-scope position comparison with exclusions and displayed limits | Synthetic tests cover deduplication, owned/fork exclusion, incompatible scopes and multiple-recipient totals. A regression with 101 owned canaries verifies that owned signatures beyond the latest 100 displayed rows remain excluded. Live tape fidelity remains pending. |

These detector measurements describe the registered synthetic scenarios. They do
not establish Live sensitivity, predictive performance or validator independence.
Thresholds, the 256-state approximation and the claims policy are recorded in
`docs/phase5-methodology.md`.

## Pages and interaction

**Regimes** now contains a seven-signal selector, a chart whose missing values
break the line, and expandable exact-value/provenance tables. Persisted changes
show their firing signals, BOCPD short-run probability, CUSUM baseline, epoch
context and automatic actions. Historical coverage has its own explicit pending
or limited state. Sim changes carry both source and simulation labels.

**Observer health** now shows per-observer receive/send lag distributions, missing
evidence, conflicts and incomparable clocks. Pair agreement requires complete
slot, block identity and result evidence; incomplete comparisons are shown
separately. Persisted diagnostic events link to original owned-canary evidence.
The page also shows source-scoped alert history and the existing gap episodes.
Sim creates no imitation provider deliveries.

**Tape & receipts** now includes the workload-fidelity table, raw median positions,
sample counts and exclusions. Observed known-recipient payments are totaled per
signature and index scope, with duplicate observer reports removed. Query limits
may truncate multi-recipient totals. Passive finalization, route, workload,
contention and block fullness remain uncontrolled. Empty Sim market evidence
stays empty. The Phase 6 wallet-receipt engine is still a later task.

Cockpit, Quote, Prove and Forecast ledger retain their existing flow. New regime
context prevents freezing a stale claim. A changed regime refuses unsupported
recommendations until fresh samples arrive and voids crossing forecasts without
rewriting their immutable payloads. The Aftershock brand assets, palette, local
fonts, both themes, keyboard controls and reduced-motion behavior are retained.

## Operational behavior

Observe/Live diagnostics run only inside the existing daemon lifetime. They use
sealed clock bins and bounded sampling; they remain stopped under the current
collection pause. Fullness is unavailable unless both a verified per-slot CU
capacity and its provenance are configured. Optional Webhook ingress verifies
HMAC-SHA256 over the exact request bytes, bounds body size and request rate,
accepts owned signatures only, and is disabled in Sim/Replay. The captured
Webhook contract lacks block identity and index, so its records cannot establish
complete candidate agreement on their own.

Regime changes and detector checkpoints commit in one SQLite transaction. Active
changes exclude historical backfill. Uniform exploration lasts 30 minutes under
the existing daily/burst governor and wallet reserve. Original labels remain
immutable. Forecast voiding scores at most 10,000 crossing forecasts individually;
additional crossing forecasts still become VOIDED with scores unavailable.

Alerts persist locally before delivery. Outbound delivery still requires an
explicit configured endpoint and was not exercised here. A durable ATTEMPTING
claim prevents duplicate dispatch; an uncertain interrupted attempt is retained
without automatic retry. Historical Replay changes cannot reset Live training
or enable Live exploration.

## Verification

| Check | Result |
|---|---|
| Rust workspace with Beam feature | 98 passed; zero failed |
| Format and all-target Beam Clippy | PASS |
| Rust OpenAPI versus generated SDK snapshot | Exact byte match |
| TypeScript SDK | Lint/generator PASS; four tests passed |
| Web | Lint/build PASS; three unit tests passed |
| Browser | Seven tests passed; zero retries or failures |
| Accessibility | Fifteen Axe audits; zero violations |
| Developer flow | CLI, Rust SDK and TypeScript SDK each completed 40 Sim members and verified the ledger |
| Browser integrity | Export verification, tampered export rejection and copied TypeScript compilation passed |
| Packaged Caddy proxy | Static assets/CSP, diagnostics, operator authentication, disabled Sim Webhook and endpoint allowlist passed |
| Historical receipts | Phase 3 and Phase 4 publishing snapshots still verify |
| Repository scan and Compose configuration | PASS; private briefs and known credentials excluded |

Both runtime images are built from the Phase 5 sources. The packaged web preview
uses a fresh isolated Sim database and loopback-only ports. The Live Compose
stack was not started. The portable collector build compiled the release binary,
but the fresh Debian runtime package download stalled. Local packaging reused
the previously verified Debian runtime image and replaced its binary with the
current release build; an isolated, network-disabled Sim smoke check verified it.
The web image used the existing public npm cache. `data/phase-5-validation.json` records the sanitized
checks, limits and code hashes; `scripts/verify_phase5_receipt.py` verifies the
snapshot that publishes those bytes.

## Remaining Live acceptance

Run a bounded chain-history job after an explicit resume/read authorization and
record actual retained coverage, even if it cannot reveal the historical steps.
After funding and an explicit collection resume, validate governed owned canaries
across the configured observers and compare available Live tape at equal tips and
index scope. Resolve any provider contract limits without inventing missing block
identities. Funding has not been checked here and no transaction budget has been
increased. The saved Phase 3 funded N=40 acceptance also remains pending.

Use `docs/operations.md` for the finite offline commands and future deployment
procedure. The receipt reports `OFFLINE_PASS_LIVE_PENDING` and
`phase_complete=false` until those measured checks exist.
