# Phase 5 detection and diagnostics registration

Registered before running the new detector acceptance suite on 6 October 2026.

The primary detector is Bayesian online change-point detection with a Normal–Inverse-Gamma predictive distribution, following [Adams and MacKay](https://arxiv.org/abs/0710.3742). The constant hazard is 1/500 windows, the run-length approximation retains 256 states, and each signal needs 32 warm-up windows. Tail mass is discarded and renormalized; this approximation is explicit. CUSUM is a separately reported baseline with drift 0.5 and threshold 12 standardized units.

A vote requires posterior mass of at least 0.80 in run lengths 1–8, a three-standard-deviation effect, and two consecutive qualifying windows. Minimum absolute changes are 5% for clock signals, 0.05 for fractions and 10 ms for observer lag. Two independent signal families must vote. Slot mean and p95 count as one family. A persistent clock vote alone requires posterior mass at least 0.95 over three consecutive windows. Observer lag alone cannot change a network regime. Epochs annotate evidence; calendar dates never trigger a change or establish an upgrade.

Offline acceptance: seeded 240-window two-step sequences must produce exactly two changes, each within eight windows of the injected step. Across 20 seeds of 600 stationary windows, the total must be zero false changes. Test restart determinism, source isolation, missing values, observer-only perturbations and duplicate delivery independently. These are simulation checks, not live sensitivity estimates.

Change actions commit atomically with the detector checkpoint: persist the regime, cap old regime effective sample mass at zero, enable 100% uniform exploration for 30 minutes under the unchanged budget governor, append VOIDED grades to crossing forecasts, and persist a regime alert. Original forecasts and canary labels remain immutable. New measurements must rebuild sample support before a recommendation is offered.

Observer comparisons use shared host monotonic clocks. Receive lag is relative to the earliest comparable observer; it is not network propagation latency or a claim of independent infrastructure. Complete slot, block identity and success evidence is required for agreement. Missing identities remain incomplete. Missing expected evidence after 30 seconds is diagnostic only. Existing disagreement alerts fire at three distinct canaries within five minutes.

Slot timing uses sealed 256-slot windows, at least 128 slots and ten chain seconds to limit integer timestamp quantization. Parent gaps and DEAD events are evidence of skipped candidates, not complete network coverage. Block sampling records compute consumption, nullable non-vote share, and fullness only when a verified per-slot compute capacity and its provenance are provided. The [RPC block contract](https://solana.com/docs/rpc/http/getblock) does not provide that capacity. Unavailable signals remain null with a reason.

Backfill is read-only, bounded to 42–56 requested days and 1,024 RPC requests per invocation. Retention coverage, missing timestamps and sampled resolution must be reported. It must not assert historical 400→350→300→250 ms changes unless retained chain timestamps support those detections. Historical Replay/backfill does not set the Live regime.

Workload fidelity compares raw in-block positions only at equal paid tips and identical index scope. Workload, route, block fullness and account contention remain uncontrolled; this comparison does not establish swap performance or causal efficacy. Passive transfers never enter canary training.

The comparison uses the same one-hour window for both populations. It totals distinct observed known-recipient payments per signature and scope, deduplicates observer deliveries, and excludes owned signatures and contradictory candidates. Its `tape_transfers` count is the number of grouped position samples. Bounded tape coverage can truncate multi-recipient totals; the comparison does not independently establish passive candidate finalization.

`LANDED_THEN_DROPPED` remains provisional while the signature can still land elsewhere. Finalization requires finalized canonical exclusion of every observed candidate, finalized covered RPC absence, and a checked block height beyond the transaction's last valid height. A duplicate candidate or missing observer alone never finalizes that outcome.
