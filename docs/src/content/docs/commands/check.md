---
title: ws check
description: Reads your files before anything runs, and says what is wrong.
---

`itero ws check` reads your workflows, actions and connectors and tells you
what is wrong with them, before anything runs. Run it after you edit a
file. (To see runs that already failed, use
[`itero ws wf runs --errors`](/commands/workflow/#runs).)

```sh
itero ws check [--json]
```

It reads every workflow, action and connector. It prints each workflow with its
id, its entry in `itero.config.json`, its runs so far and its last run, then the actions and
workflows its steps name and where each is; then each connector. Internal
workflows come last, apart.

```
$ itero ws check
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

Under `#ai`, a line for each of its steps says where it sends: `local:
<model>` for itero-ai, `local: <url>` for a server on this machine
(`localhost`, `127.0.0.1`, `[::1]`), `remote: <url>` for any other host,
one on your own network too, and for a url known only in a run
(`remote: $event.url`). A step with no `url` of its own sends where
the machine's defaults say. See
[A model behind a URL](/ai/workflows/#a-model-behind-a-url).

```
  #ai  built in
    local: unsloth/Qwen3.5-4B-GGUF
    remote: https://api.openai.com/v1/chat/completions
```

It reports every problem in every file in one run, not only the first: a
key it does not know, an event type of one word, an action or a workflow
that is not there, an action that is not executable, a manifest entry that
is wrong. Each is its message, then where it is, as an
[address](/reference/concepts/#addresses); then how many, and the check
fails (exit 1), printing nothing else:

```
$ itero ws check
error: it needs $message (home/workflow/posta/steps/#ai)
error: no action ship in the actions folders (home/workflow/order/steps/#ship)
error: not JSON: expected `,` or `}` at line 7 column 3 (home/workflow/report)
warning: key looks like a token: write it as a $$NAME (home/workflow/sort/steps/#ai/2)
3 errors, 1 warning
```

Two things stop it inside one file: JSON that does not parse (nothing past
the error can be read), and a value of the wrong shape, like `steps` that
is not a list (nothing inside it is checked, so one mistake is told once).
Run it after every edit.

`--json` gives the same as `errors` and `warnings`, each a `message` and
its address `at`, beside what it could read.

A warning does not fail the check. It says what is likely a mix-up: a
secret that is also an environment variable, an `#ai` key written out that
looks like a token (write it as a `$$NAME`), or an arg `#notify` does not
read (it takes `title`, `text`, `subtitle`, `sound`, `url`, `buttons`,
`wait` and `data`):

```
warning: #notify takes title, text, subtitle, sound, url, buttons, wait and data, not Price (home/workflow/price/steps/#notify)
```
