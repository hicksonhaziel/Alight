# Documentation

Start with the [project README](../README.md) for a quick start and an overview.
Use these guides for the details:

| Guide | What it covers |
| --- | --- |
| [Architecture](architecture.md) | Data flow, Solami integrations, crate boundaries and operating modes |
| [Operations](operations.md) | Local setup, safe live controls, SDK/CLI examples, deployment and troubleshooting |
| [API and data contracts](contracts.md) | Endpoints, authentication, JSON types, storage and migration rules |
| [Limitations](limitations.md) | What the measurements support and where conclusions stop |
| [Datasets](dataset.md) | CSV/Parquet export, schema, checksums, ledger witnesses and licensing |
| [Wallet receipts](receipts.md) | Bounded wallet history, payment accounting and historical comparisons |
| [Ledger anchoring](anchoring.md) | Offline commitments and governed mainnet verification |
| [Decisions](decisions.md) | Dated integration findings and changes from the original design |

## Registered methods

These documents are analysis registrations, not progress reports. Forecasts and
validation receipts hash their exact bytes; prospective wording describes the
design at registration time. Current behavior is described in the contracts
and operations guide.

- [Estimation, latency, Signal and grading](methodology.md)
- [Held-out Prove](prove-methodology.md)
- [Regimes and observer diagnostics](regime-methodology.md)

## Inspect the evidence

- [Final review](../data/final-review-validation.json): bounded Live runtime,
  finalized spending audit, retained failures and remaining coverage gaps.
- [Live Prove](../data/live-prove-validation.json): one RPC configuration,
  40 held-out attempts, finalized outcomes and wallet reconciliation.
- [Owned Webhook delivery](../data/webhook-owned-validation.json): exact-body
  HMAC verification and persisted owned observations.
- [Sim mainnet anchor](../data/anchors/sim-ledger-mainnet.json): a finalized
  fingerprint of synthetic ledger bytes; its source remains Sim.
- [Restart/reconnect drill](../data/stability-drill-validation.json) and
  [security review](../data/hardening-security-validation.json): bounded checks
  with their remaining limitations.

Each receipt records its own date, scope and limitations. Historical fixtures
and completed runs are evidence from their recorded windows, not a current
availability or performance promise. Live, Sim and Replay remain separate.
