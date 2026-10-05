# Prove methodology v1

Registered 5 October 2026 before funded Prove or inspection of live Prove results.
Each lock stores the SHA-256 of these bytes. Existing Signal registration and
historical receipts remain unchanged.

Lock an unexpired supported forecast, its exact configuration, immutable model
snapshot, source, regime and N (normally 40). The probability claim is the frozen
point estimate for the quoted slot horizon. A latency-quantile claim is the
requested quantile at the quoted maximum slots or same-process milliseconds.
This tests the claim used by that request, not an unrelated four-slot probability.

Count only the run's prospectively linked attempts, with unchanged configuration,
source and regime, sent after the lock and no later than forecast expiry. An
outcome must be finalized and known at report time. LANDED_OK and LANDED_FAILED
count as landing when within the target; proven EXPIRED, REJECTED and
LANDED_THEN_DROPPED count as nonlanding. UNRESOLVED stays unknown. Millisecond
targets require comparable send/observer clock IDs; missing timing stays unknown.
Keep all held-out attempts out of model fitting, including after completion.

For k successes among n resolved attempts, report the Wilson interval with
z=1.959963984540054. CONSISTENT means the complete N-attempt interval contains
the locked point claim. INCONSISTENT means the complete interval excludes it,
including unexpectedly high observed landing. Either result requires all N
attempts resolved, N at least 30, and no regime change. Partial runs, unresolved
outcomes, small N, expiry before N sends, and changed regimes are INCONCLUSIVE;
changed regimes also mark the run VOIDED. Partial intervals describe the resolved
subset and cannot support a final verdict. Consistency is not proof of equality,
real-swap fidelity or calibration at other horizons/configurations.

The server uses the existing durable daily/burst governor for every attempt.
Reservations and prepared identities survive restarts; uncertain submissions are
never automatically replayed. Expiry stops scheduling but never creates a canary
outcome. Sim uses an independent seeded prospective draw, exact locked settings,
clearly synthetic outcomes and source=sim budget reservations, with no signer.
