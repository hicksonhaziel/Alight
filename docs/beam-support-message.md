# Beam HTTP support message

Draft for Hickson to post in Solami's Telegram chat. No message has been sent.

> Hi Solami team, I'm building Alight, a Solana transaction-landing observatory.
> Your public llms.txt lists beam-http.solami.dev for Beam HTTP, but that hostname
> returns NXDOMAIN on my connection and Google Public DNS (checked 5 October 2026).
> Beam QUIC at beam.solami.dev:11000 resolves. Is Beam HTTP currently supported?
> If so, could you confirm the correct URL/path, authentication method and request
> format for submitting a signed transaction? Also, does HTTP use the same minimum
> tip and recipient addresses as QUIC? Thanks.

Public references: [Solami documentation](https://solami.dev/llms.txt),
[Google DNS lookup](https://dns.google/resolve?name=beam-http.solami.dev&type=A).
Google returned DNS status 3, with no answer records; the local resolver returned
name-not-found. This prevents an HTTP request to that hostname, so it does not
establish an authentication problem or an HTTP service response. No keys, account
screenshots, private endpoint URLs or wallet credentials are needed in the post.
