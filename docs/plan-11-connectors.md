# Plan 11: sources become connectors

A source is a program that keeps running and brings messages in. Some will
also send to other apps over the connection they keep open (a chat bot, an
MQTT client), so the name moves to the one that fits both ways:
**connector**, drawn ▲. This plan renames; sending out is a plan of its
own.

## Spec

```
itero.config.json
workflows/       ● what runs, step by step
actions/         ■ one step: runs once and ends
connectors/      ▲ keeps running: each line it prints is a message
```

- **The folder:** `connectors/` replaces `sources/`, and
  `folders.connectors` in `itero.config.json` replaces `folders.sources`
  (default `["connectors"]`). A connector is, as a source was, an
  executable whose file name is the type of the messages it prints
  (`connectors/gmail.received`), two or more words joined by dots, never
  `itero.*`.
- **Its entry:** `connector.<name>` replaces `source.<name>` (its
  `description`, `tags`, own keys and `args`). `itero args <name>` writes
  a connector's args when the workspace has a connector of that name.
- **What the daemon says:** the log's `connector.started`,
  `connector.ended` and `connector.gave-up`; itero's own message
  `itero.connector.gave-up` (`connector`, `why`); `/health` lists
  `connectors`, each with its `state`, `restarts` and `last_exit`. A
  message from one has the origin `connector/<name>`.
- **The commands:** `itero which` and `itero check` name connectors;
  `itero watch` is "for connectors".
- **No shim:** nobody depends on `sources/` yet. A `sources/` folder, a
  `source` entry or `folders.sources` is an error from `itero check` and
  the daemon, saying what it is now (as `args/` and `config/` were).
- **How a message type reads:** `<who>[.<detail>...].<event>`. The first
  word is who: a service, an app, a thing (`gmail`, `stripe`, `order`,
  `itero`); words between add detail (`stripe.invoice.paid`); the last is
  what: past tense for a fact, something that happened (`gmail.received`,
  `order.placed`), imperative for a request, something asked for
  (`gmail.send`, `app.notify`). So the tense says the way: facts come in,
  requests go out. A convention the messages guide teaches; itero still
  checks only two or more words joined by dots.
- **Unchanged:** an event's origin field is still named `source` (where a
  message came from, CloudEvents' word), and Terraform's `source` in the
  terraform example is Terraform's.
- **Not in this plan:** sending out through a connector (itero writing
  messages to its stdin). Its design waits for its own spec.

## Tasks

1. **The workspace.** `src/workspace.rs`: `folders.connectors`,
   `connector_files`, `connector_args`, the manifest key `connector` (in
   `RUNNING`), and the errors for `sources/`, `source` and
   `folders.sources`. Tests in `tests/workspace.rs` first: the new names
   read; each old one is an error naming its new name.
2. **The engine.** `src/sources/` becomes `src/connectors/`
   (`supervise.rs`), `SourceHealth` `ConnectorHealth`, `source_args` and
   `source_env` in `src/engine/daemon.rs` `connector_args` and
   `connector_env`; the log events, the own message, the origin
   `connector/<name>`, `/health`'s `connectors`, and the reserved prefix
   in `events::engine_own`. `tests/sources.rs` and
   `tests/source_args.rs` become `tests/connectors.rs` and
   `tests/connector_args.rs`, their assertions on the new names written
   before the code moves.
3. **The command line.** `src/cli.rs`: `itero args`, `itero which`,
   `itero check` (its manifest check and the old names' errors), `itero
   watch`'s help. `tests/cli.rs` first.
4. **Examples and docs.** `examples/gmail` and `examples/mail-alert` move
   their programs to `connectors/` and their entries to `connector`; the
   photos README. The docs: the messages guide's "Sources" section
   becomes "Connectors" (its title "Messages and connectors") and gains
   how a message type reads (who, detail, fact or request), the
   glossary, `itero.config.json`, HTTP (`/health`), the maintenance
   guide's own messages, the commands (`args`, `check`, `daemon`,
   `watch`, `which`), concepts, install, quickstart, the README, and
   CI's comment. `make verify`, `make test-docker` (the gmail example)
   and `make test-linux`.

## Decided

- **A connector's file name stays the type it prints**
  (`connectors/gmail.received`: who, then the fact), so nothing new is
  declared. When connectors send too, its entry says which requests it
  takes (`"takes": ["gmail.send"]`), without renaming the file.
