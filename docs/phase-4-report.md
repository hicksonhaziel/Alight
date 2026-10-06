# Phase 4 workbench evidence

The offline workbench is implemented and verified against the real local Sim
API. It reuses the owner's Aftershock vector mark, charcoal/orange and light
palettes, IBM Plex typography, compact navigation, and panel language. Assets are
self-hosted. Motion is limited to useful screen, dialog, connection, and loading
feedback; reduced motion is honored. No fabricated charts, transaction outcomes,
market savings, or detector history were added.

Status: **WORKBENCH_OFFLINE_PASS_DEPENDENCIES_PENDING**. This is not a declaration
that every Phase 4 live acceptance criterion is complete. The funded Phase 3
N40/SDK/CLI live checks remain pending, and collection stays paused.

| Task | Implemented and verified | Remaining acceptance |
|---|---|---|
| 4.1 | React/Vite/TS shell, both themes, source badge, one schema/source-validated WebSocket, reconnect/pause behavior. Mixed-source responses disable evidence/actions. | Real Live and Replay browser sessions. |
| 4.2 | Sim cockpit: clock provenance, actual curves/95% bands/n/age, exact reservations, owned outcomes, eight-slot recorded-schedule contract. | Host live cockpit/runway check. |
| 4.3 | Quote card matches API prediction/configuration, exact request copy, conditional frontier/knee rendering, explicit sparse-market probability-only fallback. | Fresh live market frontier validation. |
| 4.4 | Freeze through authenticated API, recoverable claim/run URLs, N40 actual membership, Wilson interval, all verdict labels, history and progress reads. | Funded live run; no fake progress substitutes for it. |
| 4.5 | Forecast/status inspection, stored partial/final grades, reliability/Brier/log loss/baselines, complete browser SHA-256 verification and tamper rejection. | External published/anchored head provenance is separate. |
| 4.6 | Observed regime labels, temporal range/count view, context/source labels, empty states. | Phase 5 signal extractors, detector, calendar annotations and backfill. |
| 4.7 | Observer freshness/cursors/gaps, explicit recorded disagreement list, original observation drill-down and same-clock latency limits. | Phase 5 observer lag distributions and agreement aggregates; live disagreement drill-down. |
| 4.8 | Bounded actual tip tape/distribution, requested-vs-paid distinction, stated receipt/history limits. | Phase 6 wallet history/receipt service and a real-wallet report. |
| 4.9 | Responsive 390px mobile, both themes, native dialogs, keyboard command navigation, labels/shapes, chart table alternatives, reduced motion. | Human assistive-technology review beyond automated Axe. |
| 4.10 | Real Sim browser quote → freeze → 40 held-out members → ledger verify; five Playwright tests integrated in CI. | Funded/live flow remains pending. |

Supporting contracts add only bounded read-only workbench, owned observations,
prospective Prove members and canonical ledger pages. Migration 0009 adds indexes.
Shared Rust/OpenAPI/TypeScript schemas are regenerated. Live display caps use
the governor's configured caps. Sim grades later Prove evidence after its logical
clock advances while keeping those members excluded from training.

Validation performed on 6 October 2026:

- Required Rust format, all-target Beam Clippy, and complete Beam workspace tests.
- TypeScript SDK strict lint, three SDK tests and generator drift check.
- Phase 3 regression: both SDK examples and CLI each resolve N40 in Sim, verify
  the ledger, and retain all four CLI exit codes; zero provider calls/chain sends.
- Web strict TypeScript/React-hooks lint, three precision/clock/ledger tests,
  production build and five Playwright tests.
- Fourteen Axe WCAG 2 A/AA and 2.1 AA audits: seven screens × two themes, zero
  violations. Mobile/offline/mixed-source and conditional-market fallback pass.
- Actual browser flow compares the quote with the HTTP response, sends exactly
  one freeze and one Prove POST, checks all 40 stored members, recovers after
  reload, verifies a Rust-produced chain with browser SHA-256 and rejects an
  edited export. Public requests omit operator authorization.
- Compiled production assets served through Caddy 2.10.2 against isolated Sim:
  browser stream connected and no console warnings/errors; assets/public routes,
  CSP, unknown-API 404, static-write 405 and unauthenticated-Prove 401 pass.
  Compose configuration and Caddy configuration validate. The packaged image
  also builds with the exact pinned lockfile using an ephemeral mount of already
  verified public package-cache entries and networking disabled. It serves its
  baked assets successfully against isolated Sim. The normal online dependency
  install encountered local registry connectivity delays and was stopped.
- Repository/private-file checks and historical Phase 3 receipt verification.

An initial new source-isolation fixture attempted to import synthetic Live data;
the store correctly refused it. The fixture now uses Replay and passes. Initial
browser selector ambiguities were corrected before the final passing run.
The first clean GitHub browser run exposed Chromium discarding a response body
before the test read it. The test now buffers the actual API body before releasing
it to the page; it performs one request with redirects/retries disabled and keeps
the exact-value comparisons. Four other browser tests passed on that first run.

The browser verifies exact Rust canonical JSON bytes, not JavaScript reserialized
floats. Its uploaded export's declared head establishes internal integrity, not
independent provenance or model-artifact validation. Pending scores, unavailable
baselines, poor verdicts and unresolved outcomes remain visible. Canary evidence
does not establish swap execution performance.

See `data/phase-4-validation.json` for bounded machine-readable results and source
hashes. The receipt records the publishing code snapshot; subsequent phases can
change implementation without rewriting this historical evidence.
