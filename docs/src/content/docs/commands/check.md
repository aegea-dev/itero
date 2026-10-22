---
title: itero check
description: Reads every file and says what is wrong.
---

```sh
itero check
```

Reads every workflow, action and source. It prints each workflow with its
id, its `meta`, its runs so far and its last run, then the actions and
workflows its steps name and where each is; then each source. Internal
workflows come last, apart.

```
$ itero check
greet
  id 01a16000-8c57-75e1-b8ee-cf8f8d2199d1  since 2026-10-21T19:46:51Z
  file workflows/greet.json
  runs so far 1
  last run fair-glacier done  2026-10-21T19:46:53Z
  #hello  /Users/me/home/actions/hello
  #notify  built in
```

Anything wrong is an error naming the file: a key it does not know, a
message type of one word, an action or a workflow that is not there, an
action that is not executable. Run it after every edit.
