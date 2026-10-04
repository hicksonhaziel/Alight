# Agent instructions for Alight

## Start here

- Owner: Hickson. Project: Alight, a Solana transaction-landing observatory,
  calibrated quote service, and developer SDK built around Solami data.
- Read `context.txt` when present, then `ALIGHT_PLAN.md` for the product rationale
  and `ALIGHT_BUILD.md` for task IDs, dependencies, and acceptance criteria.
- The planning documents describe intended behavior, not a completed product or
  proof of current provider capabilities. Validate external assumptions in Phase 0.
- If the local briefs are absent in a fresh clone, use the committed project docs
  and request missing requirements only when they are necessary for the task.
- Work within the user's current request. Phase 0 implementation is authorized.
  Hickson explicitly permits reading and editing project environment files for
  debugging, including reusing Aftershock's Solami configuration in Alight.

## Git and context rules

- Never stage, commit, force-add, or push `ALIGHT_PLAN.md` or `ALIGHT_BUILD.md`.
  Preserve both as local planning documents; do not alter them without a request.
- Keep their root paths in `.gitignore`. Before any commit or push, check
  `git ls-files -- ALIGHT_PLAN.md ALIGHT_BUILD.md` and the staged file list.
  Neither planning file may be tracked or staged.
- Keep `context.txt` local and ignored. Update it when Hickson asks to save context
  or prepare a handoff for another chat; do not rely on chat history alone.
- A handoff records the current objective, explicit user constraints, actual
  changes, branch and repository state, checks and results, decisions, unresolved
  dependencies, and the next concrete step. Separate completed work from plans.
- Credentials, private keys, and authenticated URLs may be stored in the ignored
  local `.env`, as authorized by Hickson. Never expose them in context, chat,
  versioned files, logs, fixtures, or exports. Runtime code uses environment values.
- Hickson requests all work on `main`, with no extra project branches. Commit and
  push coherent verified batches directly to `main`.
- Hickson authorizes regular commits and pushes to this repository. Save coherent
  verified batches as work progresses; avoid tiny noisy commits and large unsaved
  change piles. Inspect the staged file set and check for credentials before each push.
- Leave README.md unchanged until Hickson asks to work on it again.
- Aftershock belongs to the same owner. A separate third-party notice for reused
  owner-controlled code was removed at Hickson's request.

## Architecture and dependency order

- Core: Rust workspace, Tokio daemon `alightd`, SQLite WAL through sqlx,
  axum REST/WebSocket API, and a CLI. Pin the stable toolchain and dependencies
  after the integration spikes establish compatibility.
- Web: React, Vite, and TypeScript in `web/`; TypeScript SDK in `sdk/ts/`;
  Rust SDK in `crates/alight-client/`. Deploy through Docker Compose and Caddy.
- Shared contracts belong in `crates/alight-types/`. Document changes with an
  example payload, any required migration, and an update to `docs/contracts.md`.
- Hickson or the designated integration owner coordinates shared types,
  migrations, lockfiles, CI, and integration. Respect assigned crate boundaries.
- Begin with Phase 0 integration evidence and contracts. Then build and run the
  Phase 1 collector, governor, resolver, and persistence as early as possible.
- Later dependencies are models, simulator, quotes and the forecast ledger,
  Prove, regime detection, observer comparison, economics, receipts, and anchoring.
  The full product remains the target; sequencing preserves its dependencies.
- Reuse Aftershock components only after inspecting their suitability. Its
  Postgres and job-lease control plane does not belong in Alight.

## Correctness and evidence

- Only `crates/alight-canary/` may sign or send transactions, through the budget
  governor. Enforce daily and burst caps, safe defaults, and restart-safe spend.
- Outcomes are `LANDED_OK`, `LANDED_FAILED`, `LANDED_THEN_DROPPED`, `EXPIRED`,
  `REJECTED`, and `UNRESOLVED`. A timeout is never an outcome.
- `EXPIRED` requires the last valid block height to have passed and an RPC check
  proving signature absence at the required commitment. Preserve unresolved
  canaries across restarts and retain observer disagreement as evidence.
- Key landing records by `(slot, block identity)`. Measure slot time and use the
  current regime for conversions between slots and milliseconds.
- Mark data and UI modes clearly: `live`, `sim`, or `replay`. `observe` sends no
  transactions; `sim` requires no network or keys; `replay` sends no transactions.
- Record random seeds, policy IDs, assignment probabilities, and the uniform
  exploration arm. Reproduce simulator and replay results deterministically.
- Build provider adapters against sanitized real spike responses. Do not invent
  Blur, Mirage, Webhook, or Data API payloads, or assume dashboard endpoints.
- Every quote carries evidence class, interval, effective sample count, age,
  size class, route, region, and observation window. Refuse a recommendation when
  evidence is insufficient. Canary results do not establish real-swap performance.
- Pre-register the Signal analysis before inspecting live results. Publish
  `DISCRIMINATING`, `FLAT`, or `INCONCLUSIVE` honestly, including null results.
- Forecasts are immutable, hash-chained, and graded on later held-out outcomes.
  Show unfavorable Prove verdicts and forecasts voided by regime changes.
- Market economics are conditional estimates; expose their assumptions and fall
  back to probability-only quotes when Blur data are stale or sparse.
- Keep provider keys server-side and public endpoints read-only and rate-limited.
  Operator controls require authentication. Redact logs, fixtures, and exports.

## Verification and reporting

- Use the task's acceptance command and include its actual result in the handoff.
  Do not claim live integration, measured performance, or test success without it.
- Once the relevant workspace and scripts exist, run before a PR:
  `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`,
  `npm --prefix web run lint`, and `npm --prefix web test`.
- Use meaningful governor and resolver tests, recorded-fixture replay, estimator
  property tests, seeded convergence and regime tests, ledger tamper checks, and
  end-to-end quote -> lock -> prove -> ledger verification as components arrive.
- Record contradictions found in spikes in `docs/decisions.md` and resolve the
  affected design before proceeding with dependent implementation.
- Use typed Rust errors; avoid `unwrap` in production paths. Public function
  documentation states units: ms, slots, lamports, and micro-lamports per CU.
- End each phase with an evidence report and sanitized validation receipt as
  specified in the build brief. Distinguish observed facts from intended behavior.
