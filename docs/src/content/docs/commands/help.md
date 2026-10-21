---
title: itero help
description: The commands, or one command's options.
---

```sh
itero help
itero help <command>
itero <command> --help
itero --version
```

`itero help` lists the commands, by name; `itero help <command>` shows one
command's arguments and options, as does `--help` after it.

```
$ itero help emit
Send a message: it starts runs and reaches the runs waiting for it

Usage: itero emit <TYPE> [DATA]

Arguments:
  <TYPE>  Its type, two or more words joined by dots, like order.placed
  [DATA]  Its data, a JSON object
```

`itero --version` prints the version: `itero 0.3.0`.
