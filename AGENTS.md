# itero

## Project

itero is a Rust CLI and a service daemon for workflows.

## Required Context

Before substantial work, read:

- CLAUDE.md and CLAUDE.local.md files if they exist
- README.md
- Relevant product docs under docs/src/content/docs/

Private/local context may exist outside this repository, but it must not be quoted, committed, or published.

## Safety Rules

- Never blanket-stage files. Use explicit paths only.
- Never stage more than 30 files in one pass.
- Do not read or expose local secrets unless explicitly asked.
- Do not delete anything without approval.

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
