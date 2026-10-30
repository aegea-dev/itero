# itero

## Project

itero is a Rust CLI and a service daemon for workflows.

## Required Context

Before substantial work, read:

- CLAUDE.md and CLAUDE.local.md files if they exist
- README.md
- docs/src/content/docs/reference/concepts.md: the six parts itero is made of
- Relevant product docs under docs/src/content/docs/

Private/local context may exist outside this repository, but it must not be quoted, committed, or published.

## Safety Rules

- Never blanket-stage files. Use explicit paths only.
- Never stage more than 30 files in one pass.
- Do not read or expose local secrets unless explicitly asked.
- Do not delete anything without approval.

## Where Things Are

- `src/flow.rs`: workflow files, steps, conditions, and how values are read and filled.
- `src/workspace.rs`: the workspace and its manifest, `itero.config.json`.
- `src/engine/`: the daemon (`daemon.rs`), how a run moves (`steps.rs`), running an action (`worker.rs`), the journal, events, schedules, secrets, the HTTP API, the log.
- `src/cli.rs` and `src/cli/`: the commands.
- `src/watch/`: watched paths (inotify on Linux; kqueue and FSEvents on macOS).
- `src/connectors/`: starting and watching connectors.
- `src/ai/`: the built-in `#ai` step and model finding; `itero-ai/` runs a local model, `itero-ai-index/` lists the models a machine has.
- `src/builtin/`: the daemon's own workflows; `src/bin/itero-notifier.rs`: Itero.app's notifier.
- `tests/`: integration tests, one file per area.
- `examples/`: workspaces you can run; some have their own tests in `test/`, and `tests/examples.rs` checks them all.
- `docs/`: the docs site (Astro); its pages are in `docs/src/content/docs/`.
- `packaging/`: Homebrew, macOS app, licence notices; `scripts/`: what the Linux, Docker, install and formula tests run.

## Build And Test

- Build the debug binary: make build-dev
- Build the release binary: make build
- Format before committing: make fmt (not bare `cargo fmt`: it runs rustfmt on every .rs file, also those outside the module tree)
- Check formatting: make fmt-check
- Type-check every target: make check
- Run tests: make test
- Lint (list clippy warnings): make lint
- Lint gate, any warning fails: make lint-check
- Tests and clippy on Linux, in Docker: make test-linux
- Tests that need Docker (container actions; IMAP, POP3 and Gmail against GreenMail): make test-docker
- Security audit of Cargo.lock: make audit
- Test coverage per file: make coverage
- Full verification (fmt-check, check, lint-check, test, audit, docs-build): make verify
- Install docs deps: make docs-install
- Build docs: make docs-build
- An example's own tests: `bun test` in `examples/<name>`, with `ITERO_BIN` set to an absolute path to `target/debug/itero`
- Every target: make help

Environment variables for tests:

- `ITERO_NO_SECRET_STORE=1`: no OS secret store; `$$NAME` comes from `.env` and the environment.
- `ITERO_NOTIFIER=/usr/bin/true`: notifications go nowhere.
- `ITERO_NOTIFY_LOG=<file>`: each notification is a line in that file instead (title, text, then `name=value`, tab separated).
- `ITERO_TEST_DIE_AT=<point>`, with `ITERO_TEST_DIE_ONCE` or `ITERO_TEST_DIE_WHILE` (a file's path): kills the process at that point, for the kill tests.
- `ITERO_STUCK_AFTER=<seconds>`: how long a pass may run before the watchdog calls its workspace stuck.
- `ITERO_AI_BIN`: which itero-ai to start.

CI (`test.yml`) runs fmt-check, check, lint-check and test on both
ubuntu-latest and macos-latest, and test-docker on ubuntu-latest. A Mac
compiles out the `cfg(target_os = "linux")` code, so when a change touches
Linux paths or shared code, run `make test-linux` (Docker) before pushing;
when it touches the mail examples or connectors, `make test-docker`.

## Development Rules

- Rust toolchain is pinned in rust-toolchain.toml.
- Prefer Makefile targets where they exist.
- Verify with emitting event and running workflow before claiming work is complete.
- Tests come first: write the failing unit/integration test, then implement the feature or fix (TDD).
- Work each ticket in its own git worktree under .claude/worktrees/.
- .claude/worktrees/ is shared: never modify or delete a worktree you did not create.
