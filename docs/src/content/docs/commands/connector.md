---
title: ws connector
description: Your connectors, each with the args the workflows give it.
---

`itero ws cn` (`itero ws connector` in full) shows your connectors. A
connector is a program that keeps running and turns what it sees into
events ([Connectors](/guides/connectors/)). Its args are given by the
workflows that listen to it, in their `on` entry
([A connector's args](/guides/secrets/#a-connectors-args)).

```sh
itero ws cn list
```

It takes `--json`, to print JSON for scripts instead of a table.

## list

Every connector, once for each set of args the workflows give it: the
type of the events it sends (its file name), its file, and those args, as
written.

```
$ itero ws cn list
mail.received   connectors/mail.received   {"MAILBOX":"sales"}
mail.received   connectors/mail.received   {"MAILBOX":"support"}
```

The daemon runs one copy of the connector per line
([One connector, many sets of args](/guides/connectors/#one-connector-many-sets-of-args)).
