---
title: itero runs
description: The runs, or one run step by step.
---

```sh
itero runs [--all]
itero runs <id or name>
```

The runs, newest first: id, name, workflow, status, started and ended. A
status is `running`, `waiting`, `done`, `failed`, `error` or `split`.
The internal workflows' runs show only with `--all`.

```
$ itero runs
01a16000-9448-7276-8f7e-ac6e8a326b9e  fair-glacier   greet   done   2026-10-21T19:46:53Z  2026-10-21T19:46:53Z
```

With a run's id or name, its story: how it started, each step and how it
went, the messages it took, how it ended, and its state.

```
$ itero runs fair-glacier
fair-glacier  01a16000-9448-7276-8f7e-ac6e8a326b9e  greet  done
  2026-10-21T19:46:53Z  started  {"name":"Ayşe"}
  2026-10-21T19:46:53Z  #hello  done  {"text":"Merhaba, Ayşe!"}
  2026-10-21T19:46:53Z  #notify  done  {}
  2026-10-21T19:46:53Z  ended done
  state {"name":"Ayşe","text":"Merhaba, Ayşe!"}
```

A name is two random words. Of two runs with one name it shows the newest,
and says so; give the id for another. Ended runs are kept for `keep` (a
day), then forgotten. See [The log, IDs and names](/guides/log/).
