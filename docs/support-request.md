# Solami support request — prepared, not sent

The owner has not authorized sending a message to support. This draft contains
public project context only; add no keys, private URLs, or wallet secrets.

Hello Solami team,

I am building Alight, a Solana transaction-landing observatory and calibrated
quote service for the live-data build. The account's Pro trial ends 9 October;
the listing deadline is 13 October at 06:59 UTC. RPC, gRPC, Mirage, Blur and
signed webhook deliveries have passed bounded read-only integration checks.
A Beam QUIC identity also connected successfully without submitting a transaction.

Could you confirm:

1. Whether startup credits or a small measurement-wallet tip rebate are
   available for a capped canary experiment? The wallet is currently unfunded.
2. The supported current Beam HTTP URL, authentication, request schema, tip
   minimum and error semantics, if HTTP is available.
3. The arrangement for scoped read-only judge access and continued trial access
   through the submission deadline, including any costs before renewal.
4. The intended meaning of Yellowstone transaction indexes versus RPC block
   signature-list positions. In our saved sample, slot 453096590 reported gRPC
   index 205 while the same signature appeared at RPC signatures position 167;
   slot 453096591 reported 887 versus 401. We preserve both values and do not
   assume they share an ordering. Do these streams expose different ordering
   semantics or omit transactions?
5. Current transaction-version support: full-block requests limited to version
   0 returned -32015 requiring version 1. Which clients/encodings should be used?

We can share sanitized fixtures and exact public signatures if useful. No
canaries have been signed or submitted yet; no outcome/performance claim is made.
