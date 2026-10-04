# Decisions

## 2026-10-03: Submission cutoff

The official [listing](https://superteam.fun/earn/listing/build-something-live-on-solana-data)
specifies **2026-10-13 06:59 UTC**, or **13 October 07:59 Africa/Lagos**.
The listing's live countdown was also inspected in the browser. This corrects
the provisional 12 October date in the private briefs, which remain unchanged.
Recheck the listing before submission in case the organizer changes it.

## 2026-10-03: Zero funding

The owner cannot fund canaries now. Remain in `observe` mode. A dedicated wallet
exists locally and its mainnet balance was zero. No transaction has been signed
or submitted. G0 and the 50–100 canary pilot cannot pass without funded live sends.
Do not use Aftershock's wallet, enable PAYG, renew a paid plan, or substitute
simulated outcomes for live evidence.

## 2026-10-03: Provider configuration

Reuse the owner's existing Aftershock RPC, gRPC, and API credentials server-side
in Alight's ignored environment, as explicitly authorized. Keep endpoint/token
separation and the legacy stream aliases. Blur REST and WebSocket authenticated
successfully with the API credential. Mirage required a saved subscription;
`alight-phase0-observer` is now enabled for the dedicated payer plus 15 published
tip addresses, no votes, processed commitment, slots, and block metadata.
Its WebSocket stream passed a bounded live probe.

The dashboard showed a Pro period from 2–9 October, RPC 200 requests/s,
sendTransaction 5 requests/s, 2 gRPC streams, 5 WebSocket streams, and 10 webhook
slots after account loading completed. No billing settings were changed.
Beam's published minimum tip is 100,000 lamports. Its account tip balance is zero.
The SWQoS key and temporary webhook actions were subsequently completed on
4 October; see the follow-up evidence below.

## 2026-10-03: Solami SDK version

The old SDK 0.1.4 documentation used `.fast` endpoints. The official crate source
for **0.1.58** uses `.dev` endpoints, regional overrides, and configurable Beam
connections. Inspecting the latest source corrected the initial compatibility
concern. Use a pinned compatible version for the future sender, after a real
QUIC connection spike. Do not use its random, floating-point `build_tip_ix` helper
for reproducible experiment assignments: build tips with integer lamports and
an explicitly recorded recipient. The Beam connection-only example now pins this SDK behind the optional
`alight-ingest/beam` feature. There is still no transaction sender.
Sources: [crate](https://crates.io/crates/solami),
[official SDK repository](https://github.com/useSolami/solami-sdk),
[Solami SDK](https://solami.dev/sdk).

The QUIC endpoint `beam.solami.dev:11000` is confirmed by the dashboard. The HTTP
candidate in `.env.example` comes from public reference material; its current
authentication and request contract remain unverified. It is not a working route
until a spike proves it.

## 2026-10-03: Clock, retention, and host measurements

Recent block timestamp windows yielded 265.625 ms/slot for 64 and 256 slot
distances, and 266.6015625 ms/slot for 1,024. The denominator includes skipped
slots. Timestamps are coarse chain time, not transport latency; use a rolling
measured clock rather than a fixed 250 ms conversion.

Historical `getBlockTime` worked at 1 million, 4 million, and 16 million slots
before the sampled tip. `getFirstAvailableBlock` was only 114,726 slots behind
the tip. An old full-block request timed out; that does not prove pruning.
Timestamp backfill is promising, but full-block historical coverage is unresolved.

Successful local RPC probes commonly took 167–283 ms after initial connection;
the initial genesis request took 5,889 ms. Leader schedule initially exceeded
12 seconds, then returned 667 leaders in 4,503 ms. `getSlotLeaders` took 20,348 ms.
These are request elapsed times from the owner's current machine, not transport
RTTs or measurements from a deployment host. No production region has been chosen.
Measure candidate deployment regions before claiming a regional latency advantage.

## 2026-10-03: Blur pool selection and exact values

Sorting only by TVL surfaced an inactive pool. The three-pool spike instead ranks
by daily volume and requires at least $100,000 reported TVL and 100 reported
trades in the active window. These thresholds select schema samples, not vetted
trading recommendations. Record provider prices/USD decimals as strings and
raw token quantities as exact integers. Blur WebSocket fixtures retain `raw_json`
because JavaScript's parsed number preview can lose precision above 2^53−1.

## 2026-10-03: Observer evidence

gRPC and Mirage are separate transport observations from the same provider;
their agreement is not independent trust. The current captures demonstrate
slot/block metadata framing and authentication. They contain no canary sends. Subsequent public-control captures and webhook
checks are recorded below. Capture completeness and canary landing latency
remain unmeasured.


## 2026-10-04: Beam credential and connection

Created `alight-phase0-beam`, a 30-day SWQoS identity, using the owner's explicit
permission. The dashboard's creation modal labeled a 64-byte secret keypair as
public; validate it as a keypair and keep it private. The private value is saved
only in ignored `.env`, mode 0600. Its public identity is
`88XWfp4ct9UkWt6nm15Zi56A4a15pKvQ8uU9YsVoDwUD`.

`cargo run -p alight-ingest --example beam_connect --features beam --locked --offline`
passed against `beam.solami.dev:11000` in 5,397 ms. It authenticates a QUIC
connection and immediately drops the client. No transaction is constructed,
signed or submitted; this does not pass G0 or establish a landing probability.
The key stays available for the later governed sender. Beam HTTP remains a
candidate, with no verified current request/authentication contract.

The SDK's broad Solana dependency ranges selected patches requiring Rust 1.97.1.
Pin the compatible families from the SDK's published lockfile for Rust 1.96.1,
and enable `solana-transaction/serde` explicitly for its Beam serialization.
Its unconditional protobuf-source build also runs for Beam-only builds; the
first optional-feature compilation is expensive. Do not vendor a patched SDK
or silently upgrade the project's toolchain to work around this.

## 2026-10-04: Verified webhook, then cleanup

A temporary `alight-phase0-webhook-test` watched the dedicated payer and public
Orca control pool `Czfq3xZZDmsdGdUyrNLtRhGc47cXcZtLG4crryfu44zE`.
The control supplied real public transactions without spending funds. A bounded
loopback receiver accepted only a random callback path, capped bodies/captures,
served no files, and used a temporary Cloudflare tunnel.

Initial deliveries arrived before the creation secret could be extracted; they
received HTTP 503 and were kept as local raw evidence. Twenty of those bodies
subsequently verified offline. After the secret was available, six unique
real deliveries verified online and received HTTP 200. The observed signature
is lowercase hex HMAC-SHA256 over exact body bytes, keyed with the literal
provider secret. Modified bodies and wrong/missing secrets fail verification.
A separate authenticated webhook WebSocket probe captured three JSON frames.

Provider `received_at` seconds versus local receive UTC gave min 2,058 ms,
median 6,188 ms, max 6,315 ms across six deliveries. These include enrichment,
queue/retry, timestamp rounding and clock skew; they are not chain landing
latencies or transport RTTs. Neither capture contains Alight canary outcomes.

The owner explicitly approved deletion. The dashboard now shows zero webhooks.
The receiver/tunnel were stopped, and the temporary webhook id, signing secret,
stream URL/token and callback URL were cleared from `.env`. Shared credentials,
Beam identity and sanitized verified fixtures remain. No paid settings changed.
See `data/webhook-validation.json` for delivery counts and cleanup flags.

## 2026-10-04: Transaction indexes and modern full-block payloads

The public-control gRPC capture contains 13 transactions, 12 slot events and
four block metadata events. Mirage's capture contains three transactions,
seven slots, two block metadata frames and one ping. Its original filter of
16 payer/tip addresses was restored after the temporary control-pool capture.

RPC confirmed nine sampled gRPC/webhook signatures finalized. Sampled RPC block
hashes match gRPC block metadata. But provider indexes and RPC list positions
**disagree**: slot 453096590 has 205 versus 167; slot 453096591 has 887 versus
401. A full-block RPC retry also placed the latter at 401. The gRPC metadata
reports 1,279 executed transactions for that block, whereas RPC full JSON
contains 598 transactions. Filtering/order differences are a hypothesis, not
an established cause. Keep `index_scope` and source evidence, and quarantine
cross-source position comparisons until semantics are confirmed.

Full-block requests declaring max transaction version 0 returned -32015,
requiring version 1. With version 1, one 4,090,522-byte block returned successfully
with 169 version-0, 272 legacy and 157 version-1 transactions. Another retry
failed with URLError. Preserve all failures in `data/rpc-index-followup.json`.
The successful response establishes current full-block decoding on this sample,
not historical full-block retention or long-run reliability. Future observers
need version-1 support and bounded bodies larger than the doctor's metadata cap.
The [Solana RPC structures](https://solana.com/docs/rpc/json-structures) document
separate full transactions and signature-only block responses; our measured
ordering disagreement remains unresolved by those shape descriptions.

## 2026-10-04: Contract freeze and remaining acceptance

Freeze shared v1 types with lossless u64 strings, clock identities/UTC times,
separate gap records, explicit index scope, quote context and strict insufficient
response handling. Provider fixtures are historical live captures, not active
streams or a deployed API. See `docs/contracts.md` for examples and migration.
G1's contract/observer-fixture prerequisite is satisfied. Phase 1 still needs
implemented adapters, persistent storage and a tested restart-safe resolver.

G0, funded route checks and the 50–100 canary pilot remain unfunded. No deployment
host was supplied, so task 0.11's deployment RTT/region acceptance is deferred;
local elapsed times must not be relabeled as deployment measurements. A support
request is prepared in `docs/support-request.md` but has not been sent: sending
a message requires explicit owner authorization. No credit, rebate, judge-key
arrangement, renewal or reply has been assumed.

## Phase 1 route and local deployment checks — 4 October 2026

The public tip-address API returned 15 accounts during Phase 1; Solami Rust
0.1.58 embeds 10. The engine fetches and validates the current recipient list,
then uses the SDK's documented `skip_precheck` option to avoid rejecting newer
published accounts. Integer transfers and the 100,000-lamport floor remain
unchanged. The exact tip list and recipient are retained with each prepared send.
The public Beam HTTP hostname did not resolve (DNS name not found). Dashboard
instructions exposed QUIC; no funded HTTP send contract has been established.
Its implementation and observed availability must remain separate in reports.
See the [published tip route](https://api.solami.dev/onchain/tip-addresses) and
[pinned SDK](https://docs.rs/crate/solami/0.1.58).

Hickson selected this computer as the initial deployment host. Compose/Caddy
binds only localhost, preserves the existing SQLite database, and uses bounded
metadata/log retention. Laptop uptime determines collection availability; this
choice does not establish endpoint-region latency or uninterrupted VPS hosting.
The dedicated wallet's confirmed balance was zero. All funded route and real
mid-flight canary gates remain pending; simulated recovery and ambient observer
traffic are identified as such.
