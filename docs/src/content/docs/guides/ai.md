---
title: A local model in a step
description: The built-in #ai asks a model on your own machine, through itero-ai, for a text, one of a list, or typed fields.
---

The built-in action `#ai` asks a language model in a step: to sum up a
mail, to sort it, to pull the amount and the date out of an invoice. The
model runs on your own machine, through **itero-ai**, a program of its
own beside itero. Nothing leaves the machine.

## Installing

itero ships no model, and itero-ai only when you want it:

```sh
brew install aegea-dev/tap/itero-ai
```

or its archive from the [release page](https://github.com/aegea-dev/itero/releases).
Without it, `#ai` steps fail saying so, and nothing else changes.

Then a model: any GGUF file. itero finds the ones you have already, in
LM Studio's folder and Hugging Face's cache, or downloads one:

```sh
itero ai models add unsloth/Qwen3.5-4B-GGUF
itero ai models list
```

itero advises **Qwen3.5-4B** (Apache-2.0): about 2.7 GB as `Q4_K_M`,
fast enough on a laptop. See [`itero ai`](/commands/ai/).

## `#ai` ■

```json
{ "#ai": { "args": { "model": "unsloth/Qwen3.5-4B-GGUF",
                     "text": "Sum up this mail in one line: $output.body" } } }
```

`$output` is the answer, as text. Two other kinds of answer hold the
model to a form, so the next steps can rely on it:

**One of a list:** `$output` is exactly one of the labels.

```json
{ "#ai": { "args": { "model": "unsloth/Qwen3.5-4B-GGUF", "text": "$output.body",
                     "choose": ["urgent", "later", "spam"] } } }
```

**Fields:** `$output` is an object with exactly those fields, of those
types.

```json
{ "#ai": { "args": { "model": "unsloth/Qwen3.5-4B-GGUF", "text": "$output.body",
                     "fields": { "urgent": "boolean", "amount": "number", "due": "date" } } } }
```

A condition reads them as any step's result:

```json
{ "#notify": { "on": [{ "$output.urgent": true }],
               "args": { "title": "Urgent", "text": "$output.body" } } }
```

### Its args

| Arg | Is |
|---|---|
| `model` | the model: `<org>/<repo>`, with `:<quantization>` or without (`itero ai models list` shows them); needed |
| `text` | what is asked; needed |
| `system` | the system message: who the model is to be |
| `choose` | a list of labels: the answer is one of them |
| `fields` | names and their types, `text`, `number`, `integer`, `boolean` or `date`: the answer is an object of them |
| `thinking` | `true` lets the model think first; its thinking goes to the log, not into `$output` |
| `temperature`, `top_p`, `top_k`, `min_p`, `presence_penalty`, `max_tokens`, `seed` | sampling, sent as given |

`choose` and `fields` go one at a time. Without a quantization, the
model is the repo's only file, else its `Q4_K_M`, else an error naming
the ones there.

What suits a model is written in the workflow: each model's makers say
which sampling it likes. Qwen3.5-4B's, without thinking:

```json
"temperature": 0.7, "top_p": 0.8, "top_k": 20, "min_p": 0, "presence_penalty": 1.5
```

Sampling not given is llama.cpp's own: `temperature` 0.8, `top_k` 40,
`top_p` 0.95, `min_p` 0.05. An answer has 16,384 tokens of context, its
prompt included.

### While it answers

A running `#ai` step's progress is its token count (`[48 tokens]`), so a
long answer is not stopped as quiet; the action's `limits` apply as to
any action. `itero check` checks its args before any run, and warns of a
model this machine does not have.

## itero-ai

The daemon starts itero-ai when an `#ai` step needs it, and keeps it
while steps ask. After five minutes with nothing asked it exits, and the
model's memory is free again; the next step starts it again, which takes
a second or two. One that ends badly is started again after a pause
that doubles each time.

It is OpenAI's Chat Completions API on a Unix socket only you can read,
not a port: other tools can talk to it as to OpenAI, with `curl
--unix-socket`. What it does is told as events
([The daemon's own workflows](/guides/maintenance/)): `itero.ai.started`,
`itero.ai.stopped`, `itero.ai.failed`, `itero.ai.model.loaded`, and
`itero.ai.model.missing` when a step names a model the machine does not
have.

## Testing without a model

With `ITERO_AI_SOCKET` set in the daemon's environment, `#ai` steps ask
whatever answers on that socket, and no itero-ai is started: a small
server of your test's, giving fixed answers in OpenAI's shape (Bun serves
on a Unix socket: `Bun.serve({ unix: path, fetch })`). See the `ai-mail`
example.
