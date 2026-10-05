# Alight methodology — preregistration v1

Registered 5 October 2026, before any owned live canary results or Signal report.
The Git commit containing this file establishes registration order. Reports must
record SHA-256 of these exact UTF-8 bytes. Changes to an analysis require a new
version, prospective registration, and disclosure; never overwrite an old report.

This document specifies Phase 2, including work that is still to be implemented.
Implemented components and actual acceptance results belong in the phase report.

## Population, clocks, and outcomes

The population is owned canary-class transactions, separated by source (`live`,
`sim`, `replay`), route, exact tip lamports, exact CU price, CU limit, size class,
region, and regime. Passive observations of other people's landed transactions
are never denominators or model-training outcomes. Simulations spend no SOL and
provide no evidence of provider performance. HTTP simulation does not establish
that the real Beam HTTP service works.

The primary endpoint is finalized landing within H slots, H in {1, 2, 4}.
Slot distance is landed_slot minus sent_slot; distance zero qualifies. Both
LANDED_OK and LANDED_FAILED count as landing: execution success is separate.
EXPIRED, REJECTED, and LANDED_THEN_DROPPED count as zero. An expiry needs the
existing resolver's block-height and history-absence proof. Pending and UNRESOLVED
records are excluded from the Bernoulli fit but their share is always reported.
Do not use provisional live outcomes; finalization is a storage eligibility gate.
No wall-clock timeout is an expiry and no unresolved sample is silently a miss.

Weights use elapsed UTC seconds since send, not since eventual resolution.
Only outcomes known at the snapshot's as-of time are eligible. Monotonic receive
times can be subtracted only within the same clock identity. Slots and milliseconds
are distinct endpoints: current measured slot duration describes a conversion,
not an observed millisecond landing latency. A simulator explicitly supplies its
slot duration. Single-region canaries do not validate busy swaps.

## M0: exponentially decayed cells

Use Beta(1,1) prior and w_i = 2^(-age_seconds/3600). For eligible resolved owned
canaries in one exact cell/regime, alpha = 1 + sum(w_i*y_i), beta = 1 +
sum(w_i*(1-y_i)). Publish alpha/(alpha+beta), the equal-tail 95% Beta posterior
interval (numerical inverse CDF), and n_effective = sum(w_i). This count excludes
prior pseudo-counts and measures discounted evidence mass, not Kish sample size.
The decayed likelihood gives an approximate posterior under nonstationarity;
its interval is not a guaranteed frequentist confidence interval.

MEASURED requires n_effective >= 30, latest eligible send <= 300 seconds old,
and pending/unresolved share <= 25% of the selected cell's observed assignments.
Otherwise return INSUFFICIENT, samples_needed >= 1, and no recommendation.
Stale evidence requires fresh probes even if its discounted count exceeds 30.
M0 never labels an unprobed exact cell INTERPOLATED or EXTRAPOLATED.
Cells retain all failures. Propensity is validated and retained; no inverse
propensity weight is used for a conditional exact-cell mean. Aggregate comparisons
use the common uniform arm or explicit propensity adjustment, never naive pooling.

M0 acceptance: Beta(1,1) bounds [0.025,0.975]; interval width decreases over
increasing balanced sample counts; one half-life halves evidence mass and moves
the mean toward the prior; source/regime/future/unresolved/duplicate exclusions
are exercised. Empty input gives INSUFFICIENT, not a prior-only recommendation.

## M1, M2 and exploration (prospective)

M1 uses route intercepts, nonnegative route-specific log(tip/minimum) slopes and
a nonnegative log(1+CU price/1000) slope. Size, congestion, and leader stake/skip
terciles are covariates. Leader/congestion deviations receive L2 shrinkage toward
their parent/global effects; unknown classes remain explicit. Fit only historical
eligible outcomes. M2 blends by prequential held-out log loss (probabilities clipped
to [1e-6,1-1e-6]), with fallback to M0 or INSUFFICIENT. Interval/evidence requirements
must also apply to pooled predictions. Interpolation stays within observed numeric
support; extrapolation is flagged and cannot silently become measured evidence.

Policy v1 keeps at least 30% uniform exploration over the complete 81-cell grid,
with a Thompson-style, cost-weighted remainder. Log marginal assignment propensity
before drawing outcomes. No cell is removed for poor results. Compare v0 and v1
at equal cumulative simulated fee plus tip spend, with identical environments and
separate deterministic random streams. Publish regret against the ground-truth
cost/probability frontier, including cases in which v1 loses.

