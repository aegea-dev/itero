---
title: Running and troubleshooting
description: "How itero-ai starts, idles and fails; its events, its environment, common errors."
---

## Its life

- **Started on demand.** The daemon starts itero-ai when an `#ai` step
  needs it, never before: a workspace with no `#ai` step never runs it.
- **Kept while asked.** One itero-ai serves the workspace's steps, one
  answer at a time, with one model in memory. On the machine's daemon
  ([One itero per machine](/guides/machine/)), one itero-ai serves every
  workspace it runs, and its events go to each of them;
  `itero.ai.model.missing` goes only to the workspace whose step asked.
- **Gone when idle.** After five minutes with nothing asked, it frees the
  model's memory and exits. The next `#ai` step starts it again, in a
  second or two.
- **Gone with the daemon.** When the daemon stops, so does its itero-ai.

## When it ends badly

When itero-ai crashes, the step it was answering fails, and the next
`#ai` step starts it again, after a pause: one second, then doubling
each time it ends badly again, up to a minute. After five calm minutes
the pause is back to one second. After ten bad ends in ten minutes the
daemon gives up, and `#ai` steps fail with its last words until those
ten minutes have passed.

Its last line on stderr is the `why` of the `itero.ai.failed` event,
and of the error later `#ai` steps get during the pause.

## Its events

What happens to itero-ai is told as events, which a workflow can wait
on like any other ([The daemon's own workflows](/guides/maintenance/)):

| Event | When | Fields |
|---|---|---|
| `itero.ai.started` | the daemon started it | `socket` |
| `itero.ai.stopped` | it exited after its idle time | `after` (seconds) |
| `itero.ai.failed` | it ended any other way | `exit`, `why` |
| `itero.ai.model.loaded` | it loaded a model | `model`, `seconds` |
| `itero.ai.model.missing` | an `#ai` step named a model this machine does not have | `model`, `workflow` |

A notification when it fails:

```json
{
  "on": [{ "event": "itero.ai.failed" }],
  "steps": [
    { "#notify": { "args": { "title": "itero-ai failed", "text": "$why" } } }
  ]
}
```

The daemon's [`/health`](/reference/http/) has an `ai` field once it has
started one: `state` (`running`, `stopped` or `failed`), the `model`
loaded, and its recent `failures`.

## Its environment

itero-ai has no commands and no flags: the daemon starts it with three
environment variables, all needed.

| Variable | Is |
|---|---|
| `ITERO_AI_SOCKET` | the Unix socket to answer on |
| `ITERO_AI_IDLE` | seconds with nothing asked before it exits |
| `ITERO_AI_INDEX` | the models list, `~/.cache/itero/models.json` |
| `ITERO_AI_LLAMA_LOG` | set to anything: llama.cpp's own log on stderr, for troubleshooting |

To run one yourself, for another tool or to see what it says:

```sh
ITERO_AI_SOCKET=/tmp/my-ai.sock ITERO_AI_IDLE=600 \
ITERO_AI_INDEX=~/.cache/itero/models.json itero-ai
```

It prints what happens on stdout, one JSON object a line
(`{"event":"started"}`, then `{"event":"model.loaded","model":"unsloth/Qwen3.5-4B-GGUF:Q4_K_M","seconds":0.70}`),
and exits when idle, or when the process that started it ends.

Two variables change what itero does:

| Variable | Is |
|---|---|
| `ITERO_AI_BIN` | the itero-ai program to start; without it, the one beside `itero`, else on `PATH` |
| `ITERO_AI_SOCKET` | in the daemon's environment: `#ai` steps ask whatever answers on this socket, and no itero-ai is started (see [Testing](/ai/testing/)) |

## Common errors

**`itero-ai is not installed: brew install aegea-dev/tap/itero-ai, or its archive from the release page`**

The daemon found no itero-ai beside `itero` or on its `PATH`. A daemon
started at login (`itero service`) may have a shorter `PATH` than your
shell: install itero-ai beside itero, as Homebrew does, or set
`ITERO_AI_BIN`.

**`no model <name> on this machine: looking for models now; run it again in a minute`**

The step names a model that is not in the list. The daemon looks for
models again at once; when the file is there, the step works a minute
later. Check the name with `itero ai models list`, and see
[Models](/ai/models/) for where files are found.

**`itero-ai ended badly (<why>); it starts again in <n> s`**

It crashed, and is in its pause ([When it ends badly](#when-it-ends-badly)).
The `why` is its last line; with `ITERO_AI_LLAMA_LOG` set, a run by hand
says more.

**`path must be shorter than SUN_LEN`**

A socket's path is at most 104 bytes on macOS (108 on Linux). The
daemon's sockets are short; this comes from an `ITERO_AI_SOCKET` set by
hand in a deep folder. Use a short path, such as one in `/tmp`.

**`<socket>: an itero-ai answers there already`**

Another itero-ai serves that socket. Use it, or another socket.

**The first answer is slow**

The model is read into memory on the first request: about a second for
Qwen3.5-4B on an M1 Max, longer for a large model or a slow disk. Later
answers do not wait for it, until itero-ai exits when idle.
