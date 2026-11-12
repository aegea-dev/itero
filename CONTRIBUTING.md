# Contributing to itero

itero is a command-line tool and a service daemon that runs workflows again
and again: schedules, events, steps and workers. It is free software. It is
written and maintained by one person, and its direction is set by that person.

**Bug reports are genuinely useful and always welcome.** A clear reproduction
with a real workspace is worth more than most patches.

**If you hit a real problem in your own workflows, or want a feature you would
use yourself, you are welcome here.** Those are the contributions worth having,
and they are worth having because they come from someone who actually needs the
result.

What this project is not looking for is contribution for its own sake. There is
no good-first-issue list and no ambition to grow a contributor base.

For anything beyond a small fix, open an issue and agree the approach before
writing code. Direction is deliberate and not every change fits, and a rejected
pull request wastes your time more than mine, which is a bad trade for both of
us. If you want something itero is not going to do, the licence lets you fork
it, and that is a legitimate outcome rather than a failure.

## Building and testing

```bash
make build-dev    # target/debug/itero
make test         # the test suite
make fmt          # format; use this, not bare `cargo fmt`
make fmt-check    # what CI runs
make lint-check   # clippy, warnings fail
make test-linux   # the tests and clippy on Linux, in Docker
make bench        # how fast events are taken and runs end (make bench-linux: in Docker)
make verify       # every gate: fmt, check, clippy, tests, audit, docs
```

CI runs formatting, `cargo check`, clippy and the tests on Ubuntu and macOS.
A Mac compiles out the Linux-only code, so run `make test-linux` when a change
touches Linux paths or shared code. The Rust version is pinned in
`rust-toolchain.toml`; change it nowhere else.

Worth knowing before changing anything:

- **Open a PR against `main`; checks must be green.**
- **One change per PR.** Put the reasoning in commit messages and keep the PR
  description short.
- **A behaviour change comes with a test that failed before it.**
- **Stage files by name, never `git add -A`.**
- **Behaviour changes update the docs in the same commit.** User-facing
  documentation lives in `docs/src/content/docs/`.

## Licensing of contributions

itero is licensed under the Apache License 2.0. If a contribution of yours is
merged, you confirm that:

- the contribution is your own work, or you otherwise have the right to submit
  it,
- it is contributed under the Apache License 2.0, like the rest of the project,
- you grant the project maintainer, and their successors and assigns, a
  perpetual, worldwide, non-exclusive, transferable and sublicensable right to
  use your contribution, **including the right to distribute it under different
  licence terms in future versions of itero**, and
- you grant the project maintainer, and their successors and assigns, a
  perpetual, worldwide, non-exclusive, royalty-free, irrevocable patent licence
  to make, use, offer to sell, sell, import and otherwise transfer your
  contribution, alone or combined with the project, with the same right to
  sublicense it under any licence the project later adopts. This patent licence
  ends for any party who initiates patent litigation claiming the contribution
  or the project infringes.

**You keep the copyright in what you write.** This exists so the project can
change its licence later without having to find and ask every past contributor,
a problem that has stalled other projects for years. It takes nothing away from
you: your contribution remains available under the Apache License 2.0 in every
release it has appeared in, permanently.
