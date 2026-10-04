# Phase 0 evidence

Status: **partial; G0 and G1 pending**. No money moved. No live canary was signed
or sent. The owner has deferred funding, so landing performance and the pilot
cannot be validated now.

| Integration | Actual evidence | Remaining acceptance |
| --- | --- | --- |
| Mainnet RPC | Genesis, version, slot, epoch, blockhash/last valid height, recent block identity, historical timestamps, wallet balance | Funded signature resolution and historical full-block coverage |
| Leader schedule | Initial 12-second timeout retained; follow-up passed, 667 leaders | Deployment-host reliability and caching |
| gRPC | 24 slot events + 8 block metadata events; original protobuf frames saved locally | A real payer/tip transaction with in-block index and fork-aware resolution |
| Mirage | Dashboard filter created; 12 binary frames decoded over authenticated WebSocket | Transaction fixture, heartbeat timing, reconnection/gaps |
| Blur REST | Pool metadata, trades, and candles for three active pools | Long-running freshness, rate-limit and error behavior |
| Blur WebSocket | 12 real frames; exact JSON retained beside parsed previews | Long-running gaps and integer-safe adapter |
| Beam QUIC | Current endpoint and 100,000-lamport tip floor inspected | SWQoS key and funded send observed by gRPC |
| Beam HTTP / plain RPC sends | Not attempted | Current Beam HTTP contract and funded sends |
| Webhook | Dashboard allows 10; no existing webhook | Public callback, provider secret, real signed delivery and measured lag |

The scaffold pins Rust 1.96.1 and dependency versions, provides a local/network
`alight doctor`, and includes bounded gRPC/Blur/Mirage probes. Five Rust tests
cover strict expiry evidence, slot clock arithmetic, redaction, and RPC absence
versus error handling. Format, Clippy, and workspace tests passed locally.
The CI workflow is prepared but has not run remotely; nothing has been committed
or pushed. Web checks are not applicable because the web application does not yet exist.

Fixtures and the sanitized receipt are in `data/fixtures/` and
`data/phase-0-receipt.json`. Raw protobuf frames and detailed research remain
under ignored `.alight/`. The receipt explicitly records zero sends and incomplete
gates. A passing transport probe is not a completed Phase 0 gate.

Next work: resolve the pending Beam credential action, implement the collector
and restart-safe resolver in observe mode, and prepare a webhook receiver. Freeze
the remaining contracts after actual transaction/delivery fixtures arrive.
Any request to support for credits or judge access needs the owner's explicit
message authorization; no message has been sent.
