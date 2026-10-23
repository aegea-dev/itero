---
title: itero check
description: Reads every file and says what is wrong.
---

```sh
itero check
```

Reads every workflow, action and source. It prints each workflow with its
id, its entry in `itero.config.json`, its settings, its runs so far and its last run, then the actions and
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

Under an action, it notes what the action needs and the `PATH` lacks:

```
  #make-thumbnails  /Users/me/home/actions/photos/make-thumbnails
    may need magick: not found on PATH
  #summarize  /Users/me/home/actions/python/summarize
    needs uv: not found on PATH
```

`needs` is the program its first line names; `may need`, for a shell script,
a command it runs (a hint: the script may guard it). Neither fails the
check. See [Actions](/guides/actions/#when-something-it-needs-is-missing).

Anything wrong is an error naming the file: a key it does not know, a
message type of one word, an action or a workflow that is not there, an
action that is not executable. Run it after every edit.
