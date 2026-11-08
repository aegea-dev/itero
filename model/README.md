# The engine's model

`Engine.tla` is a TLA+ model of how the daemon works a workspace's
journal: messages routed to workflows, a run's events given to its
manager, jobs run by workers, `itero ws emit`, `ev cancel` and `wf stop`
from outside, and crashes at any moment. Each transaction on the journal
is one action, so a daemon dying between any two writes is one more order
to try.

TLC, the TLA+ model checker, tries every order of these within each
scenario's bounds and checks the rules below in every state it reaches.
A rule holds in all of them, or TLC prints the steps that break it. The
model checks the design, not the Rust: when how the engine handles
events, runs or jobs changes, the model changes in the same pull request.

## Running it

Docker is all it needs; the script downloads `tla2tools.jar` once into
`target/model/` and checks its SHA-256.

```bash
make model
```

runs every scenario with its rules, and fails if any rule breaks.

```bash
make model-test
```

breaks the model on purpose once per rule (the constant `Bug`) and
expects that rule to fail: a rule that cannot fail checks nothing.

## Reading a trace

TLC prints each state from the first, with the action that led to it:
its name, or, for an action over the journal's rows, its line in `Next`
(each line there is one action). In a state, `ev` is the events table by
seq and `runs` the runs table by id; a subject of `0` is `itero.app`
(messages), `-1` is `itero.worker` (jobs), and any other is a run's id.
`running`, `cancel`, `held` and `deferred` are the daemon's memory, lost
when it dies; `orphans` are the workers of a daemon that died, until they
see it.

## Scenarios

Each scenario fixes the workflows and how many of each outside thing may
happen; its `.cfg` lists its rules.

| Scenario | Workflows | What may happen |
|---|---|---|
| `MCLost` | one action | a stop, a crash, 3 workers killed, a manager killed |
| `MCStop` | an action and a group of two, started by one message | a stop, a cancel, a crash, a worker and a manager killed |
| `MCWait` | two steps that each wait for a message | 2 messages, a stop, a cancel |
| `MCHeld` | a step that waits, started twice under a run limit of 1 | 2 messages, a cancel, a crash |
| `MCChild` | a workflow whose step is a workflow of 2 rounds, and a built-in on `itero.run.done` | a stop |
| `MCWindow` | two workflows on one message, one with a window that may hold it back | 2 messages, a cancel, a crash |

All finish in seconds but `MCStop`, which takes a few minutes.

## Rules

Numbered as in `Engine.tla`. "Eventually" holds while the daemon keeps
running.

| # | Name | Rule |
|---|---|---|
| 1 | `HandledOnce` | An event is handled at most once. |
| 2 | `CancelledReachesNone` | A cancelled message starts no run and reaches none. |
| 3 | `EndedStays` | A run that ended never changes again. |
| 4 | `JobRunsBounded`, `StoppedSaysNothing` | A job runs at most `MAX_RUNS` times, and a stopped job's result never reaches its run. |
| 5 | `OneCopy` | At most one copy of a job runs at a time. |
| 6 | `LimitKept` | A message starts runs only while fewer than `limits.runs` are under way (one message may start several; children and next rounds are not held). |
| 7 | `TakenOnce` | A run takes each message at most once. |
| 8 | `InOrder` | A run's events reach its manager in the order they were written. |
| 9 | `OneEnd` | A parent gets one `workflow.ended` per child step, after the child's last round. |
| 10 | `EndedNoTimer` | An ended run has no timers. |
| 11 | `AllHandled` | Eventually, every event is handled. |
| 12 | `ChildrenReport` | Eventually, every child's end reaches its parent, unless the parent has ended. |
| 13 | `RunsEnd` | Eventually, every run ends or waits for a message. |
| 14 | `NoChain` | A run whose chain started on `itero.run.done` says nothing when done. |

## What it leaves out

- Values: `$name`, args, outputs are tokens. Time: a message's time is
  its order, and a due timer is an event that may come at any moment.
- What a window decides: any windowed workflow may be held back from any
  message (`unique`, `changed`, `at_most` choose by values and time); the
  model checks that a message held back is still handled once, in the
  same transaction.
- A failure inside a group side by side (a group's actions only finish);
  a repeated step (the same step twice in a row stands for it); cron;
  chains on `itero.run.error` and `itero.run.failed`.
- A waiting step always has a timeout, so no run waits forever.
- One daemon per journal is taken as given (`flock`); the model has one.
- Forgetting old rows (`prune`), the watchdog, schema migrations,
  `--all`, connectors, HTTP and AI.

## The map to the Rust

| Action | Rust |
|---|---|
| `Emit` | `itero ws emit`, `journal.rs` `publish` |
| `CancelEvent` | `journal.rs` `cancel` |
| `StopRun` | `events.rs` `stop_run` |
| `Take`, `Route` | `daemon.rs` `work` (`next`) and `route`; `journal.rs` `route` |
| `Deliver` | `daemon.rs` `deliver`; `steps.rs` `decide` (`Decide`); `journal.rs` `handle` and `messages_since` |
| `ManagerKilled` | `daemon.rs` `ask_again`, `Delivered::Deferred` |
| `FireTimer` | `daemon.rs` `fire_timers`; `journal.rs` `fire_timer` |
| `StartJob` | `daemon.rs` `work` (its jobs); `journal.rs` `start_job` |
| `EndWorker` | `daemon.rs` `end_one`, `save_result` |
| `KillWorker` | `end_one` with `Failure::Lost`; `journal.rs` `lose_job` |
| `Crash`, `Restart` | the daemon's process ends; `lock.rs` `take_file` |
| `OrphanDies` | `worker.rs` `stop_with_parent` |
