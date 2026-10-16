---
title: Triggers
description: Schedules and app events that start runs, and a workspace's own triggers.
---

A workflow's `triggers` say when a run starts. Each trigger has one of
`schedule` and `on`, and may give the run `args`.

```json
"triggers": [
  { "schedule": "*/5 * * * *", "args": { "mailbox": "" } },
  { "on": "mail.check-now" }
]
```

## Schedules

`schedule` is a cron line in the machine's local time: minute, hour, day of
month, month, day of week. `*/5 * * * *` is every 5 minutes; `0 9 * * 1-5`
is 9:00 on weekdays.

The daemon keeps each schedule's last slot in the journal. A daemon that
was stopped for a while runs a missed schedule once when it starts again,
not once per slot it missed.

## Events

`on` is an event type, like `photo.added`: lowercase words joined by dots.
An event of that type sent to the workspace starts one run per matching
trigger, and the event's `data` is the run's `input`. Starting runs and
handling the event happen in one transaction, so an event delivered twice
starts nothing twice. [Apps and events](/guides/apps/) says how to send
one, and a [source](/guides/sources/) sends them as things happen: a file
changing, mail arriving.

A workflow whose `concurrency` is full queues or skips the start (see
[Workflow files](/reference/workflow-file/)), and a paused workflow starts
nothing.

## A workspace's own triggers

A workflow from somewhere else (an app's folder, a shared repository) says
when it runs, but the workspace may know better. `triggers/<workflow>.json`
in the workspace adds triggers to the workflow's own:

```json title="triggers/mail-check.json"
{ "triggers": [ { "on": "mail.check-now" } ] }
```

or, with `"replace": true`, is used instead of them:

```json title="triggers/mail-check.json"
{ "replace": true,
  "triggers": [ { "schedule": "*/10 * * * *", "args": { "mailbox": "/var/mail/me" } } ] }
```

The file's name is the workflow's `name`. A workflow's version leaves its
triggers out: when it runs is the workspace's business, not part of what a
run is pinned to.
