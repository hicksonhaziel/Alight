# Phase 2 progress — model and simulator foundation

Verified 5 October 2026. Phase 2 has begun; it is not complete. The live collector
and proxy remain stopped, the legacy service is inactive, and no live canary has
been sent. Offline work needs no wallet funds or provider environment values.

## Delivered

The methodology and prospective Signal analysis were committed first in
`2f4450c9f63412665c34a31b4a750c6e57a6ff88`, before implementation reports. Every
curve/report records the registered methodology's SHA-256. Simulation clock dates
are artificial; registration order uses actual Git and validation timestamps.

`alight-model` implements M0: one exponentially decayed Beta posterior per exact
route/tip/fee/size/CU/regime cell, with a 95% equal-tail interval and discounted
sample count. Only owned, finalized, resolved, source-matched outcomes known at
evaluation time are eligible. Pending outcomes remain visible. Fewer than 30
effective samples, stale data, or too many unresolved assignments returns
INSUFFICIENT with reasons and a fresh-sample requirement. Ambient transactions do
not establish a probability denominator. Empty live input cannot yield a live
recommendation.

`alight-sim` generates seeded, source=sim Canary records using the existing
81-cell assignment grid and logged propensities. The artificial environment
depends on route, tip, fee, workload size, leader class, and congestion. Optional
shifts change slot duration and congestion and create an explicit new regime.
This supplies a known change for testing; it is not a regime detector. A flat-tip
environment is available for the future Signal null test.

The CLI now runs simulation and model replay. SQLite migration 0004 stores
content-addressed snapshots by source, regime and time; repeated identical rows
are idempotent and reads detect altered payloads. Store training queries carry
the finalization flag. Simulation uses a separate database under `.alight/`;
the paused live database has not been migrated or modified by these commands.
The periodic daemon job and forecast ledger are still to be implemented.

## Verification

Formatting, all-target workspace Clippy with warnings denied, and all **40**
workspace tests passed with locked, cached dependencies. The eight new tests
cover the estimator's intervals/decay/eligibility, deterministic simulation,
registered convergence/re-adaptation, snapshot persistence, and CLI replay.
The CLI rejects an altered saved curve; this is not forecast-ledger verification.
The existing Phase 0/1 evidence checks and credential scan also passed.

Seed 42 with 8,100 simulated canaries produced **243 measured snapshots**
(81 cells times three horizons). All 243 rows persisted, replay generated the
same report bytes, and the separate SQLite database passed quick_check. The
simulation and replay each made zero network requests and sent zero transactions.

Registered accuracy bound: mean absolute error <= 0.08 among measured curves,
after 12,000 initial canaries or 12,000 canaries following the injected change.

| Seed | Initial MAE | After-change MAE | After-change measured curves |
|---|---:|---:|---:|
| 7 | 0.021495 | 0.031266 | 243 / 243 |
| 42 | 0.018729 | 0.029614 | 240 / 243 |
| 2026 | 0.018322 | 0.026171 | 243 / 243 |

All six registered M0 checks passed. Seed 42's after-change run correctly refused
one stale cell at all three horizons; the registered metric excludes insufficient
evidence. These synthetic results do not measure real Beam, RPC, or Solana behavior.
Regime IDs were supplied by the simulator; automatic detection and selective
exploration are later work. See the sanitized
[validation receipt](../data/phase-2-foundation-validation.json) and
[actual curve example](../data/fixtures/phase2_curve_snapshot.json).

## Run locally

From the project root with the built CLI:

```sh
target/debug/alight sim --seed 42 --canaries 8100
target/debug/alight replay-model --input .alight/phase2-sim.json
```

The report, replay and separate simulation database default to `.alight/` and
stay ignored. Custom paths use --output and --database. A change experiment adds
`--shift-at 12000 --shift-slot-ms 400 --shift-congestion 1 --canaries 24000`.
Exit 0 means success; invalid input or replay disagreement exits 2. Neither
command loads `.env`. Existing doctor behavior is unchanged.

## Next work and provider dependency

Next: M1 pooled monotone model, M2 performance-weighted blending, then the adaptive
policy, immutable forecast chain, grader and baseline comparisons. Periodic
snapshots, latency quantiles/targeted quotes and the daily Signal job also remain.
The pre-registration is a specification for these components, not a claim that
they are implemented. The API/SDK quote service follows in Phase 3.

Beam HTTP remains a separate live dependency. The documented hostname returned
NXDOMAIN locally and DNS status 3/no answers through Google on 5 October; the
QUIC hostname resolves. Ask Solami to confirm the supported HTTP URL, auth and
request format using the [prepared Telegram message](beam-support-message.md).
No support message has been sent. HTTP can be modeled artificially while its
real endpoint is unavailable, but no live HTTP result is claimed. Owned live
validation also requires funding the dedicated wallet. The collector stays
paused under Hickson's data-saving instruction.