## Simulator and learner acceptance

Use SplitMix64 with explicit independent assignment/outcome seeds, fixed start UTC
2026-10-05T00:00:00Z, and the normal Canary schema with source=sim. Route, log tip,
log fee, size, leader class, and congestion determine an exponential landing hazard
in slot units, plus a never-land mass. Block validity is 16 simulated slots; a
landing later than validity is EXPIRED. Record exact environment parameters and
ground-truth probability at each horizon. This is an artificial environment,
not a fitted Solana network model. An explicit shift can change slot duration and
congestion; it supplies a new regime ID rather than claiming regime detection.

Acceptance seeds are 7, 42, 2026. M0 convergence uses 12,000 canaries without a
shift; mean absolute probability error over cells/horizons with measured evidence
must be <= 0.08. Re-adaptation uses a shift at draw 12,000 and another 12,000
canaries; the same bound applies to the new regime. M1 must beat M0's mean held-out
log loss on 2,000 later draws after 8,000 training draws, with no leakage. Policy
comparison uses a 2 SOL-equivalent synthetic spend ceiling. These latter checks
belong to later Phase 2 work; no passing result is implied by registration.

## Latency and Signal (prospective, before live analysis)

Latency curves use decayed empirical quantiles with 2,000 seeded bootstrap
replicates and percentile intervals. Include expiration/failure mass as latency
infinity; when the target quantile is unidentifiable report INSUFFICIENT, not a
landed-only optimistic quantile. Report p50/p90/p99 in slots and, only with valid
same-clock observations, milliseconds. Validate simulated 95% interval coverage
over 100 independent datasets, acceptable coverage 90–99% for identifiable p50/p90.
p99 remains inconclusive unless the effective tail count is at least 20.

Publish one daily Signal result per route and size. Primary logistic regressions
for landing within 1 and 2 slots use log tip, fee bucket, route, size, leader class,
congestion and UTC-hour covariates. Use unrestricted slopes for this test: a
monotone forecasting constraint must not predetermine discrimination. Analyze
uniform-arm records as primary; a propensity-adjusted full-sample sensitivity
analysis is separately labeled. RPC has no tip slope. Minimum: 200 eligible
uniform-arm records per route/size and 30 per relevant tip tier; otherwise
INCONCLUSIVE. Do not call missing provider cells flat.

Secondary outcomes are p50/p90/p99 tier differences and congestion/hour
interactions. Cluster bootstrap by contiguous 16-slot blocks, 2,000 replicates,
seeds derived from the UTC day and methodology hash. Within each route/size,
use 99% intervals for the two primary horizons and three secondary quantiles
(Bonferroni allocation of a 5% family error budget). A log-tip slope is practically
meaningful at absolute magnitude >= 0.10; a latency gap at >= 0.25 slots.

DISCRIMINATING requires an interval wholly beyond one practical threshold.
FLAT requires sufficient data and all identifiable primary tip/fee effects and
secondary tip gaps wholly inside their equivalence bands (log-odds effects +/-0.10,
latency gaps +/-0.25 slots). Otherwise INCONCLUSIVE. Unidentifiable tails, absent
tip variation, and broad intervals cannot establish FLAT. Negative effects are
reported. Synthetic known-slope and flat-null recovery are required before live
publication. Store methodology hash, source, window, counts and exclusions in
each report. No live report exists at registration.

## Forecast ledger, grading and baselines (prospective)

Append immutable canonical JSON forecasts in a SHA-256 chain linking previous
hash and full forecast bytes. Verify sequence, bytes and links; updates/deletions
must fail verification. Record methodology hash, source and model snapshot hash.
Grade only same-source/config outcomes sent strictly after forecast creation and
before expiry, known at grading time. Display pending and regime-voided forecasts;
exclude straddling regimes from headline scores and show through-change scores.

Publish Brier, clipped log loss, ten equal-width reliability buckets, expected
calibration error, and whether a forecast interval contains the held-out cell rate.
Coverage of a noisy empirical rate is descriptive, not proof of nominal posterior
coverage. Match a hand-computed scoring fixture and reject tampered ledger bytes.

B1 chooses the published minimum tip, B2 five times minimum, B3 the previous
five-minute tape median (unavailable if no tape), and B4 the stored frozen model
snapshot. Freeze B4 before an injected shift. All use comparable route/size/fees,
held-out data and cost assumptions. Preserve unfavorable comparisons.
