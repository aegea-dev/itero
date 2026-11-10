# github-webhook

A push to a GitHub repository is a notification on this machine. It uses a real
service through a real tunnel: `examples/webhook` is the one to try with `curl`
alone.

One workflow, `forward`, takes every webhook and sends a `github.pushed` event of
its own, with only the fields it picks. Another, `tell`, shows it.

## Run it

Keep a long random secret, and start the daemon. `itero webhook` runs as its
connector, on port 8788:

    openssl rand -hex 32                     # the secret
    itero ws secret GITHUB_WEBHOOK_SECRET    # asks for it
    itero ws daemon

Give that port a public HTTPS address. With [Tailscale
Funnel](https://tailscale.com/kb/1223/funnel):

    tailscale funnel 8788

The first time, enable HTTPS Certificates in the admin console's DNS page; then
it prints a link that enables Funnel for your tailnet. Point it only at this
port, never at the daemon's own. It stops with Ctrl-C.

In the repository's Settings, under Webhooks, add one:

- Payload URL: the address it printed, then `/github`
- Content type: `application/json`
- Secret: the secret

GitHub sends a `ping` at once, which shows as delivered and tells nothing. Push a
commit, and the notification comes. Recent Deliveries has each request; Redeliver
sends one again, and itero skips a delivery it has seen in the last week (the
`unique` in `workflows/forward.json`).
