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
SWQoS key creation is prepared but awaiting the browser tool's required final
credential confirmation. Webhook HTTP delivery needs a public receiving endpoint.

## 2026-10-03: Solami SDK version

The old SDK 0.1.4 documentation used `.fast` endpoints. The official crate source
for **0.1.58** uses `.dev` endpoints, regional overrides, and configurable Beam
connections. Inspecting the latest source corrected the initial compatibility
concern. Use a pinned compatible version for the future sender, after a real
QUIC connection spike. Do not use its random, floating-point `build_tip_ix` helper
for reproducible experiment assignments: build tips with integer lamports and
an explicitly recorded recipient. The current workspace has no sender dependency.
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
slot/block metadata framing and authentication. They contain no canary sends
and do not establish transaction capture completeness, webhook signatures,
landing latency, or finality resolution. Keep those acceptance criteria pending.
