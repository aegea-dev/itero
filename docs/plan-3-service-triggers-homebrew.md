# itero, iteration 3: start at login, workspace triggers, Homebrew. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A workspace's daemon starts at login and stays up (`itero service install`), a workspace sets when its workflows run without editing them (`triggers/`), and `brew install` installs itero with `Itero.app`.

**Architecture:** The service and the watchdog come from primum (commit f20debf, `src/engine/service.rs` and `src/engine/watchdog.rs`) nearly as they are: a per-user launchd agent or systemd user unit runs `itero watchdog` in the workspace, which runs `itero daemon` and restarts it when it exits, stops answering `/health`, or stops making passes. Workspace triggers are files read when the workspace lists its workflows, so everything downstream (schedules, event triggers, the API) sees one merged list. The Homebrew formula installs the release archives `make dist` makes; the worker also finds `Itero.app` in the formula's `libexec`.

**Tech Stack:** Rust 2024 (toolchain 1.96.0); new crate `service-manager ~0.11.0` (check its license with `cargo info`: MIT or Apache-2.0). Homebrew (Ruby formula), GitHub Actions.

**Spec:** `docs/design.md` (sections 1, 4 and Installing).

## Global Constraints

- Crates: MIT or Apache-2.0 only (check with `cargo info <crate>`); requirements written `~x.y.z`.
- Test first; run the tests and read the output before saying something works.
- Fixtures use non-ASCII text (ı, ş, ğ, ü, é) in neutral words, not personal names.
- Public text (comments, README, docs, commits): no AI attribution, no em dash.
- Stage files by name, never `git add -A`. Commit locally; no push, no GitHub repo.
- The daemon decides nothing; one transaction per answer; secrets never reach events, the journal, logs or command lines.
- Outside the workspace, itero keeps only what the person asks for: the service's registration (and `Itero.app`, where they install it).
- `make fmt`, `make fmt-check`, `make lint-check` clean before each commit; Linux checked with Docker Desktop (`desktop-linux`; never colima).
- Anything that changes this Mac's setup (a LaunchAgent, Homebrew taps or formulas) is tried only with the owner's yes at that step, and undone after.

## Decisions (made for this plan)

