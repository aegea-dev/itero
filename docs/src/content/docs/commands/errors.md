---
title: itero errors
description: The runs that ended in error, and why.
---

```sh
itero errors [--limit N]
```

The runs that ended in error, newest first: when, the workflow, the run,
the step that broke and why. `--limit N` shows N of them (20). Internal
workflows' errors are here too.

```
$ itero errors
2026-10-21T19:29:01Z  page-digest   01a15ff0-3886-7420-b470-b8028e8d0757  #send-mail: SMTP_PASS is not set: no secret, no environment variable
```

[`itero runs <name>`](/commands/runs/) tells one of them step by step.
