---
title: "#ai in a workflow"
description: The built-in #ai asks a model on your own machine for a text, one of a list, or typed fields.
---

The built-in action `#ai` asks a language model in a step: to sum up a
mail, to sort it, to pull the amount and the date out of an invoice. The
model runs on your own machine, through [itero-ai](/ai/). Nothing leaves
the machine.

## `#ai` ■

```json
{ "#ai": { "args": { "model": "unsloth/Qwen3.5-4B-GGUF",
                     "text": "Sum up this mail in one line: $event.body" } } }
```

`$output` is the answer, as text. Two other kinds of answer hold the
model to a form, so the next steps can rely on it:

**One of a list:** `$output` is exactly one of the labels.

```json
{ "#ai": { "args": { "model": "unsloth/Qwen3.5-4B-GGUF", "text": "$event.body",
                     "choose": ["urgent", "later", "spam"] } } }
```

**Fields:** `$output` is an object with exactly those fields, of those
types.

```json
{ "#ai": { "args": { "model": "unsloth/Qwen3.5-4B-GGUF", "text": "$event.body",
                     "fields": { "urgent": "boolean", "amount": "number", "due": "date" } } } }
```

A condition reads them as any step's result:

```json
{ "#notify": { "on": [{ "$output.urgent": true }],
               "args": { "title": "Urgent", "text": "$event.subject" } } }
```

For `choose` and `fields`, the model is held to the form in two ways:
a grammar lets it write nothing else, and a sentence added to `text`
tells it the form (`Answer as a JSON string: "urgent", "later" or
"spam".`). A small model held by the grammar alone, without being told,
answers poorly.

## Its args

| Arg | Is |
|---|---|
| `model` | the model: `<org>/<repo>`, with `:<quantization>` or without ([Models](/ai/models/)); needed |
| `text` | what is asked; needed |
| `system` | the system message: who the model is to be |
| `choose` | a list of labels: the answer is one of them |
| `fields` | names and their types, `text`, `number`, `integer`, `boolean` or `date`: the answer is an object of them |
| `thinking` | `true` lets the model think first; its thinking goes to the log, not into `$output` |
| `temperature`, `top_p`, `top_k`, `min_p`, `presence_penalty`, `max_tokens`, `seed` | sampling, sent as given |

`choose` and `fields` go one at a time. Without a quantization, the
model is the repo's only file, else its `Q4_K_M`, else an error naming
the ones there.

What suits a model goes in the workflow: each model's makers say which
sampling it likes. [Qwen3.5-4B](/ai/qwen3.5-4b/) lists its own, by mode.
Sampling not given is llama.cpp's own: `temperature` 0.8, `top_k` 40,
`top_p` 0.95, `min_p` 0.05. An answer has 16,384 tokens of context, its
prompt included.

## While it answers

A running `#ai` step's progress is its token count (`[48 tokens]`), so a
long answer is not stopped as quiet; the action's `limits` apply as to
any action. `itero check` checks its args before any run, and warns of a
model this machine does not have.

A step naming a model the machine does not have fails, and the daemon
looks for models again at once: run it again a minute later. See
[Running and troubleshooting](/ai/running/).