- **Commands**: `itero service install [--interval S]`, `itero service uninstall`, `itero service status`, run in a workspace (they find it as every command does). `itero watchdog` is hidden: it is what the service runs. `itero daemon` stays what a person runs in a terminal.
- **One service per workspace**, label `dev.itero.daemon-<first 12 hex of SHA-256 of the workspace path>`; the port is the workspace's (itero.toml), so two workspaces need two ports. The service gets the installing shell's `PATH` (a service starts with a bare one, which would not find `mail`, `uv` or `bun`), works in the workspace, and logs to `<workspace>/.itero/daemon.log` (inside `.itero/`, never synced). `install` again replaces it (after a PATH or port change).
- **`service status`** prints the service's state and the daemon's `/health` (pid, passes, last pass, last error), or `no answer on port <port>`.
- **Workspace triggers**: `triggers/<workflow>.json` (folder from `itero.toml` `triggers = "triggers"`, the default), `{"replace": false, "triggers": [...]}`. `replace: true` uses the file's triggers instead of the workflow's own; otherwise they are added. A file for a workflow there is not is an error naming the file. Same checks as a workflow's own triggers.
- **A workflow's version leaves its triggers out**: when a workflow runs is the workspace's business, not part of what a run is pinned to. Changing a schedule starts no new version.
- **A schedule's key is `<workflow>#<schedule>`** (not its place in the list), so reordering or merging triggers keeps each schedule's last slot. Two identical schedules on one workflow are one. Existing journals (only hand-tried ones so far) wait one slot for a renamed key: acceptable, no migration.
- **Homebrew**: a formula `itero` in the owner's tap (today `erhangundogan/homebrew-tap`; it moves with the org). Bottles are not built: it installs the release archives (`on_macos` arm and intel, `on_linux` intel and arm), `bin.install "itero"`, and on macOS `libexec.install "Itero.app"`. The worker looks for `Itero.app` in `<the itero binary's real folder>/../libexec/` too, after the ones beside it. Caveats: notifications come from itero (macOS asks once), and `itero service install` in a workspace starts it at login (not `brew services`: it runs one service, and itero runs one per workspace).
- **The formula lives in this repo** (`packaging/homebrew/itero.rb`); the release workflow's `tap` job writes the version and the four checksums into the tap's copy, as videre's does (`TAP_TOKEN`).

## File structure (new and changed)

```
src/engine/watchdog.rs     from primum: decide, pause, run (itero daemon)
src/engine/service.rs      from primum: install_ctx, install, uninstall, status
src/workspace.rs           triggers folder; workflows() merges workspace triggers
src/workflow/mod.rs        version() leaves triggers out
src/engine/daemon.rs       schedule key <workflow>#<schedule>
src/engine/worker.rs       notifier(): ../libexec/Itero.app too
src/cli.rs                 itero service install|uninstall|status, itero watchdog
packaging/homebrew/itero.rb    the formula
scripts/test-formula.sh    installs the formula from a local tap (macOS, with the owner's yes)
.github/workflows/release.yml  tap job
tests/watchdog.rs, tests/service.rs, tests/workspace.rs, tests/daemon.rs
```

---

### Task 1: Workspace triggers, stable schedule keys, versions without triggers

**Files:**
- Modify: `src/workspace.rs`, `src/workflow/mod.rs` (`version`), `src/engine/daemon.rs` (`fire_schedules`)
- Test: `tests/workspace.rs`, `tests/daemon.rs`

**Interfaces:**
- Consumes: `workflow::format::Trigger`, `workflow::validate` (trigger checks, factored into `pub fn check_trigger(t: &Trigger) -> Result<(), String>`).
- Produces: `Workspace.triggers: PathBuf`; `Workspace::workflows()` returns each workflow with its triggers merged; `workflow::version(f)` ignores `f.triggers`; schedule keys `<workflow>#<schedule>`.

- [ ] **Step 1: Write the failing tests** (`tests/workspace.rs`)

```rust
#[test]
fn workspace_triggers_add_to_or_replace_a_workflows_own() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    let own = r#"{ "name": "posta", "triggers": [ { "schedule": "*/5 * * * *" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti" } ] }"#;
    write(d.path(), "workflows/posta.json", own);
    let ws = workspace::find(d.path()).unwrap();
    let triggers = |ws: &workspace::Workspace| ws.workflows().unwrap()[0].1.triggers.clone();
    assert_eq!(triggers(&ws).len(), 1);
    write(d.path(), "triggers/posta.json",
        r#"{ "triggers": [ { "on": "posta.geldi", "args": { "kutu": "gelen kutusu" } } ] }"#);
    let t = triggers(&ws);
    assert_eq!(t.len(), 2, "added");
    assert_eq!(t[1].on.as_deref(), Some("posta.geldi"));
    write(d.path(), "triggers/posta.json",
        r#"{ "replace": true, "triggers": [ { "schedule": "0 9 * * *", "args": { "kutu": "iş" } } ] }"#);
    let t = triggers(&ws);
    assert_eq!(t.len(), 1, "replaced");
    assert_eq!(t[0].schedule.as_deref(), Some("0 9 * * *"));
    assert_eq!(t[0].args["kutu"], "iş");
}

#[test]
fn a_trigger_file_for_no_workflow_or_a_bad_trigger_is_an_error_naming_it() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    write(d.path(), "workflows/posta.json", &flow("posta", SET));
    write(d.path(), "triggers/yok.json", r#"{ "triggers": [ { "on": "x" } ] }"#);
    let ws = workspace::find(d.path()).unwrap();
    let e = format!("{:#}", ws.workflows().err().unwrap());
    assert!(e.contains("triggers/yok.json") && e.contains("no workflow yok"), "{e}");
    std::fs::remove_file(d.path().join("triggers/yok.json")).unwrap();
    write(d.path(), "triggers/posta.json", r#"{ "triggers": [ { "schedule": "her gün" } ] }"#);
    let e = format!("{:#}", ws.workflows().err().unwrap());
    assert!(e.contains("triggers/posta.json") && e.contains("her gün"), "{e}");
}

#[test]
fn a_workflows_version_leaves_its_triggers_out() {
    let a = itero::workflow::parse(r#"{ "name": "akis", "triggers": [ { "schedule": "*/5 * * * *" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti" } ] }"#).unwrap();
    let mut b = a.clone();
    b.triggers.clear();
    assert_eq!(itero::workflow::version(&a), itero::workflow::version(&b));
}
```

and in `tests/daemon.rs`:

```rust
#[test]
fn a_schedule_keeps_its_last_slot_when_triggers_are_reordered() {
    let d = tempfile::tempdir().unwrap();
    let flow = |triggers: &str| format!(r#"{{ "name": "sayac", "triggers": {triggers},
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ {{ "from": "basla", "to": "bitti",
        "do": [ {{ "action": "memory.set@1", "with": {{ "son": "çalıştı" }} }} ] }} ] }}"#);
    let ws = workspace(d.path(), "sayac", &flow(r#"[ { "schedule": "*/5 * * * *" } ]"#));
    run_once(&ws, t("2026-10-15T14:01:00Z"), exe()).unwrap();
    // an event trigger put first: the schedule's place changes, its key does not
    std::fs::write(d.path().join("workflows/sayac.json"),
        flow(r#"[ { "on": "elle" }, { "schedule": "*/5 * * * *" } ]"#)).unwrap();
    run_once(&ws, t("2026-10-15T14:05:10Z"), exe()).unwrap();
    assert_eq!(Journal::open(&ws).unwrap().runs().unwrap().len(), 1, "the 14:05 slot ran");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test workspace --test daemon`
Expected: FAIL (`triggers` ignored; versions differ; the reordered schedule is seen as new and waits).

- [ ] **Step 3: Implement**

- `workflow/validate.rs`: move the trigger checks into `pub fn check_trigger(t: &Trigger) -> Result<(), String>`; `validate` calls it for each trigger (`map_err(&whole)`).
- `workflow/mod.rs` `version`: `f.triggers.clear();` beside the descriptions.
- `workspace.rs`: `Settings.triggers: String` (default `"triggers"`), `Workspace.triggers: PathBuf` (`folder(root, ..)`).

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerFile {
    #[serde(default)]
    replace: bool,
    triggers: Vec<workflow::format::Trigger>,
}
```

  In `workflows()`, after the workflows are read: for each `*.json` in `self.triggers` (a missing folder holds none), parse it (error: `<file>: <serde message>`), find the workflow named by the file's stem (error: `<file>: no workflow <stem>`), check each trigger with `check_trigger` (error: `<file>: <message>`), then `if tf.replace { w.triggers = tf.triggers } else { w.triggers.extend(tf.triggers) }`.
- `daemon.rs` `fire_schedules`: `let key = format!("{}#{schedule}", f.name);` and skip a key already fired this pass (two identical schedules are one): keep a `BTreeSet<String>` of keys seen.

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workspace.rs src/workflow/mod.rs src/workflow/validate.rs src/engine/daemon.rs tests/workspace.rs tests/daemon.rs
git commit -m "Workspace triggers: triggers/<workflow>.json adds to or replaces a workflow's own"
```

---

### Task 2: The watchdog

**Files:**
- Create: `src/engine/watchdog.rs`, `tests/watchdog.rs`
- Modify: `src/engine/mod.rs` (`pub mod watchdog;`), `src/cli.rs` (hidden `Watchdog` command)

**Interfaces:**
- Produces: `engine::watchdog::{Rules, Probe, Watch, Verdict, decide, pause, Options, run}` exactly as primum's (f20debf), `Options.exe` described as "this itero binary", the daemon started as `<exe> daemon <daemon_args>`. CLI: `itero watchdog [--interval S] [--check-every S] [--misses N] [--stale-after S] [--log FILE]`, its port the workspace's.

- [ ] **Step 1: Copy the tests**

`git -C ~/projects/primum show f20debf:tests/watchdog.rs > tests/watchdog.rs`; change `primum::` to `itero::`. Its restart test starts a real daemon: rewrite its setup to make an itero workspace (`itero.toml` with a free port, found by binding `127.0.0.1:0` and dropping the listener) instead of a primum board, and to kill the daemon by the pid in `/health`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test watchdog`
Expected: FAIL to compile (no `itero::engine::watchdog`).

- [ ] **Step 3: Copy the code**

`git -C ~/projects/primum show f20debf:src/engine/watchdog.rs > src/engine/watchdog.rs`; rename `primum` to `itero` in its comments. CLI arm (from primum's `Cmd::Watchdog`): `daemon_args` are `--interval <S>`; the port is `ws.port`; the watchdog runs in the workspace (`current_dir` is set by the service).

- [ ] **Step 4: Run the tests**

Run: `cargo test --test watchdog`
Expected: PASS (the decide and pause tests, and the real restart).

- [ ] **Step 5: Commit**

```bash
git add src/engine/watchdog.rs src/engine/mod.rs src/cli.rs tests/watchdog.rs
git commit -m "The watchdog: keeps itero daemon running, restarting it when it stops"
```

---

### Task 3: `itero service install`, `uninstall`, `status`

**Files:**
- Create: `src/engine/service.rs`, `tests/service.rs`
- Modify: `Cargo.toml` (`service-manager = "~0.11.0"`), `src/engine/mod.rs`, `src/cli.rs` (`Service { cmd: ServiceCmd }`)

**Interfaces:**
- Consumes: `watchdog` (task 2), `Workspace`.
- Produces: `engine::service::{label(root: &Path) -> ServiceLabel, log_file(root: &Path) -> PathBuf, install_ctx(root: &Path, exe: &Path, interval: u64, path: Option<String>) -> ServiceInstallCtx, install(ctx) -> Result<()>, uninstall(root) -> Result<bool>, status(root) -> Result<String>}`.

- [ ] **Step 1: Write the failing test** (`tests/service.rs`, after primum's)

```rust
use itero::engine::service::{install_ctx, label, log_file};
use std::path::Path;

#[test]
fn the_service_runs_the_watchdog_in_the_workspace() {
    let root = Path::new("/Users/biri/iş akışları");
    let exe = Path::new("/opt/homebrew/bin/itero");
    let ctx = install_ctx(root, exe, 2, Some("/opt/homebrew/bin:/usr/bin".into()));
    assert_eq!(ctx.program, exe);
    let args: Vec<String> = ctx.args.iter().map(|a| a.to_string_lossy().into()).collect();
    assert_eq!(args, ["watchdog", "--interval", "2", "--log", "/Users/biri/iş akışları/.itero/daemon.log"]);
    assert_eq!(ctx.working_directory.as_deref(), Some(root));
    assert_eq!(ctx.environment, Some(vec![("PATH".to_string(), "/opt/homebrew/bin:/usr/bin".to_string())]));
    assert!(ctx.autostart);
    assert_eq!(log_file(root), root.join(".itero/daemon.log"));
}

#[test]
fn each_workspace_has_its_own_label() {
    let a = label(Path::new("/Users/biri/ev")).to_qualified_name();
    let b = label(Path::new("/Users/biri/iş")).to_qualified_name();
    assert_ne!(a, b);
    assert!(a.starts_with("dev.itero.daemon-") && a.len() == "dev.itero.daemon-".len() + 12, "{a}");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --test service`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Copy primum's `src/engine/service.rs` (f20debf); change: the label prefix `dev.itero.daemon-` with 12 hex characters; `log_file` is `root/.itero/daemon.log` (make the folder at install); `install_ctx` drops `port` (the watchdog reads it from itero.toml) and passes `watchdog --interval <S> --log <file>`.

CLI (from primum's `DaemonCmd` arms):
- `install`: `service::install(service::install_ctx(&ws.root, &current_exe()?, interval, std::env::var("PATH").ok()))`, then print `installed: <label> runs itero in <root>; starts at login`, `log: <file>`, `API: http://127.0.0.1:<port>/`, and `it keeps this shell's PATH; install again after changing it`.
- `uninstall`: `stopped and removed`, or `no service for <root>`.
- `status`: `service: <state>`, then `daemon: pid <p>, <n> passes, last <t>[, last error: <e>]` from `/health` (reqwest, 3 s timeout), or `daemon: no answer on port <port>`.

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Try it on this Mac (with the owner's yes)**

In `examples/mail-check` (its `.itero/` is ignored): `itero service install`; check `launchctl print gui/$(id -u)/<label>` shows it running, `itero service status` answers, `curl -s 127.0.0.1:8787/health`; `kill` the daemon's pid and see the watchdog restart it (`.itero/daemon.log`); then `itero service uninstall` and check the LaunchAgent plist is gone from `~/Library/LaunchAgents`. Linux: `systemctl --user` is not in the test container; the systemd side is covered by `service-manager` and `install_ctx`'s test.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/engine/service.rs src/engine/mod.rs src/cli.rs tests/service.rs
git commit -m "itero service install, uninstall, status: a workspace's daemon starts at login"
```

---

### Task 4: The Homebrew formula

**Files:**
- Create: `packaging/homebrew/itero.rb`, `scripts/test-formula.sh`
- Modify: `src/engine/worker.rs` (`notifier`), `tests/worker.rs`, `.github/workflows/release.yml` (`tap` job), `Makefile` (`test-formula`)

**Interfaces:**
- Consumes: `make dist` (iteration 2), the release assets' names.
- Produces: the formula; `notifier()` also finds `<canonical exe dir>/../libexec/Itero.app`.

- [ ] **Step 1: Write the failing test** (`tests/worker.rs`, unix): a temp `prefix/bin/itero` (a copy of `CARGO_BIN_EXE_itero`) and `prefix/libexec/Itero.app/Contents/MacOS/itero-notifier` (the stand-in script from `notify_runs_the_notifier_with_the_title_and_text_as_arguments`); run the copied binary's `worker` with a `notify@1` job, no `ITERO_NOTIFIER`, and on macOS assert `by` is `itero-notifier` and the stand-in got the title and text. (On Linux the test is skipped: `#[cfg(target_os = "macos")]`.)

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --test worker libexec`
Expected: FAIL on macOS (`by` is `osascript`).

- [ ] **Step 3: Implement** in `notifier()`: the candidates become `[env, beside, libexec, home, /Applications]`, where `libexec` is `std::env::current_exe()?.canonicalize()?.parent()?.join("../libexec/Itero.app")` (canonicalize: Homebrew's `bin/itero` is a link into the Cellar).

- [ ] **Step 4: The formula** (`packaging/homebrew/itero.rb`)

```ruby
# Homebrew formula for itero: installs the release archives (made by
# `make dist`, checked by their SHA-256), not a build from source. The
# release workflow writes the version and checksums into the tap's copy.
class Itero < Formula
  desc "Runs workflows again and again: schedules, events, steps, workers"
  homepage "https://github.com/erhangundogan/itero"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/erhangundogan/itero/releases/download/v#{version}/itero-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/erhangundogan/itero/releases/download/v#{version}/itero-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/erhangundogan/itero/releases/download/v#{version}/itero-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_arm do
      url "https://github.com/erhangundogan/itero/releases/download/v#{version}/itero-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "itero"
    libexec.install "Itero.app" if OS.mac?
  end

  def caveats
    <<~EOS
      Start a workspace's daemon at login, in that workspace:
        itero service install

      On macOS, notifications come from itero (Itero.app, in this formula's
      libexec); macOS asks once to allow them.
    EOS
  end

  test do
    assert_match "itero #{version}", shell_output("#{bin}/itero --version")
  end
end
```

(The zeros are placeholders only in this repository's copy: the release's `tap` job writes the real ones into the tap. `scripts/test-formula.sh` writes real ones for its local archive.)

- [ ] **Step 5: `scripts/test-formula.sh`** (macOS; changes Homebrew, so run only with the owner's yes): `make dist TAG=v0.1.0` into a temp folder; copy the formula with its macOS `url` for this machine's arch replaced by `file://<archive>` and its `sha256` by the archive's; `brew tap-new --no-git itero-test/local`; copy the formula into that tap's `Formula/`; `brew install itero-test/local/itero`; check `$(brew --prefix)/bin/itero --version` and `$(brew --prefix itero)/libexec/Itero.app`; then `brew uninstall itero` and `brew untap itero-test/local` (also on failure: a `trap`). Makefile: `test-formula: ## Install the formula from a local tap (macOS; changes Homebrew, undone after)`.

- [ ] **Step 6: The `tap` job** in `release.yml`, after `publish`, as videre's: download the four `.sha256` assets, clone `erhangundogan/homebrew-tap` with `TAP_TOKEN`, copy `packaging/homebrew/itero.rb` to `Formula/itero.rb` with `version` and the four `sha256` lines written (in the file's order: macOS arm, macOS intel, Linux intel, Linux arm; assert four were written), commit `itero <version>` and push when it changed; fail loudly without `TAP_TOKEN`.

- [ ] **Step 7: Run**

Run: `cargo test --test worker` (macOS, then Linux in Docker), and with the owner's yes `make test-formula`.
Expected: PASS; `test-formula` prints `formula test: ok` and leaves no tap or formula behind.

- [ ] **Step 8: Commit**

```bash
git add packaging/homebrew/itero.rb scripts/test-formula.sh src/engine/worker.rs tests/worker.rs .github/workflows/release.yml Makefile
git commit -m "Homebrew: a formula from the release archives, Itero.app in its libexec"
```

---

### Task 5: Docs, and everything on both systems

**Files:**
- Modify: `README.md`, `docs/design.md`, `install.sh` (its closing hint mentions `itero service install`)

- [ ] **Step 1: README**: "Start at login" (`itero service install`, `status`, `uninstall`; one per workspace, the workspace's port; the log; PATH), "Workspace triggers" (a short `triggers/mail-check.json` example setting the mailbox and a 10 minute schedule, with `replace`), `brew install erhangundogan/tap/itero` beside the curl line.
- [ ] **Step 2: design.md**: section 1 (the service and the watchdog, from primum), section 4 (workspace triggers are here now; versions leave triggers out; schedule keys), Installing (Homebrew), the iterations line.
- [ ] **Step 3: Everything**

Run: `cargo test --no-fail-fast && make fmt-check && make lint-check && make test-install`, then `cargo test --no-fail-fast` and `make test-install` in the Linux container.
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/design.md install.sh
git commit -m "README and design: start at login, workspace triggers, Homebrew"
```
