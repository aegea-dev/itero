# itero

itero (Latin: "I repeat") runs workflows on one machine, when something
happens or on a schedule. A workflow is a few lines of JSON; its steps are
small executable files in any language, leaning on the command line tools
already there (`sh`, `curl`, `jq`).

```json
{ "on": ["order.placed"], "steps": [["#check-payment", "#check-stock"], "order-process", "#send-mail"] }
```

Documentation: <https://aegea-dev.github.io/itero/>.

## Install

```bash
brew install aegea-dev/tap/itero
# or
curl -fsSL https://raw.githubusercontent.com/aegea-dev/itero/main/install.sh | sh
```

Homebrew puts `Itero.app` in the formula's own folder. The script checks
the release archive's SHA-256 before unpacking, installs `itero` to
`~/.local/bin` and, on macOS, `Itero.app` to `~/Applications`, and never
edits your shell profile. `--version X.Y.Z`, `--to DIR` and `--uninstall`
are its options (`sh -s -- --help`).

## Build

```bash
cargo build
make test         # the test suite
make lint-check   # clippy, warnings fail
make fmt-check
make test-linux   # the tests and clippy on Linux, in Docker
make test-docker  # the IMAP and POP3 examples against a mail server in Docker
```

On macOS, `make notifier` builds `Itero.app` next to the binary (and `make
install` puts it in `~/Applications`): notifications then come from itero.
Without it they come through `osascript`. On Linux, notifications go
through `notify-send` (or `gdbus`, or `kdialog`).

## A workspace

A folder with `itero.toml` is a workspace; `itero daemon` runs it.

```
itero.toml                 settings (the port)
workflows/order.json       a workflow: when it runs (on) and its steps
actions/check-payment      an action: an executable, its name its file name
sources/mail.received      a source: each line it prints is a message of this type
args/check-payment.json    data for an action (itero args)
```

- **A workflow** starts on a message type (`order.placed`) or a schedule
  (`0 9 * * *`), under conditions (`$order`, `$$API`). Its steps are
  actions (`#name`), workflows, lists that run side by side, and steps
  with data and their own `on`. `success`, `failure` and `error` run at the
  end.
- **An action** reads its values as environment variables (from the step,
  its args, then the run's state; `$$NAME` for a secret) and the state on
  stdin, prints a JSON object, and exits 0 (done), 75 (not yet: wait for a
  matching message) or anything else (an error).
- **A message** starts runs of the workflows on its type, and reaches the
  runs waiting for it that it matches by their data.

Commands: `itero daemon`, `itero emit <type> '<data>'`, `itero args`,
`itero secret`, `itero check`, `itero runs`, `itero errors`, `itero watch`,
`itero service install|status|uninstall`.

## Examples

- [`examples/mail-alert`](examples/mail-alert): a notification for each
  new mail in the local mailbox, as it arrives.
- [`examples/mail-servers`](examples/mail-servers): new mail on an IMAP or
  POP3 server, checked every minute with curl.
- [`examples/order`](examples/order): payment and stock side by side, the
  payment waiting for its provider, then shipping and a mail.
- [`examples/page-digest`](examples/page-digest): a page, summarized by an
  AI assistant, by mail.

## Design

[`docs/design-simple.md`](docs/design-simple.md) is the design of the
files people write; [`docs/design.md`](docs/design.md) the engine
underneath. The `docs/plan-*.md` files are the plans the iterations
followed.
