# Phase 2 — software completed; funded live acceptance pending

Verified 5 October 2026. All Phase 2 software tasks (2.1–2.14) are implemented
and pass offline acceptance. The formal live exit condition remains open: the
wallet is unfunded, no owned live canary has been sent, and Beam HTTP needs
provider confirmation. The collector, proxy and legacy service remain stopped.
The live database was not modified. No network stream was resumed.

## Delivered

- M0 decayed Beta cell estimates; M1 monotone pooled GLM with penalized
  leader/congestion effects; M2 temporal-performance blend and all four evidence
  classes. Sparse, stale, unresolved or extrapolated evidence cannot produce an
  unsupported recommendation.
- Cost-weighted Thompson-style policy v1 with at least 30% uniform exploration,
  logged conditional propensities and deterministic restart. Signing/sending still
  requires the existing budget governor in `alight-canary`.
- Immutable per-source forecast hash chains, durable heads, frozen model documents,
  tamper verification and later-canary grading. Grades include reliability, Brier,
  log loss, interval coverage and separately identified through-change scores.
- B1 minimum tip, B2 five times minimum, B3 the preceding five-minute passive-tape
  median and B4 a frozen model. B3 checks source, transaction uniqueness and time;
  without a fresh tape it is unavailable. Passive observations never become owned
  model training labels. The live tape collector arrives in Phase 3.
- Seeded simulator/replay, periodic history and grader jobs (every five minutes),
  decayed latency quantiles, probability/slot/millisecond quote targets, and a
  stored daily Signal report for the previous completed UTC day. Manual same-day
  Signal reports are explicitly provisional.
- Daily Signal publishes DISCRIMINATING, FLAT or INCONCLUSIVE for each route/size,
  with uniform-arm primary analysis, IPW sensitivity, exclusions and registered
  clustered intervals. Empty live input remains insufficient.

No new documentation files were added. This report, the existing contracts and
methodology were updated. Results are in the compact
[validation receipt](../data/phase-2-validation.json); the
[historical foundation receipt](../data/phase-2-foundation-validation.json) and
its v1 curve fixture remain unchanged. README and local planning/handoff files
were left unchanged and uncommitted.

## Verification and actual results

Formatting, all-target workspace Clippy (Beam feature, warnings denied), all
**47 workspace tests**, credential scanning and the Phase 0/1 receipt checks
passed. GitHub CI also passed for implementation commit `13db9a0`. These checks include governor/resolver protections, synthetic convergence
and adaptation, forecast/frozen-model tampering, hand-computed scores and complete
CLI replay. No new UI exists yet, so there are no web-package checks to run.

The seed-42 lifecycle run used 8,100 synthetic canaries, produced 243 measured M0
curves, locked two forecasts (probability and p50 latency), and graded both SCORED
against later canaries. Replay reproduced the complete JSON report byte for byte.
History writes persisted and SQLite quick_check returned `ok`. Additional CLI
checks covered millisecond targets, grading, Signal, chain verification and refusal
with empty live input. The latter wrote a no-claim test entry in the separate
simulation database, never into the paused live database. All simulation/CLI
checks sent zero transactions and made zero provider requests.

M1 beat M0 on 2,000 later canaries at each of three horizons (6,000 predictions
per seed), while preserving monotonicity in tip and fee:

| Seed | M0 held-out log loss | M1 held-out log loss |
|---|---:|---:|
| 7 | 0.317141 | 0.309355 |
| 42 | 0.327095 | 0.318817 |
| 2026 | 0.337159 | 0.328876 |

At exactly 2 SOL **synthetic** spend, policy v1 had lower mean regret for every
registered seed. Utility is landing probability within one slot minus nominal
fee-plus-tip/500,000 lamports. The last budget point is linearly interpolated.
Decision counts and cumulative regret are disclosed; the latter is approximate
from rounded logged values. This benchmark does not establish live savings.

| Seed | Mean regret v0 / v1 | Decisions v0 / v1 | Cumulative regret v0 / v1 |
|---|---:|---:|---:|
| 7 | 0.669065 / 0.216310 | 4,862.9 / 14,377.8 | 3,253.6 / 3,110.1 |
| 42 | 0.659923 / 0.229789 | 4,949.4 / 12,788.0 | 3,266.2 / 2,938.5 |
| 2026 | 0.668154 / 0.216795 | 4,877.9 / 10,992.7 | 3,259.2 / 2,383.2 |

