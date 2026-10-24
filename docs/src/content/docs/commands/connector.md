---
title: connector
description: Your connectors, and their args.
---

<span class="con">▲</span> `itero connector` shows your connectors and sets the data they start
with. A connector is a program that keeps running and turns what it sees
into events ([Connectors](/guides/connectors/)).

```sh
itero connector list
itero connector args get <connector>
itero connector args set <connector> '<args>'
```

Each one takes `--json`, to print JSON for scripts instead of a table.

## list

Every connector: the type of the events it sends (its file name), its file,
and whether it has args.

```
$ itero connector list
mail.received   connectors/mail.received
```

## args

A connector's args: the data it starts with, kept in `itero.config.json`
under `connector.<name>.args`. An object: a connector runs once.

```sh
itero connector args set mail.received '{"user": "me@example.com", "password": "$$ME_PASS"}'
itero connector args get mail.received
```

`set` prints the file it wrote. The daemon starts a connector with its
args when it starts; restart the daemon after changing them. See
[Args and secrets](/guides/secrets/).
