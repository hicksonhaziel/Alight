# Phase 0 evidence and acceptance report

Status: **PARTIAL — integration spikes and contracts recorded; funded/deployment
acceptance still pending**. G1's shared contracts and observer-fixture prerequisite passes.
G0 and the 50–100 canary pilot remain unfunded. Zero canaries were built, signed
or sent; zero lamports spent. There is no daemon, quote API, governor or deployed
collector yet.

| Task | Actual result | Acceptance status |
| --- | --- | --- |
| 0.1 Cutoff | Official listing: 13 October 2026 06:59 UTC / 07:59 Lagos | PASS |
| 0.2 Account/endpoints | Existing logged-in Pro account inspected; RPC 200/s, sends 5/s, 2 gRPC, 5 WS, 10 webhooks; trial ends 9 October | PASS for existing account access; no new signup/billing action |
| 0.3 Dedicated wallet | Private key local/ignored; sampled mainnet balance zero | PARTIAL: funding deferred by owner |
| 0.4 Beam QUIC | Tip addresses captured; 30-day `alight-phase0-beam` key created; real connection authenticated in 5,397 ms | PARTIAL: no funded landing |
| 0.5 Beam HTTP/RPC sends | HTTP candidate contract unverified; no transactions sent | DEFERRED: funding and current HTTP contract |
| 0.6 gRPC | Baseline metadata plus a second capture: 13 public transactions, 12 slot events, 4 block metadata | PASS for bounded schema capture; no canary completeness claim |
| 0.7 Mirage | 3 public transactions, 7 slot events, 2 block metadata, 1 ping over binary WS | PASS for bounded framing/authentication; heartbeat cadence/reconnects remain Phase 1 work |
| 0.8 Webhook | 6 unique HMAC-verified HTTP deliveries ACKed 200; 3 authenticated WS frames; measured delivery lag | PASS for public-control delivery; payer remained idle; test deleted |
| 0.9 Blur | Three active pools, real trades/candles and 12 exact WS JSON frames | PASS for schema spike; rate-limit/error endurance unmeasured |
| 0.10 RPC | Leader schedule, blockhash/height, historical timestamps, finalized signature checks, block identities; version-1 full-block sample | PARTIAL: historical full-block retention and index semantics unresolved |
| 0.11 Host/RTT | Only local request elapsed times available | DEFERRED: deployment host not supplied; no fabricated RTT/region table |
| 0.12 Contracts/scaffold/CI | Shared v1 types, exact integers, clock/index scopes, explicit insufficient quotes; Rust/Node/Python scaffold | PASS for contract/fixture prerequisite; validation recorded in receipt |
| 0.13 Slot clock | 64/256/1024-slot timestamp windows: 265.625/265.625/266.602 ms | PASS for coarse mean including skipped slots |
| 0.14 Provider support | Draft prepared; no message sent, credit/rebate/judge/renewal terms unconfirmed | PENDING explicit message authorization/reply |
| 0.15 Pilot | No funded canaries | DEFERRED: zero outcomes, no landing-rate/latency table |

## What the checks establish

gRPC, Mirage, webhook HTTP/WS, Blur and RPC now have sanitized real fixtures.
The extra public Orca control pool supplied transactions while the dedicated
wallet was unfunded. The Mirage filter has been restored to its original
16 payer/tip addresses. Public-control traffic proves adapter shapes and
connectivity; it cannot replace Alight's randomized, budgeted canary outcomes.
The Beam probe authenticates QUIC and exits without transaction construction.

Webhook HMAC is SHA-256 lowercase hex over exact bytes using the literal issued
secret. Initial deliveries arrived before the secret was copied and were ACKed
503; 20 verified offline later. The six published deliveries verified online
and were ACKed 200. Lag from provider Unix seconds to local UTC was 2,058 ms min,
6,188 ms median, 6,315 ms max. It includes enrichment, retry/queue, timestamp
rounding and clock skew; it is not chain landing latency or transport RTT.

## Problems discovered and handled

- Solami SDK 0.1.58's dependency ranges selected patches requiring a newer Rust
  compiler. Compatible Solana families are pinned and serialization explicitly
  enabled. The optional connection probe compiles on pinned Rust 1.96.1.
- RPC confirmed nine public signatures finalized, but gRPC indexes and RPC block
  list positions differ on two samples. Both values and scopes are retained;
  position-based cross-observer analysis is deferred until semantics are known.
- Full-block RPC requests declaring max transaction version 0 failed with -32015
  requiring version 1. One version-1 retry decoded a 4.09 MB, 598-transaction
  block; another failed at the network layer. Preserve these failures and use
  version-aware, bounded decoding. Full-block historical coverage remains unknown.
- gRPC protobuf default status zero can disappear in JSON. gRPC fixtures restore
  it explicitly; Mirage adapters must decode the raw enum before normalization.
- Blur quantities can exceed JS safe integers. Exact raw JSON is preserved.
- Leader schedule initially timed out at 12 seconds, then returned 667 leaders.
  The original timeout stays in the receipt alongside the follow-up success.

Shared v1 contracts and examples are documented in `docs/contracts.md`.
They are interface shapes; later models, governor, resolver, adapters and hash
chain still need implementation and independent validation. A timeout remains
missing evidence and can never establish transaction expiry.

## Cleanup and repository state

The owner approved deleting `alight-phase0-webhook-test`. The account shows zero
webhooks. The receiver and temporary Cloudflare tunnel are stopped, and the
webhook secret/id/stream token/stream URL/callback URL were cleared from `.env`.
The Beam key remains for later use. No PAYG, renewal or paid hosting was enabled.

Regular verified batches are pushed to `codex/phase-0-validation` in
[Alight](https://github.com/hicksonhaziel/Alight/tree/codex/phase-0-validation).
The initial scaffold's [GitHub CI passed](https://github.com/hicksonhaziel/Alight/actions/runs/37164273259).
Subsequent validation is recorded in `data/phase-0-receipt.json`. CI checks
formatting, Clippy, tests, script syntax, HMAC behavior and private-file handling;
it never performs live probes or receives local credentials. Web checks are not
applicable because `web/` does not exist yet.

`THIRD_PARTY_NOTICES.md` was removed as requested. README remains unchanged and
uncommitted. The planning files, `.env`, raw evidence and `context.txt` stay
ignored and untracked. `agent.md`/`AGENTS.md` retain these working rules.

## Remaining prerequisites

To pass G0 and the pilot, a capped funded canary must land through Beam and be
observed by gRPC, followed by the recorded route/tier/size experiment. The future
budget governor must be in place before signing. No funded test is being claimed.
Deployment RTT/region measurements need a real host. The current Beam HTTP
contract, transaction-index semantics and support/judge/trial arrangements need
provider confirmation. `docs/support-request.md` is ready for the owner to send
or explicitly authorize sending. After that, Phase 1 can implement the persistent
collector and restart-safe resolver in observe mode before enabling canary sends.