The probability forecast chose the small Beam HTTP minimum-tip configuration.
Its later-canary Brier score was 0.125646 and log loss 0.418381 (34 outcomes).
The seeded baseline comparison was:

| Baseline | Tip (lamports) | Later outcomes | Brier | Log loss |
|---|---:|---:|---:|---:|
| B1 | 100,000 | 34 | 0.125646 | 0.418381 |
| B2 | 500,000 | 38 | 0.097527 | 0.364366 |
| B3 | 200,000 | 33 | 0.058028 | 0.235917 |
| B4 | 100,000 | 34 | 0.126065 | 0.419884 |

B3 used three explicitly artificial source=sim tape transactions. B4 used a
model frozen earlier in the same run. Each alternative has its own later canary
denominator; this comparison is not a causal estimate of trading performance.

Across fixed seeds 0–99, the latency intervals covered the known continuous truth
in **91/100 p50** and **97/100 p90** datasets, inside the registered 90–99 range.
Unidentifiable failure mass was refused. The p99 sample requirement now preserves
the 20-effective-tail-observation minimum instead of being capped at 30 records.
The focused 100,000-canary Signal experiments recovered the known tip slope as
DISCRIMINATING and the zero tip/fee-effect null as FLAT; absent cohorts remained
INCONCLUSIVE. These are synthetic checks, not real Beam/RPC measurements.

## Registration, corrections and limitations

The original methodology was committed in `2f4450c` before foundation reports.
The prospective v2 amendment was committed in `88a77fb` before corrected reports
or owned live analysis. Current reports bind SHA-256
`0da0241b9ce652df77f7b9ee40a713f7bfa93e3c62207c28e290da3d1c9a3f86`. Artificial simulator timestamps are not
registration timestamps; actual Git and validation timestamps establish order.

The initial percentile-only latency coverage failed for p50 (88/100). The
existing methodology discloses this result and the conservative bootstrap/order-
statistic correction. The fixed seeds and acceptance range were preserved.
Intervals under decay/nonstationarity remain approximate. Signal's bootstrap
uses one-step score updates and removes unidentified constant/collinear controls.

Sender/observer monotonic origins previously differed. They now share a process
origin and change identity on restart. Historical clock identities are preserved;
old incomparable measurements cannot become millisecond evidence retrospectively.
Regime changes are explicit simulator inputs; automatic detection is later work.
Simulator record IDs now separate different environments even when seeds match.
UTC day accounting also handles timestamp offsets correctly.

The Phase 2 container was rebuilt with cached dependencies and offline Cargo.
Its network-disabled replay normalized 24 slot events and eight blocks, with
zero canaries sent. The build context now includes only the public methodology
file from `docs/`, needed by the model; private files remain excluded. Compose
uses `alight-collector:phase2`, and its configuration validated. The new image is
ready locally, while the existing collector/proxy containers remain stopped.

## Use and remaining live acceptance

```sh
target/debug/alight sim --seed 42 --canaries 8100 --phase2 \
  --output .alight/my-run/sim.json --database .alight/my-run/sim.db
target/debug/alight replay-model --input .alight/my-run/sim.json \
  --output .alight/my-run/replay.json --database .alight/my-run/sim.db
target/debug/alight ledger verify --source sim --database .alight/my-run/sim.db
```

`quote`, `model-tick`, `grade` and `signal` use an explicit database and source;
quote input is the shared JSON request documented in [contracts](contracts.md).
These commands need no `.env`, keys or network. Outputs refuse to overwrite a
historical report with different bytes. Runtime artifacts stay under ignored
`.alight/`, outside `docs/`.

Funded, resolved owned canaries are still needed to validate live quotes and
scoring. The last recorded Beam HTTP DNS check on 5 October returned NXDOMAIN;
QUIC resolved. Solami must confirm the supported HTTP endpoint/auth/request
format before an HTTP send can be validated. No support message was sent.
The collector stays paused until Hickson asks to resume it. Phase 3 is next:
economics, passive tape ingestion, the REST/WebSocket API and SDKs.
