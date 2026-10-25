---
title: workflow
description: Your workflows, their runs, and their settings.
---

`itero workflow` (or `itero wf`) shows your workflows and what they
did. Use it to see which workflows you have, to look at their runs, and to
find out why a run went wrong.

```sh
itero wf list
itero wf runs [<run>] [--errors] [--workflow W] [--action A] [--limit N] [--all]
itero wf config get <workflow>
itero wf config set <workflow> '<settings>'
```

Each one takes `--json`, to print JSON for scripts instead of a table.

## list

Every workflow on a line: its file, what starts it, how many runs it had,
and how the last one went. The last line says whether the daemon is running
and how many events (messages) wait for it.

```
$ itero wf list
greet   workflows/greet.json   on greet.someone                    runs 1   last: done 1s ago
order   workflows/order.json   on order.placed                     runs 2   last: error 1s ago
keep    built in               on itero.space.full, at 0 * * * *   runs 0   last: none           internal
logs    built in               on itero.log.full                   runs 0   last: none           internal
daemon: not running, no event waiting
```

When nothing seems to happen, look here first: an event waiting and no
daemon running means [`itero daemon`](/commands/daemon/) is not running.

## runs

The runs, newest first: id, name, workflow, status, when it started and
ended. A run that broke also says at which step, and why.

```
$ itero wf runs
01a16fd5-46c1-77fa-88da-39c36126cd9c   sunny-moss    order   error   2026-10-24T21:33:30Z   2026-10-24T21:33:30Z   #send-mail: exit 1: smtp: connection refused
01a16fd5-46c0-7738-98d2-f87b1211af19   sunny-lake    greet   done    2026-10-24T21:33:30Z   2026-10-24T21:33:30Z
```

A status is `running`, `waiting`, `done`, `failed`, `error` or `split` (a
run that went on as one run per item of a list, see
[Args](/guides/secrets/#args)).

| Option | Shows |
|---|---|
| `--errors` | only the runs that ended in error |
| `--workflow W` | only workflow `W`'s runs |
| `--action A` | only the runs that broke at action `A` (see also [`itero action errors`](/commands/action/#errors)) |
| `--limit N` | at most `N` runs |
| `--all` | the internal workflows' runs too |

With a run's name or id, its story: how it started, each step and how it
went, the events it took, how it ended, and its state.

```
$ itero wf runs sunny-lake
sunny-lake  01a16fd5-46c0-7738-98d2-f87b1211af19  greet  done
  2026-10-24T21:33:22Z  started  {"name":"Ayşe"}
  2026-10-24T21:33:22Z  #hello  done  {"text":"Merhaba, Ayşe!"}
  2026-10-24T21:33:22Z  ended done
  state {"name":"Ayşe","text":"Merhaba, Ayşe!"}
```

A name is two random words. Of two runs with one name it shows the newest,
and says so; give the id for another. Ended runs are kept for `keep` (a
day), then forgotten. See [The log, IDs and names](/guides/log/).

`itero wf runs` shows what already ran. To find mistakes in your files
before anything runs, use [`itero check`](/commands/check/).

## config

A workflow's settings: the values its runs start with, kept in
`itero.config.json` under `workflow.<name>.settings`.

```sh
itero wf config set sites '{"sites": ["https://example.com", "http://localhost:3000"]}'
itero wf config set disk --file disk-settings.json
itero wf config get sites
```

`set` takes a JSON object and prints the file it wrote; the rest of the
file is kept as it was. `get` prints them. A run's state starts with its
workflow's settings, then the event's data on top: a step names one as
`"$sites"`, an action gets it as `$sites`. To turn a workflow off, or keep
it to some dates, its entry says `"off": true`, `from` or `until`
([itero.config.json](/reference/itero-config/)). See
[Settings](/guides/workflows/#settings).
